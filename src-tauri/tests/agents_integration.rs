//! cc's Claude terminals, agent state, resume and the first-launch import
//! against a real bitplane-sessiond with its `agent` module, served
//! in-process. Claude terminals launch a fake `claude`, the only one on the
//! `PATH` the test controls; its hooks run the bundled daemon binary.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bitplane_cc_lib::agents::{AgentEvent, AgentInfo, AgentState, Agents};
use bitplane_cc_lib::daemon::{DaemonLink, DaemonStatus};
use bitplane_cc_lib::migrate::{self, Legacy};
use bitplane_cc_lib::sessions::{
    AgentOpen, OpenOptions, OutputSink, SessionConfig, SessionError, SessionInfo, SessionStatus,
    Sessions,
};
use bitplane_sessiond::agent::{Agents as AgentModule, Timing};
use bitplane_sessiond::env::EnvMap;
use bitplane_sessiond::paths::Dirs;
use bitplane_sessiond::server::{Config, ServeError, serve};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

const BUNDLED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/binaries/bitplane-sessiond-",
    env!("CC_TARGET_TRIPLE")
);

/// Launched as a Claude terminal's PTY child, it checks its argv is exactly
/// `--settings <absolute hooks file> [--resume <id>]` with the hook env set,
/// and exits 64 otherwise. Each launch appends `<cwd> <argv>` to
/// `$FAKE_CLAUDE_LOG/calls`. It starts agent session `$FAKE_CLAUDE_LOG/next-sid`
/// (or the `--resume` one, a moment later, as the real one does) and then
/// turns each typed `prompt`, `stop`, `fail` or `quit [code]` line into that
/// hook. While `$FAKE_CLAUDE_LOG/no-hooks` exists it runs no hooks, as when
/// settings block them. It keeps its `claude agents --json` registry entry
/// in step, in the verified format.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
log="$FAKE_CLAUDE_LOG"
if [ "$1" = agents ] && [ "$2" = --json ]; then
  printf '['
  sep=""
  for entry in "$log"/registry/*.json; do
    [ -f "$entry" ] || continue
    printf '%s' "$sep"; cat "$entry"; sep=","
  done
  printf ']\n'
  exit 0
fi
echo "$PWD $*" >> "$log/calls"
bad() {
  echo "fake-claude: unexpected launch: $*"
  echo "$*" >> "$log/bad"
  exit 64
}
[ "$1" = --settings ] || bad "$@"
case "$2" in /*) ;; *) bad "$@" ;; esac
[ -f "$2" ] || bad "$@"
case "$#:$3" in
  2:) resume="" ;;
  4:--resume) resume="$4" ;;
  *) bad "$@" ;;
esac
[ -n "$BITPLANE_HOOK_TOKEN" ] && [ -n "$BITPLANE_HOOK_SOCKET" ] && [ -n "$BITPLANE_SESSION_ID" ] || bad "no hook env"
hook=$(tr ',{}[]' '\n\n\n\n\n' < "$2" | sed -n 's/.*"command": *"\([^"]*\)".*/\1/p' | grep ' hook$' | head -n 1)
[ -f "$log/no-hooks" ] && hook=""
sid="${resume:-$(cat "$log/next-sid")}"
entry="$log/registry/$$.json"
status=idle
reg() {
  mkdir -p "$log/registry"
  printf '{"pid":%s,"cwd":"%s","kind":"interactive","startedAt":1,"sessionId":"%s","status":"%s"}' $$ "$PWD" "$sid" "$status" > "$entry.tmp" && mv "$entry.tmp" "$entry"
}
emit() {
  [ -n "$hook" ] || return 0
  printf '{"hook_event_name":"%s","session_id":"%s","cwd":"%s"%s}' "$1" "$sid" "$PWD" "$2" | sh -c "$hook"
}
trap 'rm -f "$entry"' EXIT
reg
if [ -n "$resume" ]; then
  sleep 0.3
  emit SessionStart ',"source":"resume"'
else
  emit SessionStart ',"source":"startup"'
fi
while IFS= read -r line; do
  set -- $line
  case "$1" in
    prompt) status=busy; reg; emit UserPromptSubmit ',"prompt":"hi"' ;;
    stop) status=idle; reg; emit Stop '' ;;
    fail) status=idle; reg; emit StopFailure ',"error":"rate_limit","last_assistant_message":"Rate limit reached"' ;;
    quit) emit SessionEnd ',"reason":"prompt_input_exit"'; exit "${2:-0}" ;;
  esac
done
"#;

#[tokio::test(flavor = "multi_thread")]
async fn a_claude_terminal_follows_its_agent_and_resumes_after_it_exits() {
    let host = Host::new();
    let _daemon = host.serve().await;
    let cc = Cc::connect(&host).await;
    host.next_agent("agent-1");

    let tab = cc.open(Some(claude(None, None))).await;
    assert_eq!(tab.agent.as_deref(), Some("claude"));
    let agent = cc
        .wait_for_agent("agent-1", |a| a.state == AgentState::Idle)
        .await;
    assert_eq!(agent.kind, "claude");
    assert_eq!(agent.session.as_deref(), Some(tab.key.as_str()));
    assert!(!agent.polled);
    assert_eq!(agent.cwd, host.plane.display().to_string());

    cc.type_in(&tab, "prompt\r");
    cc.wait_for_agent("agent-1", |a| a.state == AgentState::Busy)
        .await;
    cc.type_in(&tab, "fail\r");
    let failed = cc
        .wait_for_agent("agent-1", |a| a.last_error.is_some())
        .await;
    let error = failed.last_error.unwrap();
    assert_eq!(error.error, "rate_limit");
    assert_eq!(error.message.as_deref(), Some("Rate limit reached"));
    cc.type_in(&tab, "prompt\r");
    cc.wait_for_agent("agent-1", |a| {
        a.state == AgentState::Busy && a.last_error.is_none()
    })
    .await;

    // A clean exit keeps the tab, unlike a shell's.
    cc.type_in(&tab, "quit\r");
    cc.wait_for_agent("agent-1", |a| a.state == AgentState::Exited)
        .await;
    let exited = cc
        .wait_for_session(&tab, |s| s.status == SessionStatus::Exited { code: 0 })
        .await;
    assert_eq!(exited.agent.as_deref(), Some("claude"));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(cc.sessions.list().len(), 1);
    assert!(cc.events().iter().any(|e| e.agent_session == "agent-1"));

    // Resume: `session.restart` relaunches the linked agent session.
    cc.sessions.restart(tab.id).await.unwrap();
    let resumed = cc
        .wait_for_agent("agent-1", |a| a.state == AgentState::Idle)
        .await;
    assert_eq!(resumed.session.as_deref(), Some(tab.key.as_str()));
    assert!(
        host.calls().ends_with(" --resume agent-1\n"),
        "{}",
        host.calls()
    );

    cc.type_in(&tab, "quit 3\r");
    cc.wait_for_session(&tab, |s| s.status == SessionStatus::Exited { code: 3 })
        .await;
    assert_eq!(host.bad(), "");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_whose_hooks_are_blocked_is_polled() {
    let host = Host::new();
    let _daemon = host.serve().await;
    let cc = Cc::connect(&host).await;
    host.next_agent("agent-2");
    std::fs::write(host.log.join("no-hooks"), "").unwrap();

    let tab = cc.open(Some(claude(None, None))).await;
    let agent = cc.wait_for_agent("agent-2", |a| a.polled).await;
    assert_eq!(agent.session.as_deref(), Some(tab.key.as_str()));

    let resumed = cc.open(Some(claude(Some("agent-5"), None))).await;
    let agent = cc.wait_for_agent("agent-5", |a| a.polled).await;
    assert_eq!(agent.session.as_deref(), Some(resumed.key.as_str()));
}

#[tokio::test(flavor = "multi_thread")]
async fn resuming_starts_in_the_agents_cwd_and_a_live_agent_is_refused() {
    let host = Host::new();
    let _daemon = host.serve().await;
    let cc = Cc::connect(&host).await;
    let api = host.plane.join("api");
    std::fs::create_dir_all(&api).unwrap();
    let cwd = api.display().to_string();

    let tab = cc.open(Some(claude(Some("agent-3"), Some(&cwd)))).await;
    assert_eq!(tab.agent.as_deref(), Some("claude"));
    let resuming = cc.wait_for_agent("agent-3", |_| true).await;
    assert_eq!(resuming.session.as_deref(), Some(tab.key.as_str()));
    let resumed = cc
        .wait_for_agent("agent-3", |a| a.state == AgentState::Idle)
        .await;
    assert_eq!(resumed.cwd, cwd);
    assert!(host.calls().starts_with(&format!("{cwd} --settings ")));

    let again = OpenOptions {
        name: None,
        agent: Some(claude(Some("agent-3"), Some(&cwd))),
    };
    assert_eq!(
        cc.sessions.open("demo", &host.plane, again).await,
        Err(SessionError::AgentLive)
    );
    assert_eq!(cc.sessions.list().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_claude_terminal_that_cannot_launch_keeps_its_tab_and_retries() {
    let host = Host::new();
    let _daemon = host.serve().await;
    let cc = Cc::connect(&host).await;
    host.next_agent("agent-6");
    let fake = host.bin.join("claude");
    let away = host.bin.join("claude-away");
    std::fs::rename(&fake, &away).unwrap();

    let tab = cc.open(Some(claude(None, None))).await;
    let failed = cc
        .wait_for_session(&tab, |s| {
            matches!(s.status, SessionStatus::SpawnFailed { .. })
        })
        .await;
    assert_eq!(failed.agent.as_deref(), Some("claude"));

    std::fs::rename(&away, &fake).unwrap();
    cc.sessions.restart(tab.id).await.unwrap();
    let agent = cc
        .wait_for_agent("agent-6", |a| a.state == AgentState::Idle)
        .await;
    assert_eq!(agent.session.as_deref(), Some(tab.key.as_str()));
    cc.wait_for_session(&tab, |s| s.status == SessionStatus::Running)
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_first_launch_imports_ccs_old_tabs_and_removes_its_hooks() {
    let host = Host::new();
    let _daemon = host.serve().await;
    let cc = Cc::connect(&host).await;
    let legacy = host.legacy();
    let gone = host.root.path().join("gone");
    let api = host.plane.join("api");
    std::fs::create_dir_all(&api).unwrap();
    let entry = |key: &str, dir: &Path, title: Option<&str>, seq: u32| json!({ "key": key, "planeId": "demo", "planeDir": dir, "title": title, "seq": seq });
    let sessions = json!({ "version": 1, "sessions": [
        entry("k2", &host.plane, Some("build"), 2),
        entry("k3", &gone, None, 3),
        entry("k1", &host.plane, None, 1),
    ] });
    std::fs::write(&legacy.sessions, sessions.to_string()).unwrap();
    std::fs::create_dir_all(&legacy.agents).unwrap();
    let link = json!({ "session_id": "agent-4", "cwd": api, "hook_event_name": "Stop" });
    std::fs::write(legacy.agents.join("k1.json"), link.to_string()).unwrap();
    std::fs::write(legacy.agents.join("k1.pid"), "1").unwrap();
    let settings = legacy.claude_settings.clone().unwrap();
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    let foreign = json!({ "hooks": [{ "type": "command", "command": "notify-me" }] });
    let ours =
        json!({ "hooks": [{ "type": "command", "command": "cat > /dev/null # BITPLANE_CC v2" }] });
    std::fs::write(
        &settings,
        json!({ "hooks": { "Stop": [foreign, ours], "SessionEnd": [ours] } }).to_string(),
    )
    .unwrap();

    migrate::import(&legacy, &cc.sessions).await;

    let mut tabs = cc.sessions.list();
    tabs.sort_by_key(|t| t.seq);
    assert_eq!(
        tabs.iter()
            .map(|t| (t.name.as_deref(), t.agent.as_deref()))
            .collect::<Vec<_>>(),
        [(None, Some("claude")), (Some("build"), None)]
    );
    let resumed = cc
        .wait_for_agent("agent-4", |a| a.state == AgentState::Idle)
        .await;
    assert_eq!(resumed.session.as_deref(), Some(tabs[0].key.as_str()));
    assert_eq!(resumed.cwd, api.display().to_string());
    assert!(
        host.calls()
            .starts_with(&format!("{} --settings ", api.display()))
    );
    assert!(!legacy.sessions.exists());
    assert!(!legacy.agents.exists());
    let after: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(after, json!({ "hooks": { "Stop": [foreign] } }));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_import_without_a_daemon_keeps_the_old_tabs() {
    let host = Host::new();
    let cc = Cc::disconnected(&host);
    let legacy = host.legacy();
    let sessions = json!({ "version": 1, "sessions": [
        { "key": "k1", "planeId": "demo", "planeDir": host.plane, "title": null, "seq": 1 },
    ] });
    std::fs::write(&legacy.sessions, sessions.to_string()).unwrap();

    migrate::import(&legacy, &cc.sessions).await;

    assert!(legacy.sessions.exists());
}

fn claude(resume: Option<&str>, cwd: Option<&str>) -> AgentOpen {
    AgentOpen {
        kind: "claude".into(),
        resume: resume.map(Into::into),
        cwd: cwd.map(Into::into),
    }
}

/// A host's directories in a short temp dir (macOS caps socket paths at 104
/// bytes), with a plane root, a `HOME` of its own so no profile runs and no
/// real `~/.claude` is read, and the fake `claude` the only one on the
/// daemon's `PATH`. The env is given, never captured from a login shell,
/// whose `path_helper` would put the host's real `claude` first.
struct Host {
    dirs: Dirs,
    plane: PathBuf,
    bin: PathBuf,
    log: PathBuf,
    home: PathBuf,
    env: EnvMap,
    root: tempfile::TempDir,
}

impl Host {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("cc")
            .tempdir_in("/tmp")
            .unwrap();
        let base = root.path().canonicalize().unwrap();
        let dirs = Dirs::for_version(&base.join("run"), &base.join("state"));
        let plane = base.join("demo");
        let home = base.join("home");
        let bin = base.join("bin");
        let log = base.join("log");
        for dir in [&plane, &home, &bin, &log] {
            std::fs::create_dir_all(dir).unwrap();
        }
        executable(&bin.join("claude"), FAKE_CLAUDE);
        // Still runs the `-l -c` script the daemon captures its env with.
        executable(
            &bin.join("sh-nologin"),
            "#!/bin/sh\nif [ \"$1 $3\" = \"-l -c\" ]; then eval \"$4\"; exit; fi\nexec /bin/sh\n",
        );
        let env = EnvMap::from([
            ("SHELL".into(), bin.join("sh-nologin").display().to_string()),
            ("HOME".into(), home.display().to_string()),
            ("PATH".into(), format!("{}:/usr/bin:/bin", bin.display())),
            ("PS1".into(), "$ ".into()),
            ("FAKE_CLAUDE_LOG".into(), log.display().to_string()),
        ]);
        Self {
            dirs,
            plane,
            bin,
            log,
            home,
            env,
            root,
        }
    }

    fn next_agent(&self, id: &str) {
        std::fs::write(self.log.join("next-sid"), id).unwrap();
    }

    /// Every launch of the fake `claude`: its cwd and argv.
    fn calls(&self) -> String {
        std::fs::read_to_string(self.log.join("calls")).unwrap_or_default()
    }

    /// The launches the fake `claude` refused.
    fn bad(&self) -> String {
        std::fs::read_to_string(self.log.join("bad")).unwrap_or_default()
    }

    fn legacy(&self) -> Legacy {
        let data = self.root.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        Legacy {
            sessions: data.join("sessions.json"),
            agents: data.join("agents"),
            claude_settings: Some(self.home.join(".claude").join("settings.json")),
        }
    }

    /// Stopped when dropped with the runtime, hanging up its sessions.
    async fn serve(&self) -> Daemon {
        let (stop, stopped) = oneshot::channel::<()>();
        let timing = Timing {
            poll: Duration::from_millis(100),
            live: Duration::from_millis(1000),
            grace: Duration::from_millis(500),
        };
        let config = Config {
            dirs: self.dirs.clone(),
            listener: None,
            // SAFETY: getuid has no preconditions.
            peer_uid: unsafe { libc::getuid() },
            modules: vec![
                bitplane_sessiond::session::Sessions::module(),
                AgentModule::claude_with(PathBuf::from(BUNDLED), timing),
            ],
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
            _stop: stop,
            _task: task,
        }
    }
}

struct Daemon {
    _stop: oneshot::Sender<()>,
    _task: JoinHandle<Result<(), ServeError>>,
}

/// cc's backend: its daemon link, session tabs and agents, with every
/// `agent_changed` it emits.
struct Cc {
    _link: Arc<DaemonLink>,
    plane: PathBuf,
    sessions: Arc<Sessions>,
    agents: Arc<Agents>,
    events: Mutex<mpsc::UnboundedReceiver<AgentEvent>>,
    seen: Mutex<Vec<AgentEvent>>,
}

impl Cc {
    async fn connect(host: &Host) -> Self {
        let (status_tx, mut statuses) = mpsc::unbounded_channel();
        let cc = Self::new(host, status_tx);
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match statuses.try_recv() {
                Ok(DaemonStatus::Connected { .. }) => break,
                Ok(_) => {}
                Err(_) => {
                    assert!(Instant::now() < deadline, "timed out connecting");
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }
        cc.sessions.ready(Duration::from_secs(10)).await;
        cc
    }

    /// Never connects: nothing serves the host's socket.
    fn disconnected(host: &Host) -> Self {
        let (status_tx, _) = mpsc::unbounded_channel();
        Self::new(host, status_tx)
    }

    fn new(host: &Host, statuses: mpsc::UnboundedSender<DaemonStatus>) -> Self {
        let link = DaemonLink::new(
            Ok(host.dirs.clone()),
            // Starting it does nothing, so only the in-process daemon answers.
            PathBuf::from("/usr/bin/true"),
            Arc::new(move |status| {
                let _ = statuses.send(status);
            }),
        );
        let sessions = Sessions::new(&link, SessionConfig::default(), Arc::new(|_| {}));
        let (event_tx, events) = mpsc::unbounded_channel();
        let agents = Agents::new(
            &link,
            Arc::new(move |event| {
                let _ = event_tx.send(event);
            }),
        );
        link.start();
        Self {
            _link: link,
            plane: host.plane.clone(),
            sessions,
            agents,
            events: Mutex::new(events),
            seen: Mutex::default(),
        }
    }

    async fn open(&self, agent: Option<AgentOpen>) -> SessionInfo {
        let options = OpenOptions { name: None, agent };
        let tab = self
            .sessions
            .open("demo", &self.plane, options)
            .await
            .unwrap();
        self.sessions
            .attach(tab.id, 100, 30, Arc::new(Discard))
            .unwrap();
        tab
    }

    fn type_in(&self, tab: &SessionInfo, text: &str) {
        self.sessions.write(tab.id, text.as_bytes()).unwrap();
    }

    fn events(&self) -> Vec<AgentEvent> {
        let mut seen = self.seen.lock().unwrap();
        while let Ok(event) = self.events.lock().unwrap().try_recv() {
            seen.push(event);
        }
        seen.clone()
    }

    async fn wait_for_session(
        &self,
        tab: &SessionInfo,
        matches: impl Fn(&SessionInfo) -> bool,
    ) -> SessionInfo {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let session = self.sessions.list().into_iter().find(|s| s.id == tab.id);
            if let Some(session) = session.filter(&matches) {
                return session;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for session {}: {:?}",
                tab.id,
                self.sessions.list()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn wait_for_agent(&self, id: &str, matches: impl Fn(&AgentInfo) -> bool) -> AgentInfo {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let agent = self
                .agents
                .list()
                .into_iter()
                .find(|a| a.agent_session == id);
            if let Some(agent) = agent.filter(&matches) {
                return agent;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for agent {id}: {:?}",
                self.agents.list()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

struct Discard;

impl OutputSink for Discard {
    fn send(&self, _bytes: Vec<u8>) -> bool {
        true
    }
}

fn executable(path: &Path, script: &str) {
    std::fs::write(path, script).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}
