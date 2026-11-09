//! cc's agent state, resume and the first-launch import against a real
//! bitplane-sessiond with its `agent.claude` module, served in-process, and a
//! fake `claude` on `PATH`. The module's hooks run the bundled daemon binary.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bitplane_cc_lib::agents::{AgentEvent, AgentInfo, AgentState, Agents};
use bitplane_cc_lib::daemon::{DaemonLink, DaemonStatus};
use bitplane_cc_lib::migrate::{self, Legacy};
use bitplane_cc_lib::sessions::{
    OpenOptions, OutputSink, SessionConfig, SessionError, SessionId, SessionInfo, Sessions,
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

/// Started through the daemon's wrapper it gets `--settings` with the hook
/// command, starts agent session `$FAKE_CLAUDE_LOG/next-sid` (or the
/// `--resume` one, a moment later, as the real one does) and then turns each
/// typed `prompt`, `stop`, `fail` or `quit` line into that hook. It keeps its
/// `claude agents --json` registry entry in step, in the verified format.
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
settings=""
resume=""
while [ $# -gt 0 ]; do
  case "$1" in
    --settings) settings="$2"; shift ;;
    --resume) resume="$2"; shift ;;
  esac
  shift
done
case "$settings" in
  "") json="" ;;
  "{"*) json="$settings" ;;
  *) json=$(cat "$settings") ;;
esac
hook=$(printf '%s' "$json" | tr ',{}[]' '\n\n\n\n\n' | sed -n 's/.*"command": *"\([^"]*\)".*/\1/p' | grep ' hook$' | head -n 1)
export CLAUDECODE=1
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
  case "$line" in
    prompt*) status=busy; reg; emit UserPromptSubmit ',"prompt":"hi"' ;;
    stop*) status=idle; reg; emit Stop '' ;;
    fail*) status=idle; reg; emit StopFailure ',"error":"rate_limit","last_assistant_message":"Rate limit reached"' ;;
    quit*) emit SessionEnd ',"reason":"prompt_input_exit"'; exit 0 ;;
  esac
done
"#;

#[tokio::test(flavor = "multi_thread")]
async fn a_tab_follows_its_agents_state_and_last_error() {
    let host = Host::new();
    let _daemon = host.serve().await;
    let cc = Cc::connect(&host).await;
    let tab = cc.open(OpenOptions::default()).await;
    host.next_agent("agent-1");

    cc.type_in(tab.id, "claude\r");
    let agent = cc
        .wait_for_agent("agent-1", |a| a.state == AgentState::Idle)
        .await;
    assert_eq!(agent.session.as_deref(), Some(tab.key.as_str()));
    assert!(!agent.polled);
    assert_eq!(agent.cwd, host.plane.display().to_string());

    cc.type_in(tab.id, "prompt\r");
    cc.wait_for_agent("agent-1", |a| a.state == AgentState::Busy)
        .await;
    cc.type_in(tab.id, "fail\r");
    let failed = cc
        .wait_for_agent("agent-1", |a| a.last_error.is_some())
        .await;
    let error = failed.last_error.unwrap();
    assert_eq!(error.error, "rate_limit");
    assert_eq!(error.message.as_deref(), Some("Rate limit reached"));

    cc.type_in(tab.id, "prompt\r");
    cc.wait_for_agent("agent-1", |a| {
        a.state == AgentState::Busy && a.last_error.is_none()
    })
    .await;
    cc.type_in(tab.id, "quit\r");
    cc.wait_for_agent("agent-1", |a| a.state == AgentState::Exited)
        .await;
    assert!(cc.events().iter().any(|e| e.agent_session == "agent-1"));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_without_hooks_is_polled() {
    let host = Host::new();
    let _daemon = host.serve().await;
    let cc = Cc::connect(&host).await;
    let tab = cc.open(OpenOptions::default()).await;
    host.next_agent("agent-2");

    // An absolute path bypasses the wrapper, as a shell rebuilding PATH does.
    let bypass = format!("{}/claude\r", host.bin.display());
    cc.type_in(tab.id, &bypass);

    let agent = cc.wait_for_agent("agent-2", |a| a.polled).await;
    assert_eq!(agent.session.as_deref(), Some(tab.key.as_str()));
}

#[tokio::test(flavor = "multi_thread")]
async fn resuming_opens_a_session_and_a_live_agent_is_refused() {
    let host = Host::new();
    let _daemon = host.serve().await;
    let cc = Cc::connect(&host).await;

    let tab = cc
        .open(OpenOptions {
            name: None,
            resume: Some("agent-3".into()),
        })
        .await;

    let resuming = cc.wait_for_agent("agent-3", |_| true).await;
    assert_eq!(resuming.session.as_deref(), Some(tab.key.as_str()));
    let resumed = cc
        .wait_for_agent("agent-3", |a| a.state == AgentState::Idle)
        .await;
    assert_eq!(resumed.session.as_deref(), Some(tab.key.as_str()));
    let again = OpenOptions {
        name: None,
        resume: Some("agent-3".into()),
    };
    assert_eq!(
        cc.sessions.open("demo", &host.plane, again).await,
        Err(SessionError::AgentLive)
    );
    assert_eq!(cc.sessions.list().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_first_launch_imports_ccs_old_tabs_and_removes_its_hooks() {
    let host = Host::new();
    let _daemon = host.serve().await;
    let cc = Cc::connect(&host).await;
    let legacy = host.legacy();
    let gone = host.root.path().join("gone");
    let entry = |key: &str, dir: &Path, title: Option<&str>, seq: u32| json!({ "key": key, "planeId": "demo", "planeDir": dir, "title": title, "seq": seq });
    let sessions = json!({ "version": 1, "sessions": [
        entry("k2", &host.plane, Some("build"), 2),
        entry("k3", &gone, None, 3),
        entry("k1", &host.plane, None, 1),
    ] });
    std::fs::write(&legacy.sessions, sessions.to_string()).unwrap();
    std::fs::create_dir_all(&legacy.agents).unwrap();
    let link = json!({ "session_id": "agent-4", "cwd": host.plane, "hook_event_name": "Stop" });
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
        tabs.iter().map(|t| t.name.as_deref()).collect::<Vec<_>>(),
        [None, Some("build")]
    );
    let resumed = cc
        .wait_for_agent("agent-4", |a| a.state == AgentState::Idle)
        .await;
    assert_eq!(resumed.session.as_deref(), Some(tabs[0].key.as_str()));
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

/// A host's directories in a short temp dir (macOS caps socket paths at 104
/// bytes), with a plane root, a `HOME` of its own so no profile runs, and the
/// fake `claude` on `PATH`. Its shell is never a login shell: macOS's
/// `path_helper` would put the host's real `claude` first.
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

    async fn open(&self, options: OpenOptions) -> SessionInfo {
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

    fn type_in(&self, id: SessionId, text: &str) {
        self.sessions.write(id, text.as_bytes()).unwrap();
    }

    fn events(&self) -> Vec<AgentEvent> {
        let mut seen = self.seen.lock().unwrap();
        while let Ok(event) = self.events.lock().unwrap().try_recv() {
            seen.push(event);
        }
        seen.clone()
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
