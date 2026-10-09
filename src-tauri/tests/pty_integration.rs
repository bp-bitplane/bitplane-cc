//! Real PTYs against `/bin/sh`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bitplane_cc_lib::env::{self, EnvMap};
use bitplane_cc_lib::sessions::{
    OutputSink, Restore, SessionError, SessionEvent, SessionId, SessionStatus, Sessions,
    SpawnConfig, WINDOW,
};

#[test]
fn session_runs_in_the_plane_root_with_plane_variables() {
    let fixture = Fixture::new("/bin/sh");
    let id = fixture.open();
    fixture.type_line(id, "echo \"id=$BITPLANE_PLANE_ID term=$TERM_PROGRAM\"; pwd");
    let root = fixture.root.canonicalize().unwrap();
    fixture.wait_for_output("id=demo term=bitplane-cc");
    fixture.wait_for_output(root.to_str().unwrap());
}

#[test]
fn sessions_carry_a_key_and_the_agents_dir() {
    let fixture = Fixture::new("/bin/sh");
    let id = fixture.open();
    let key = fixture.sessions.list()[0].key.clone();
    assert_eq!(key.len(), 36);
    fixture.type_line(
        id,
        "echo \"key=$BITPLANE_SESSION_ID agents=$BITPLANE_CC_AGENTS cc=${CLAUDECODE-unset} cfg=$CLAUDE_CONFIG_DIR\"",
    );
    fixture.wait_for_output(&format!(
        "key={key} agents={} cc=unset cfg=/kept",
        fixture.agents.display()
    ));
}

#[test]
fn closing_deletes_the_agent_link() {
    let fixture = Fixture::new("/bin/sh");
    let id = fixture.open();
    let link = fixture
        .agents
        .join(format!("{}.json", fixture.sessions.list()[0].key));
    std::fs::write(&link, "{}").unwrap();
    fixture.type_line(id, "exit 0");
    fixture.wait_for_event(|e| e.id == id && e.session.is_none());
    assert!(!link.exists());
}

#[test]
fn restore_keeps_the_key_and_types_the_resume_line() {
    let fixture = Fixture::new("/bin/sh");
    let cwd = fixture.root.join("api");
    std::fs::create_dir(&cwd).unwrap();
    let info = fixture
        .sessions
        .restore(Restore {
            key: "k-restored".into(),
            plane_id: "demo".into(),
            plane_dir: fixture.root.clone(),
            title: Some("claude".into()),
            cwd: cwd.clone(),
            input: Some("echo resumed-$((6*7)) \"$PWD\"\n".into()),
            notice: Some("note-for-user".into()),
        })
        .unwrap();
    assert_eq!(info.key, "k-restored");
    assert_eq!(info.title.as_deref(), Some("claude"));
    fixture.attach(info.id);
    fixture.wait_for_output("note-for-user");
    fixture.wait_for_output(&format!(
        "resumed-42 {}",
        cwd.canonicalize().unwrap().display()
    ));
    assert_eq!(fixture.sessions.entries()[0].key, "k-restored");
}

#[test]
fn clean_exit_closes_the_session() {
    let fixture = Fixture::new("/bin/sh");
    let id = fixture.open();
    fixture.type_line(id, "exit 0");
    fixture.wait_for_event(|e| e.id == id && e.session.is_none());
    assert!(fixture.sessions.list().is_empty());
}

#[test]
fn failed_exit_keeps_an_exited_session_that_can_restart() {
    let fixture = Fixture::new("/bin/sh");
    let id = fixture.open();
    fixture.type_line(id, "exit 3");
    fixture.wait_for_event(|e| {
        e.session
            .as_ref()
            .is_some_and(|s| s.status == SessionStatus::Exited { code: 3 })
    });

    let restarted = fixture.sessions.restart(id).unwrap();
    assert_eq!(restarted.id, id);
    assert_eq!(restarted.status, SessionStatus::Running);
    fixture.type_line(id, "echo again-$((40+2))");
    fixture.wait_for_output("again-42");
}

#[test]
fn missing_shell_is_a_spawn_failure() {
    let fixture = Fixture::new("/nonexistent/shell");
    let info = fixture
        .sessions
        .open("demo", &fixture.root)
        .expect("a spawn failure still makes a session");
    assert!(matches!(info.status, SessionStatus::SpawnFailed { .. }));
}

#[test]
fn missing_root_spawns_nothing() {
    let fixture = Fixture::new("/bin/sh");
    let missing = fixture.root.join("gone");
    assert_eq!(
        fixture.sessions.open("demo", &missing).unwrap_err(),
        SessionError::RootMissing(missing)
    );
    assert!(fixture.sessions.list().is_empty());

    let id = fixture.open();
    std::fs::remove_dir_all(&fixture.root).unwrap();
    fixture.sessions.close(id);
    std::fs::create_dir(&fixture.root).unwrap();
    let id = fixture.open();
    fixture.type_line(id, "exit 1");
    fixture.wait_for_event(|e| {
        e.session
            .as_ref()
            .is_some_and(|s| matches!(s.status, SessionStatus::Exited { .. }))
    });
    std::fs::remove_dir_all(&fixture.root).unwrap();
    assert!(matches!(
        fixture.sessions.restart(id),
        Err(SessionError::RootMissing(_))
    ));
}

#[test]
fn busy_while_a_program_runs() {
    let fixture = Fixture::new("/bin/sh");
    let id = fixture.open();
    fixture.type_line(id, "echo ready");
    fixture.wait_for_output("ready");
    assert!(!fixture.sessions.busy(id).unwrap().busy);

    fixture.type_line(id, "sleep 30");
    let deadline = Instant::now() + Duration::from_secs(10);
    let busy = loop {
        let busy = fixture.sessions.busy(id).unwrap();
        if busy.busy || Instant::now() > deadline {
            break busy;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(busy.busy);
    assert_eq!(busy.process_name.as_deref(), Some("sleep"));
    let busy = fixture.sessions.busy_sessions();
    assert_eq!(
        (busy[0].plane_id.as_str(), busy[0].process_name.as_str()),
        ("demo", "sleep")
    );
    fixture.sessions.close(id);
}

#[test]
fn output_waits_for_acks_and_never_drops_bytes() {
    let fixture = Fixture::new("/bin/sh");
    fixture
        .sink
        .auto_ack
        .store(false, std::sync::atomic::Ordering::SeqCst);
    let id = fixture.open();
    let total = 600_000;
    fixture.type_line(
        id,
        &format!("head -c {total} /dev/zero | tr '\\0' x; echo; echo END-$((6*7))"),
    );

    std::thread::sleep(Duration::from_millis(500));
    let received = fixture.sink.len();
    assert!(
        received <= WINDOW as usize,
        "sent {received} bytes without acks"
    );

    fixture
        .sink
        .auto_ack
        .store(true, std::sync::atomic::Ordering::SeqCst);
    fixture.sessions.ack(id, received as u64);
    fixture.wait_for_output("END-42");
    let xs = fixture.sink.text().chars().filter(|&c| c == 'x').count();
    assert!(xs >= total, "lost bytes: {xs} of {total}");
}

#[test]
fn title_changes_are_emitted_once() {
    let fixture = Fixture::new("/bin/sh");
    let id = fixture.open();
    fixture.sessions.set_title(id, "vim".into());
    fixture.sessions.set_title(id, "vim".into());
    let titles = fixture
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|e| e.session.as_ref().and_then(|s| s.title.as_deref()) == Some("vim"))
        .count();
    assert_eq!(titles, 1);
}

struct Fixture {
    sessions: Arc<Sessions>,
    events: Arc<Mutex<Vec<SessionEvent>>>,
    sink: Arc<Sink>,
    root: PathBuf,
    agents: PathBuf,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new(shell: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("demo");
        std::fs::create_dir(&root).unwrap();
        let agents = dir.path().join("agents");
        std::fs::create_dir(&agents).unwrap();
        let mut env: EnvMap = env::original();
        env.insert("CLAUDECODE".into(), "1".into());
        env.insert("CLAUDE_CONFIG_DIR".into(), "/kept".into());
        // A quiet, predictable shell.
        env.insert("PS1".into(), "$ ".into());
        env.remove("ENV");
        let events: Arc<Mutex<Vec<SessionEvent>>> = Arc::default();
        let recorded = Arc::clone(&events);
        let sessions = Sessions::new(
            SpawnConfig {
                env,
                shell: shell.into(),
                app_version: "0.0.0-test".into(),
                agents_dir: Some(agents.clone()),
            },
            Arc::new(move |event| recorded.lock().unwrap().push(event)),
        );
        let sink = Arc::new(Sink::default());
        sink.auto_ack
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Fixture {
            sessions,
            events,
            sink,
            root,
            agents,
            _dir: dir,
        }
    }

    fn open(&self) -> SessionId {
        let info = self.sessions.open("demo", &self.root).unwrap();
        assert_eq!(info.status, SessionStatus::Running);
        self.attach(info.id);
        info.id
    }

    fn attach(&self, id: SessionId) {
        let sink = Arc::clone(&self.sink);
        sink.session
            .lock()
            .unwrap()
            .replace((id, Arc::downgrade(&self.sessions)));
        self.sessions.attach(id, sink).unwrap();
    }

    fn type_line(&self, id: SessionId, line: &str) {
        self.sessions
            .write(id, format!("{line}\n").as_bytes())
            .unwrap();
    }

    fn wait_for_output(&self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.sink.text().contains(needle) {
            assert!(
                Instant::now() < deadline,
                "never saw {needle:?} in {:?}",
                self.sink.text()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for_event(&self, matches: impl Fn(&SessionEvent) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.events.lock().unwrap().iter().any(&matches) {
            assert!(Instant::now() < deadline, "event never arrived");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Collects output and, like the frontend, acks what it consumed.
#[derive(Default)]
struct Sink {
    bytes: Mutex<Vec<u8>>,
    auto_ack: std::sync::atomic::AtomicBool,
    session: Mutex<Option<(SessionId, std::sync::Weak<Sessions>)>>,
}

impl Sink {
    fn len(&self) -> usize {
        self.bytes.lock().unwrap().len()
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes.lock().unwrap()).into_owned()
    }
}

impl OutputSink for Sink {
    fn send(&self, bytes: Vec<u8>) -> bool {
        let total = {
            let mut all = self.bytes.lock().unwrap();
            all.extend_from_slice(&bytes);
            all.len()
        };
        if self.auto_ack.load(std::sync::atomic::Ordering::SeqCst)
            && let Some((id, sessions)) = self.session.lock().unwrap().clone()
            && let Some(sessions) = sessions.upgrade()
        {
            // Acks arrive asynchronously in the app; never from the flusher.
            std::thread::spawn(move || sessions.ack(id, total as u64));
        }
        true
    }
}
