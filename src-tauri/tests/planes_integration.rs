//! The sidebar's planes and plane creation against a real bitplane-sessiond
//! with its plane module, served in-process, running a fake `bp`.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bitplane_cc_lib::contract::BranchIntent;
use bitplane_cc_lib::daemon::{DaemonLink, DaemonStatus};
use bitplane_cc_lib::jobs::{CreateArgs, CreateInfo, CreateStatus, JobEmitters, Jobs, Phase};
use bitplane_cc_lib::planes::Planes;
use bitplane_protocol::plane::PlaneKind;
use bitplane_sessiond::env::EnvMap;
use bitplane_sessiond::paths::Dirs;
use bitplane_sessiond::plane::Plane;
use bitplane_sessiond::server::{Config, serve};
use serde_json::json;
use tokio::sync::{mpsc, oneshot};

const WAIT: Duration = Duration::from_secs(10);

/// `--version` answers; a request on stdin is recorded. `plane_create`
/// writes `progress` to fd 3 and `log` to stderr, waits while `hold`
/// exists, and answers `interrupted.stdout` to a SIGINT. Any action then
/// answers from `<action>.{stderr,stdout,exit}`.
const FAKE_BP: &str = r#"#!/bin/sh
case " $* " in
*" --version "*)
	echo '{"version":"0.1.0","contract":1}'
	exit 0
	;;
esac
request=$(cat)
printf '%s\n' "$request" >> "$FAKE_BP/requests"
action=$(printf '%s' "$request" | sed -n 's/.*"action":"\([a-z_]*\)".*/\1/p')
if [ "$action" = plane_create ]; then
	trap 'cat "$FAKE_BP/interrupted.stdout"; exit 130' INT
	[ -f "$FAKE_BP/log" ] && cat "$FAKE_BP/log" >&2
	[ -f "$FAKE_BP/progress" ] && cat "$FAKE_BP/progress" >&3
	while [ -f "$FAKE_BP/hold" ]; do sleep 0.05; done
	[ -f "$FAKE_BP/log.after" ] && cat "$FAKE_BP/log.after" >&2
fi
[ -f "$FAKE_BP/$action.stderr" ] && cat "$FAKE_BP/$action.stderr" >&2
[ -f "$FAKE_BP/$action.stdout" ] && cat "$FAKE_BP/$action.stdout"
exit "$(cat "$FAKE_BP/$action.exit" 2>/dev/null || echo 0)"
"#;

#[tokio::test(flavor = "multi_thread")]
async fn the_sidebar_follows_the_daemons_plane_list() {
    let host = Host::new().await;
    host.listed(&["alpha"]);
    host.answer(
        "project_list",
        &json!({"action": "project_list", "projects": [
            {"project": "api", "detail": {"source_repo_present": true}}
        ]})
        .to_string(),
        0,
    );
    let cc = Cc::connect(&host).await;

    let model = cc
        .wait_for_model("alpha listed", |m| m.planes.iter().any(|p| p.id == "alpha"))
        .await;
    let alpha = &model.planes[0];
    assert_eq!(alpha.kind, PlaneKind::Plane);
    assert_eq!(alpha.members[0].member, "@api");
    assert_eq!(
        PathBuf::from(&alpha.members[0].path),
        host.planes.join("alpha/api")
    );
    assert_eq!(alpha.findings, ["member_worktree_missing"]);

    cc.planes.create_draft("scratch").await.unwrap();
    cc.wait_for_model("the draft appears", |m| {
        m.planes
            .iter()
            .any(|p| p.id == "scratch" && p.kind == PlaneKind::Draft)
    })
    .await;
    assert_eq!(
        cc.planes.create_draft("scratch").await,
        Err("scratch already exists".into())
    );

    host.listed(&[]);
    std::fs::remove_dir_all(host.planes.join("alpha")).unwrap();
    cc.planes.refresh().await.unwrap();
    cc.wait_for_model("alpha is gone", |m| {
        m.planes
            .iter()
            .any(|p| p.id == "alpha" && p.kind == PlaneKind::Gone)
    })
    .await;

    let projects = cc.planes.projects().await.unwrap();
    assert_eq!(projects[0].name, "api");
    assert!(projects[0].source_repo_present);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_create_streams_its_phase_and_log_and_ends_on_bps_answer() {
    let host = Host::new().await;
    host.set("hold", "");
    host.set("log", "fetching api\n");
    host.set(
        "progress",
        &progress(&[("@api", "fetch", "done"), ("@api", "worktree", "started")]),
    );
    host.set("log.after", "running scripts\n");
    host.created("auth", false, false);
    let cc = Cc::connect(&host).await;

    let job = cc.jobs.create(args("auth")).await.unwrap();

    let running = cc
        .wait_for_create("worktrees are being added", |c| {
            c.phase == Some(Phase::AddingWorktrees) && c.log.contains("fetching api")
        })
        .await;
    assert_eq!(running.job, job);
    assert_eq!(running.status, CreateStatus::Running);
    assert!(running.started_by.starts_with("bitplane-cc "));
    assert_eq!(running.args, Some(args("auth")));
    assert_eq!(cc.jobs.creating()[0].id, "auth");
    let request = host.requests().pop().unwrap();
    assert_eq!(request["members"], json!(["@api", "/repos/web:main"]));
    assert_eq!(request["fetch"], true);

    host.unset("hold");
    let done = cc
        .wait_for_create("bp answered", |c| c.status == CreateStatus::Succeeded)
        .await;
    assert!(done.log.ends_with("running scripts"), "{}", done.log);
    assert!(
        cc.lines
            .lock()
            .unwrap()
            .iter()
            .any(|l| l == "running scripts"),
        "live lines arrive as create_log"
    );
    assert!(cc.jobs.creating().is_empty());

    cc.jobs.dismiss(&job).await.unwrap();
    cc.wait_until("the job is gone", |jobs| jobs.list().is_empty())
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_create_ends_on_bps_interrupted_answer() {
    let host = Host::new().await;
    host.set("hold", "");
    let interrupted = created(&host.planes, "auth", true, false);
    host.set("interrupted.stdout", &interrupted);
    let cc = Cc::connect(&host).await;

    let job = cc.jobs.create(args("auth")).await.unwrap();
    cc.wait_for_create("running", |c| c.status == CreateStatus::Running)
        .await;
    cc.jobs.cancel(&job).await.unwrap();

    let done = cc
        .wait_for_create("bp unwound", |c| c.status != CreateStatus::Running)
        .await;
    assert!(
        matches!(done.status, CreateStatus::NothingCreated { .. }),
        "{:?}",
        done.status
    );
    assert_eq!(
        cc.jobs.cancel(&job).await,
        Ok(()),
        "cancelling again is harmless"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_relaunched_cc_finds_the_running_create_with_its_log() {
    let host = Host::new().await;
    host.set("hold", "");
    host.set("log", "fetching api\nfetching web\n");
    host.created("auth", false, false);
    let first = Cc::connect(&host).await;
    first.jobs.create(args("auth")).await.unwrap();
    first
        .wait_for_create("the log arrived", |c| c.log.contains("fetching web"))
        .await;
    drop(first);

    let second = Cc::connect(&host).await;
    let found = second
        .wait_for_create("the backlog arrived", |c| c.log.contains("fetching web"))
        .await;
    assert_eq!(found.log, "fetching api\nfetching web");
    assert_eq!(found.status, CreateStatus::Running);
    assert!(found.started_by.starts_with("bitplane-cc "));
    assert_eq!(
        found.args, None,
        "the dialog's entries stayed with the first"
    );

    host.unset("hold");
    second
        .wait_for_create("it ends", |c| c.status == CreateStatus::Succeeded)
        .await;
}

fn args(id: &str) -> CreateArgs {
    CreateArgs {
        id: id.into(),
        branch: "feat".into(),
        members: vec![
            bitplane_cc_lib::jobs::Member {
                spec: "@api".into(),
                branch: None,
            },
            bitplane_cc_lib::jobs::Member {
                spec: "/repos/web".into(),
                branch: Some("main".into()),
            },
        ],
        intent: BranchIntent::Resolve,
        no_fetch: false,
        no_scripts: false,
    }
}

fn created(planes: &Path, id: &str, interrupted: bool, remnant: bool) -> String {
    json!({"action": "plane_create", "id": id, "directory": planes.join(id), "members": [],
           "interrupted": interrupted, "remnant": remnant, "scripts": []})
    .to_string()
}

fn progress(steps: &[(&str, &str, &str)]) -> String {
    steps
        .iter()
        .map(|(member, step, state)| {
            format!(
                "{}\n",
                json!({"event": "member", "member": member, "step": step, "state": state})
            )
        })
        .collect()
}

/// A daemon with the plane module, `bp` being the fake above.
struct Host {
    dirs: Dirs,
    control: PathBuf,
    planes: PathBuf,
    _root: tempfile::TempDir,
    _daemon: oneshot::Sender<()>,
}

impl Host {
    async fn new() -> Self {
        // Short paths: macOS caps a socket path at 104 bytes.
        let root = tempfile::Builder::new()
            .prefix("cc")
            .tempdir_in("/tmp")
            .unwrap();
        let path = root.path().canonicalize().unwrap();
        let dirs = Dirs::for_version(&path.join("run"), &path.join("state"));
        let (control, bin, home, planes) = (
            path.join("control"),
            path.join("bin"),
            path.join("home"),
            path.join("planes"),
        );
        for dir in [&control, &bin, &home, &planes] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let bp = bin.join("bp");
        std::fs::write(&bp, FAKE_BP).unwrap();
        std::fs::set_permissions(&bp, std::fs::Permissions::from_mode(0o755)).unwrap();
        let env = EnvMap::from([
            ("PATH".into(), format!("{}:/usr/bin:/bin", bin.display())),
            ("HOME".into(), home.display().to_string()),
            ("SHELL".into(), "/bin/sh".into()),
            ("FAKE_BP".into(), control.display().to_string()),
            ("BITPLANE_PLANES_DIR".into(), planes.display().to_string()),
        ]);
        let (stop, stopped) = oneshot::channel::<()>();
        let config = Config {
            dirs: dirs.clone(),
            listener: None,
            // SAFETY: getuid has no preconditions.
            peer_uid: unsafe { libc::getuid() },
            modules: vec![Plane::module()],
            env: Some(env),
        };
        let host = Self {
            dirs,
            control,
            planes,
            _root: root,
            _daemon: stop,
        };
        host.listed(&[]);
        tokio::spawn(serve(config, async move {
            let _ = stopped.await;
        }));
        while !host.dirs.socket().exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        host
    }

    fn set(&self, name: &str, contents: &str) {
        std::fs::write(self.control.join(name), contents).unwrap();
    }

    fn unset(&self, name: &str) {
        let _ = std::fs::remove_file(self.control.join(name));
    }

    fn answer(&self, action: &str, stdout: &str, exit: i32) {
        self.set(&format!("{action}.stdout"), stdout);
        self.set(&format!("{action}.exit"), &exit.to_string());
    }

    /// `bp list` reports these planes, each with an `@api` member; their
    /// directories exist.
    fn listed(&self, ids: &[&str]) {
        let planes: Vec<_> = ids
            .iter()
            .map(|id| {
                let dir = self.planes.join(id);
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join("plane.toml"), "version = 1\n").unwrap();
                json!({
                    "id": id,
                    "directory": dir,
                    "created_at": null,
                    "members": [{"member": "@api", "path": "api", "head": {"head": "branch", "branch": "main"}}],
                    "health": {"checked": "cheap", "findings": [
                        {"finding": "member_worktree_missing", "member": "@api", "path": "api"}
                    ]},
                })
            })
            .collect();
        let response = json!({"action": "plane_list", "planes": planes});
        self.answer("plane_list", &response.to_string(), 3);
    }

    fn created(&self, id: &str, interrupted: bool, remnant: bool) {
        let stdout = created(&self.planes, id, interrupted, remnant);
        self.answer("plane_create", &stdout, if interrupted { 130 } else { 0 });
    }

    fn requests(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.control.join("requests"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

/// cc's planes and jobs mirrors on their own connection, as one launch of cc.
struct Cc {
    _link: Arc<DaemonLink>,
    planes: Arc<Planes>,
    jobs: Arc<Jobs>,
    lines: Arc<Mutex<Vec<String>>>,
}

impl Cc {
    async fn connect(host: &Host) -> Self {
        let (connected_tx, mut connected) = mpsc::unbounded_channel();
        let link = DaemonLink::new(
            Ok(host.dirs.clone()),
            PathBuf::from("/usr/bin/true"),
            Arc::new(move |status| {
                if let DaemonStatus::Connected { .. } = status {
                    let _ = connected_tx.send(());
                }
            }),
        );
        let planes = Planes::new(&link, Arc::new(|| {}));
        let lines = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&lines);
        let jobs = Jobs::new(
            &link,
            JobEmitters {
                create: Arc::new(|_| {}),
                log: Arc::new(move |event| {
                    let mut lines = log.lock().unwrap();
                    lines.extend(event.text.lines().map(str::to_owned));
                }),
                changed: Arc::new(|| {}),
            },
        );
        link.start();
        tokio::time::timeout(WAIT, connected.recv())
            .await
            .expect("cc connects to the daemon");
        tokio::join!(planes.ready(WAIT), jobs.ready(WAIT));
        Self {
            _link: link,
            planes,
            jobs,
            lines,
        }
    }

    async fn wait_until(&self, what: &str, done: impl Fn(&Jobs) -> bool) {
        let deadline = Instant::now() + WAIT;
        while !done(&self.jobs) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn wait_for_create(&self, what: &str, done: impl Fn(&CreateInfo) -> bool) -> CreateInfo {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(found) = self.jobs.list().into_iter().find(|c| done(c)) {
                return found;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}: {:?}",
                self.jobs.list()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn wait_for_model(
        &self,
        what: &str,
        done: impl Fn(&bitplane_protocol::plane::PlanesModel) -> bool,
    ) -> bitplane_protocol::plane::PlanesModel {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(model) = self.planes.model().filter(|m| done(m)) {
                return model;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}: {:?}",
                self.planes.model()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}
