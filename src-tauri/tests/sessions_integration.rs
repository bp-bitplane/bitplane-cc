//! cc's session tabs against a real bitplane-sessiond, served in-process on a
//! temp host so a test can stop it and start the next instance.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bitplane_cc_lib::daemon::{DaemonLink, DaemonStatus};
use bitplane_cc_lib::sessions::{
    FRAME_OUTPUT, FRAME_SNAPSHOT, OutputSink, SessionConfig, SessionError, SessionEvent, SessionId,
    SessionInfo, SessionStatus, Sessions,
};
use bitplane_sessiond::client::Client;
use bitplane_sessiond::env::EnvMap;
use bitplane_sessiond::paths::Dirs;
use bitplane_sessiond::server::{Config, ServeError, serve};
use serde_json::json;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

/// The daemon's per-attachment window (sessiond ADR 0026).
const WINDOW: u64 = 128 * 1024;

#[tokio::test(flavor = "multi_thread")]
async fn a_tab_types_into_its_daemon_session() {
    let host = Host::new();
    let _daemon = host.serve().await;
    let cc = Cc::connect(&host).await;

    let info = cc.sessions.open("demo", &host.plane).await.unwrap();
    let screen = cc.attach(info.id);
    cc.sessions
        .write(info.id, b"echo \"plane=$BITPLANE_PLANE_ID\" $((6*7))\r")
        .unwrap();

    screen.wait_for("plane=demo 42").await;
    assert_eq!(info.plane_id, "demo");
    assert_eq!(info.key.len(), 36);
    assert_eq!(info.status, SessionStatus::Running);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reload_reattaches_with_a_snapshot_of_the_scrollback() {
    let host = Host::new();
    let _daemon = host.serve().await;
    let cc = Cc::connect(&host).await;
    let info = cc.sessions.open("demo", &host.plane).await.unwrap();
    let before = cc.attach(info.id);
    cc.sessions
        .write(info.id, b"for i in 1 2 3; do echo line-$i-$((i*7)); done\r")
        .unwrap();
    before.wait_for("line-3-21").await;

    let after = cc.attach(info.id);

    after.wait_for("line-1-7").await;
    assert_eq!(after.snapshots(), 1);
    cc.sessions
        .write(info.id, b"echo after-$((6*7))\r")
        .unwrap();
    after.wait_for("after-42").await;
    assert!(!before.text().contains("after-42"));
}

#[tokio::test(flavor = "multi_thread")]
async fn sessions_of_other_clients_get_tabs_and_closing_a_tab_closes_them() {
    let host = Host::new();
    let _daemon = host.serve().await;
    let cc = Cc::connect(&host).await;
    let other = host.other_client().await;

    let opened = other
        .call(
            "session.open",
            Some(json!({ "root": host.plane, "plane": "demo" })),
        )
        .await
        .unwrap();
    let key = opened["session"]["id"].as_str().unwrap().to_owned();
    cc.wait_until("the session has a tab", |tabs| {
        tabs.iter()
            .any(|tab| tab.key == key && tab.plane_id == "demo")
    })
    .await;
    let id = cc.sessions.list()[0].id;

    cc.sessions.close(id).await.unwrap();

    assert!(cc.sessions.list().is_empty());
    let listed = other.call("session.list", None).await.unwrap();
    assert_eq!(listed["sessions"], json!([]));
    cc.wait_for_event(|e| e.id == id && e.session.is_none())
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rename_names_the_tab_apart_from_the_program_title() {
    let host = Host::new();
    let _daemon = host.serve().await;
    let cc = Cc::connect(&host).await;
    let info = cc.sessions.open("demo", &host.plane).await.unwrap();
    let _screen = cc.attach(info.id);

    let renamed = cc
        .sessions
        .rename(info.id, Some("build".into()))
        .await
        .unwrap();
    cc.sessions
        .write(info.id, b"printf '\\033]2;vim notes\\007'\r")
        .unwrap();

    assert_eq!(renamed.name.as_deref(), Some("build"));
    cc.wait_until("the program set its title", |tabs| {
        tabs[0].title.as_deref() == Some("vim notes") && tabs[0].name.as_deref() == Some("build")
    })
    .await;
    let cleared = cc
        .sessions
        .rename(info.id, Some("  ".into()))
        .await
        .unwrap();
    assert_eq!(cleared.name, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_clean_exit_closes_the_tab() {
    let host = Host::new();
    let _daemon = host.serve().await;
    let cc = Cc::connect(&host).await;
    let info = cc.sessions.open("demo", &host.plane).await.unwrap();
    let _screen = cc.attach(info.id);

    cc.sessions.write(info.id, b"exit 0\r").unwrap();

    cc.wait_for_event(|e| {
        let status = e.session.as_ref().map(|s| &s.status);
        assert_ne!(
            status,
            Some(&SessionStatus::Exited { code: 0 }),
            "shown as exited"
        );
        e.id == info.id && e.session.is_none()
    })
    .await;
    let other = host.other_client().await;
    let listed = other.call("session.list", None).await.unwrap();
    assert_eq!(listed["sessions"], json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_exit_keeps_the_tab_and_restart_starts_it_over() {
    let host = Host::new();
    let _daemon = host.serve().await;
    let cc = Cc::connect(&host).await;
    let info = cc.sessions.open("demo", &host.plane).await.unwrap();
    let screen = cc.attach(info.id);
    cc.sessions
        .write(info.id, b"echo old-$((6*7)); exit 3\r")
        .unwrap();
    screen.wait_for("old-42").await;
    cc.wait_until("the shell exited", |tabs| {
        tabs[0].status == SessionStatus::Exited { code: 3 }
    })
    .await;

    let restarted = cc.sessions.restart(info.id).await.unwrap();

    assert_eq!(restarted.id, info.id);
    assert_eq!(restarted.status, SessionStatus::Running);
    cc.sessions.write(info.id, b"echo new-$((6*7))\r").unwrap();
    screen.wait_for("new-42").await;
    assert_eq!(screen.snapshots(), 2);
    assert!(!screen.text().contains("old-42"));
}

#[tokio::test(flavor = "multi_thread")]
async fn busy_names_the_foreground_program() {
    let host = Host::new();
    let _daemon = host.serve().await;
    let cc = Cc::connect(&host).await;
    let info = cc.sessions.open("demo", &host.plane).await.unwrap();
    let screen = cc.attach(info.id);
    cc.sessions
        .write(info.id, b"echo ready-$((6*7))\r")
        .unwrap();
    screen.wait_for("ready-42").await;
    assert!(!cc.sessions.busy(info.id).await.unwrap().busy);

    cc.sessions.write(info.id, b"sleep 30\r").unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    let busy = loop {
        let busy = cc.sessions.busy(info.id).await.unwrap();
        if busy.busy || Instant::now() > deadline {
            break busy;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert!(busy.busy);
    assert_eq!(busy.process_name.as_deref(), Some("sleep"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_gone_root_is_shown_and_cannot_restart() {
    let host = Host::new();
    let _daemon = host.serve().await;
    let cc = Cc::connect(&host).await;
    let info = cc.sessions.open("demo", &host.plane).await.unwrap();
    let _screen = cc.attach(info.id);
    cc.sessions.write(info.id, b"exit 4\r").unwrap();
    cc.wait_until("the shell exited", |tabs| {
        tabs[0].status == SessionStatus::Exited { code: 4 }
    })
    .await;

    std::fs::remove_dir_all(&host.plane).unwrap();

    cc.wait_until("the root is gone", |tabs| tabs[0].root_gone)
        .await;
    assert_eq!(
        cc.sessions.restart(info.id).await,
        Err(SessionError::RootMissing(host.plane.clone()))
    );
    assert_eq!(
        cc.sessions.open("demo", &host.plane).await,
        Err(SessionError::RootMissing(host.plane.clone()))
    );
    assert_eq!(cc.sessions.plane_sources()[0].directory, host.plane);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_daemon_instance_brings_the_tabs_back_as_fresh_shells() {
    let host = Host::new();
    let first = host.serve().await;
    let cc = Cc::connect(&host).await;
    let a = cc.sessions.open("demo", &host.plane).await.unwrap();
    let b = cc.sessions.open("demo", &host.plane).await.unwrap();
    let screen = cc.attach(b.id);
    cc.sessions.write(b.id, b"echo first-$((6*7))\r").unwrap();
    screen.wait_for("first-42").await;

    first.stop().await;
    cc.wait_for_status(|s| matches!(s, DaemonStatus::Connecting))
        .await;
    let _second = host.serve().await;
    cc.wait_for_status(|s| {
        matches!(
            s,
            DaemonStatus::Connected {
                restarted: true,
                ..
            }
        )
    })
    .await;

    cc.wait_until("both tabs are running again", |tabs| {
        tabs.len() == 2 && tabs.iter().all(|t| t.status == SessionStatus::Running)
    })
    .await;
    let tabs = cc.sessions.list();
    assert_eq!(
        tabs.iter()
            .map(|t| (t.id, t.key.as_str()))
            .collect::<Vec<_>>(),
        [(a.id, a.key.as_str()), (b.id, b.key.as_str())]
    );
    cc.sessions.write(b.id, b"echo second-$((6*7))\r").unwrap();
    screen.wait_for("second-42").await;
    assert_eq!(screen.snapshots(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn output_waits_for_the_webviews_acks() {
    let host = Host::new();
    let _daemon = host.serve().await;
    let cc = Cc::connect(&host).await;
    let info = cc.sessions.open("demo", &host.plane).await.unwrap();
    let screen = cc.attach(info.id);
    cc.sessions
        .write(info.id, b"echo ready-$((6*7))\r")
        .unwrap();
    screen.wait_for("ready-42").await;
    screen.auto_ack.store(false, Ordering::SeqCst);
    let acked = screen.consumed();

    let total = 600_000;
    let command = format!("head -c {total} /dev/zero | tr '\\0' x; echo; echo END-$((6*7))\r");
    cc.sessions.write(info.id, command.as_bytes()).unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;

    let unacked = screen.consumed() - acked;
    assert!(unacked <= WINDOW, "{unacked} bytes arrived unacked");
    screen.auto_ack.store(true, Ordering::SeqCst);
    cc.sessions.ack(info.id, screen.consumed());
    screen.wait_for("END-42").await;
}

/// A host's directories in a short temp dir (macOS caps socket paths at 104
/// bytes), with a plane root and a `HOME` of its own so no profile runs.
struct Host {
    dirs: Dirs,
    plane: PathBuf,
    env: EnvMap,
    _root: tempfile::TempDir,
}

impl Host {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("cc")
            .tempdir_in("/tmp")
            .unwrap();
        let dirs = Dirs::for_version(&root.path().join("run"), &root.path().join("state"));
        let plane = root.path().join("demo");
        let home = root.path().join("home");
        std::fs::create_dir_all(&plane).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let env = EnvMap::from([
            ("SHELL".into(), "/bin/sh".into()),
            ("HOME".into(), home.display().to_string()),
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("PS1".into(), "$ ".into()),
        ]);
        Self {
            dirs,
            plane: plane.canonicalize().unwrap(),
            env,
            _root: root,
        }
    }

    /// Stopped by the test, or when dropped with the runtime.
    async fn serve(&self) -> Daemon {
        let (stop, stopped) = oneshot::channel::<()>();
        let config = Config {
            dirs: self.dirs.clone(),
            listener: None,
            // SAFETY: getuid has no preconditions.
            peer_uid: unsafe { libc::getuid() },
            modules: vec![bitplane_sessiond::session::Sessions::module()],
            env: Some(self.env.clone()),
        };
        let task = tokio::spawn(serve(config, async move {
            let _ = stopped.await;
        }));
        let socket = self.dirs.socket();
        while !socket.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Daemon {
            stop: Some(stop),
            task: Some(task),
        }
    }

    /// Another client of the same daemon, as the dev client would be.
    async fn other_client(&self) -> Client {
        let client = Client::connect(&self.dirs.socket()).await.unwrap();
        client.hello("other").await.unwrap();
        client
    }
}

struct Daemon {
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<(), ServeError>>>,
}

impl Daemon {
    async fn stop(mut self) {
        let _ = self.stop.take().unwrap().send(());
        self.task.take().unwrap().await.unwrap().unwrap();
    }
}

/// cc's backend: its daemon link and its session tabs, with every
/// `session_changed` it emits.
struct Cc {
    _link: Arc<DaemonLink>,
    sessions: Arc<Sessions>,
    statuses: Mutex<mpsc::UnboundedReceiver<DaemonStatus>>,
    events: Mutex<mpsc::UnboundedReceiver<SessionEvent>>,
}

impl Cc {
    async fn connect(host: &Host) -> Self {
        let (status_tx, statuses) = mpsc::unbounded_channel();
        let link = DaemonLink::new(
            Ok(host.dirs.clone()),
            // Starting it does nothing, so only the in-process daemon answers.
            PathBuf::from("/usr/bin/true"),
            Arc::new(move |status| {
                let _ = status_tx.send(status);
            }),
        );
        let (event_tx, events) = mpsc::unbounded_channel();
        let sessions = Sessions::new(
            &link,
            SessionConfig::default(),
            Arc::new(move |event| {
                let _ = event_tx.send(event);
            }),
        );
        link.start();
        let cc = Self {
            _link: link,
            sessions,
            statuses: Mutex::new(statuses),
            events: Mutex::new(events),
        };
        cc.wait_for_status(|s| matches!(s, DaemonStatus::Connected { .. }))
            .await;
        cc.sessions.ready(Duration::from_secs(10)).await;
        cc
    }

    fn attach(&self, id: SessionId) -> Arc<Screen> {
        let screen = Arc::new(Screen::new(Arc::clone(&self.sessions), id));
        self.sessions
            .attach(id, 100, 30, Arc::clone(&screen) as Arc<dyn OutputSink>)
            .unwrap();
        screen
    }

    async fn wait_for_status(&self, matches: impl Fn(&DaemonStatus) -> bool) -> DaemonStatus {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let next = {
                let mut statuses = self.statuses.lock().unwrap();
                statuses.try_recv().ok()
            };
            match next {
                Some(status) if matches(&status) => return status,
                Some(_) => {}
                None if left.is_zero() => panic!("timed out waiting for a daemon status"),
                None => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        }
    }

    async fn wait_for_event(&self, matches: impl Fn(&SessionEvent) -> bool) -> SessionEvent {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let next = self.events.lock().unwrap().try_recv().ok();
            match next {
                Some(event) if matches(&event) => return event,
                Some(_) => {}
                None if Instant::now() > deadline => {
                    panic!("timed out waiting for a session event")
                }
                None => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        }
    }

    async fn wait_until(&self, what: &str, done: impl Fn(&[SessionInfo]) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !done(&self.sessions.list()) {
            assert!(Instant::now() < deadline, "timed out waiting until {what}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

/// What the webview's channel receives, acked as it is drawn.
struct Screen {
    sessions: Arc<Sessions>,
    id: SessionId,
    auto_ack: AtomicBool,
    state: Mutex<ScreenState>,
}

#[derive(Default)]
struct ScreenState {
    /// Everything since the last snapshot, the snapshot included.
    text: String,
    snapshots: u32,
    consumed: u64,
}

impl Screen {
    fn new(sessions: Arc<Sessions>, id: SessionId) -> Self {
        Self {
            sessions,
            id,
            auto_ack: AtomicBool::new(true),
            state: Mutex::default(),
        }
    }

    fn text(&self) -> String {
        self.state.lock().unwrap().text.clone()
    }

    fn consumed(&self) -> u64 {
        self.state.lock().unwrap().consumed
    }

    fn snapshots(&self) -> u32 {
        self.state.lock().unwrap().snapshots
    }

    async fn wait_for(&self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !self.text().contains(needle) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {needle:?}; screen: {:?}",
                self.text()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

impl OutputSink for Screen {
    fn send(&self, bytes: Vec<u8>) -> bool {
        let (kind, data) = bytes.split_first().expect("a kind byte");
        let consumed = {
            let mut state = self.state.lock().unwrap();
            match *kind {
                FRAME_SNAPSHOT => {
                    state.snapshots += 1;
                    state.text.clear();
                }
                FRAME_OUTPUT => {}
                other => panic!("unknown frame kind {other}"),
            }
            state.text.push_str(&String::from_utf8_lossy(data));
            state.consumed += data.len() as u64;
            state.consumed
        };
        if self.auto_ack.load(Ordering::SeqCst) {
            self.sessions.ack(self.id, consumed);
        }
        true
    }
}
