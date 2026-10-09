//! Sessions: the registry, PTY threads, flow control, busy check, restart and
//! restore (spec §4, §7.3, §7.4; ADRs 0003, 0004 and 0005).

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use portable_pty::{
    Child, ChildKiller, CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::agents;
use crate::env::EnvMap;
use crate::persist::Entry;
use crate::planes::Source;

pub type SessionId = u32;

/// A reader holds a partial frame at most this long before flushing it.
pub const COALESCE: Duration = Duration::from_millis(4);
/// Largest frame sent over the channel.
pub const MAX_FRAME: usize = 16 * 1024;
/// Unacked bytes allowed in flight towards the webview.
pub const WINDOW: u64 = 128 * 1024;
/// Once the window is full, sending resumes when unacked bytes drop to this.
pub const LOW_WATER: u64 = 32 * 1024;
/// Bytes read from the PTY but not yet sent, before the reader stops reading.
const MAX_PENDING: usize = 256 * 1024;

const DEFAULT_SIZE: PtySize = PtySize {
    rows: 24,
    cols: 80,
    pixel_width: 0,
    pixel_height: 0,
};

/// One session, as the frontend sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: SessionId,
    /// Stable across restarts of the app, unlike `id`.
    pub key: String,
    pub plane_id: String,
    /// Creation order; a plane's tabs are its sessions sorted by this.
    pub seq: u32,
    pub status: SessionStatus,
    /// Set by the program via OSC 0/2.
    pub title: Option<String>,
    /// The fallback title.
    pub shell_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionStatus {
    Running,
    /// The shell exited with a non-zero code.
    Exited {
        code: i32,
    },
    /// The shell couldn't be started.
    SpawnFailed {
        error: String,
    },
}

/// `session_changed`: one session, or `None` once it's closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SessionEvent {
    pub id: SessionId,
    pub session: Option<SessionInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BusyInfo {
    pub busy: bool,
    pub process_name: Option<String>,
}

/// Where a session's output goes. `send` returns false once the receiver is gone.
pub trait OutputSink: Send + Sync {
    fn send(&self, bytes: Vec<u8>) -> bool;
}

pub type EmitSession = Arc<dyn Fn(SessionEvent) + Send + Sync>;

/// How shells are started.
#[derive(Debug, Clone)]
pub struct SpawnConfig {
    /// The app's original environment (spec §6.1).
    pub env: EnvMap,
    pub shell: PathBuf,
    pub app_version: String,
    /// Where the agent hooks write agent links; `None` leaves them unset.
    pub agents_dir: Option<PathBuf>,
}

/// A session to bring back on launch (ADR 0005).
#[derive(Debug, Clone)]
pub struct Restore {
    pub key: String,
    pub plane_id: String,
    pub plane_dir: PathBuf,
    pub title: Option<String>,
    /// Where the shell starts; the plane root unless an agent session resumes.
    pub cwd: PathBuf,
    /// Typed into the shell once it's spawned.
    pub input: Option<String>,
    /// Shown above the shell's first prompt.
    pub notice: Option<String>,
}

/// A busy session, for the quit confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Busy {
    pub key: String,
    pub plane_id: String,
    pub plane_dir: PathBuf,
    pub process_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    NotFound(SessionId),
    /// The plane root is gone; nothing was spawned.
    RootMissing(PathBuf),
    Io(String),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::NotFound(id) => write!(f, "there is no session {id}"),
            SessionError::RootMissing(dir) => {
                write!(f, "the plane root {} no longer exists", dir.display())
            }
            SessionError::Io(message) => f.write_str(message),
        }
    }
}

pub struct Sessions {
    inner: Mutex<Inner>,
    config: SpawnConfig,
    emit: EmitSession,
}

#[derive(Default)]
struct Inner {
    next_id: SessionId,
    sessions: BTreeMap<SessionId, Session>,
}

struct Session {
    info: SessionInfo,
    plane_dir: PathBuf,
    flow: Arc<Flow>,
    pty: Option<Pty>,
}

struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    shell_pid: Option<u32>,
}

impl Sessions {
    pub fn new(config: SpawnConfig, emit: EmitSession) -> Arc<Self> {
        Arc::new(Sessions {
            inner: Mutex::new(Inner::default()),
            config,
            emit,
        })
    }

    /// Open a session in `plane_dir`. A spawn failure still makes a session,
    /// in the exited state; a missing root makes none.
    pub fn open(
        self: &Arc<Self>,
        plane_id: &str,
        plane_dir: &Path,
    ) -> Result<SessionInfo, SessionError> {
        self.restore(Restore {
            key: uuid::Uuid::new_v4().to_string(),
            plane_id: plane_id.into(),
            plane_dir: plane_dir.into(),
            title: None,
            cwd: plane_dir.into(),
            input: None,
            notice: None,
        })
    }

    /// Open a session under a key it had before. Restores happen in creation
    /// order, so tabs keep their order.
    pub fn restore(self: &Arc<Self>, restore: Restore) -> Result<SessionInfo, SessionError> {
        check_root(&restore.plane_dir)?;
        let mut inner = self.inner.lock().unwrap();
        inner.next_id += 1;
        let id = inner.next_id;
        let session = Session {
            info: SessionInfo {
                id,
                key: restore.key,
                plane_id: restore.plane_id,
                seq: id,
                status: SessionStatus::Running,
                title: None,
                shell_name: crate::env::shell_name(&self.config.shell),
            },
            plane_dir: restore.plane_dir,
            flow: Arc::new(Flow::default()),
            pty: None,
        };
        inner.sessions.insert(id, session);
        let mut info = self.start_pty(&mut inner, id, &restore.cwd);
        let session = inner.sessions.get_mut(&id).expect("just inserted");
        if let Some(notice) = restore.notice {
            session
                .flow
                .inject(format!("\x1b[2m{notice}\x1b[0m\r\n").as_bytes());
        }
        if let (Some(input), Some(pty)) = (restore.input, &session.pty) {
            let mut writer = pty.writer.lock().unwrap();
            let _ = writer
                .write_all(input.as_bytes())
                .and_then(|()| writer.flush());
        }
        session.info.title = restore.title.filter(|t| !t.trim().is_empty());
        info.title = session.info.title.clone();
        drop(inner);
        (self.emit)(SessionEvent {
            id,
            session: Some(info.clone()),
        });
        Ok(info)
    }

    /// A new PTY under the same session, keeping its tab and position.
    pub fn restart(self: &Arc<Self>, id: SessionId) -> Result<SessionInfo, SessionError> {
        let mut inner = self.inner.lock().unwrap();
        let session = inner
            .sessions
            .get_mut(&id)
            .ok_or(SessionError::NotFound(id))?;
        check_root(&session.plane_dir)?;
        if let Some(pty) = session.pty.take() {
            hang_up(pty);
        }
        let plane_dir = session.plane_dir.clone();
        let info = self.start_pty(&mut inner, id, &plane_dir);
        drop(inner);
        (self.emit)(SessionEvent {
            id,
            session: Some(info.clone()),
        });
        Ok(info)
    }

    /// Close a session: SIGHUP its shell and forget it, and its agent link.
    pub fn close(&self, id: SessionId) {
        let removed = self.inner.lock().unwrap().sessions.remove(&id);
        if let Some(session) = removed {
            if let Some(dir) = &self.config.agents_dir {
                agents::remove(dir, &session.info.key);
            }
            session.flow.close();
            if let Some(pty) = session.pty {
                hang_up(pty);
            }
            (self.emit)(SessionEvent { id, session: None });
        }
    }

    /// Bind a fresh output channel, resetting the ack window. Then nudge the
    /// size so full-screen programs redraw (spec §7.5).
    pub fn attach(&self, id: SessionId, sink: Arc<dyn OutputSink>) -> Result<(), SessionError> {
        let inner = self.inner.lock().unwrap();
        let session = inner.sessions.get(&id).ok_or(SessionError::NotFound(id))?;
        let reattached = session.flow.attach(sink);
        if let (true, Some(pty)) = (reattached, &session.pty) {
            nudge_size(pty.master.as_ref());
        }
        Ok(())
    }

    /// Blocks while the PTY's input buffer is full, so the registry lock is
    /// released first: acks must keep flowing while a large paste drains.
    pub fn write(&self, id: SessionId, bytes: &[u8]) -> Result<(), SessionError> {
        let writer = {
            let inner = self.inner.lock().unwrap();
            let session = inner.sessions.get(&id).ok_or(SessionError::NotFound(id))?;
            match &session.pty {
                Some(pty) => Arc::clone(&pty.writer),
                None => return Ok(()),
            }
        };
        let mut writer = writer.lock().unwrap();
        writer
            .write_all(bytes)
            .and_then(|()| writer.flush())
            .map_err(|e| SessionError::Io(e.to_string()))
    }

    pub fn resize(&self, id: SessionId, cols: u16, rows: u16) -> Result<(), SessionError> {
        let inner = self.inner.lock().unwrap();
        let session = inner.sessions.get(&id).ok_or(SessionError::NotFound(id))?;
        if let Some(pty) = &session.pty {
            pty.master
                .resize(PtySize {
                    rows: rows.max(1),
                    cols: cols.max(1),
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| SessionError::Io(e.to_string()))?;
        }
        Ok(())
    }

    /// Cumulative count of bytes the frontend has consumed.
    pub fn ack(&self, id: SessionId, bytes: u64) {
        let inner = self.inner.lock().unwrap();
        if let Some(session) = inner.sessions.get(&id) {
            session.flow.ack(bytes);
        }
    }

    pub fn set_title(&self, id: SessionId, title: String) {
        let info = {
            let mut inner = self.inner.lock().unwrap();
            let Some(session) = inner.sessions.get_mut(&id) else {
                return;
            };
            let title = Some(title).filter(|t| !t.trim().is_empty());
            if session.info.title == title {
                return;
            }
            session.info.title = title;
            session.info.clone()
        };
        (self.emit)(SessionEvent {
            id,
            session: Some(info),
        });
    }

    /// A session is busy when its terminal's foreground process group isn't
    /// the shell's. Checked on demand only.
    pub fn busy(&self, id: SessionId) -> Result<BusyInfo, SessionError> {
        let inner = self.inner.lock().unwrap();
        let session = inner.sessions.get(&id).ok_or(SessionError::NotFound(id))?;
        Ok(session.pty.as_ref().map_or(
            BusyInfo {
                busy: false,
                process_name: None,
            },
            busy_info,
        ))
    }

    pub fn list(&self) -> Vec<SessionInfo> {
        let inner = self.inner.lock().unwrap();
        inner.sessions.values().map(|s| s.info.clone()).collect()
    }

    /// The planes that have sessions, for merging gone planes into the sidebar.
    pub fn plane_sources(&self) -> Vec<Source> {
        let inner = self.inner.lock().unwrap();
        let mut sources: BTreeMap<&str, Source> = BTreeMap::new();
        for session in inner.sessions.values() {
            sources
                .entry(&session.info.plane_id)
                .or_insert_with(|| Source::new(&session.info.plane_id, &session.plane_dir));
        }
        sources.into_values().collect()
    }

    /// Every busy session's plane and foreground process.
    pub fn busy_sessions(&self) -> Vec<Busy> {
        let inner = self.inner.lock().unwrap();
        inner
            .sessions
            .values()
            .filter_map(|session| {
                let info = busy_info(session.pty.as_ref()?);
                info.busy.then(|| Busy {
                    key: session.info.key.clone(),
                    plane_id: session.info.plane_id.clone(),
                    plane_dir: session.plane_dir.clone(),
                    process_name: info.process_name.unwrap_or_else(|| "a process".into()),
                })
            })
            .collect()
    }

    /// What `sessions.json` holds, in creation order.
    pub fn entries(&self) -> Vec<Entry> {
        let inner = self.inner.lock().unwrap();
        inner
            .sessions
            .values()
            .map(|session| Entry {
                key: session.info.key.clone(),
                plane_id: session.info.plane_id.clone(),
                plane_dir: session.plane_dir.clone(),
                title: session.info.title.clone(),
                seq: session.info.seq,
            })
            .collect()
    }

    /// SIGHUP every shell (quitting).
    pub fn hang_up_all(&self) {
        let mut inner = self.inner.lock().unwrap();
        for session in inner.sessions.values_mut() {
            session.flow.close();
            if let Some(mut pty) = session.pty.take() {
                let _ = pty.killer.kill();
            }
        }
    }

    /// Spawn a PTY for session `id`, updating its status.
    fn start_pty(self: &Arc<Self>, inner: &mut Inner, id: SessionId, cwd: &Path) -> SessionInfo {
        let session = inner
            .sessions
            .get_mut(&id)
            .expect("caller checked the session");
        let generation = session.flow.next_generation();
        match spawn(&self.config, &session.info, &session.plane_dir, cwd) {
            Ok((pty, child, reader)) => {
                spawn_reader(reader, Arc::clone(&session.flow), generation);
                self.spawn_waiter(id, child, Arc::clone(&session.flow), generation);
                session.pty = Some(pty);
                session.info.status = SessionStatus::Running;
            }
            Err(error) => {
                session.info.status = SessionStatus::SpawnFailed { error };
            }
        }
        session.info.title = None;
        session.info.clone()
    }

    /// Exit code 0 closes the session; anything else leaves it exited.
    fn spawn_waiter(
        self: &Arc<Self>,
        id: SessionId,
        mut child: Box<dyn Child + Send + Sync>,
        flow: Arc<Flow>,
        generation: u64,
    ) {
        let sessions = Arc::downgrade(self);
        std::thread::Builder::new()
            .name(format!("pty-wait-{id}"))
            .spawn(move || {
                let code = child
                    .wait()
                    .map(|status| status.exit_code() as i32)
                    .unwrap_or(-1);
                let Some(sessions) = sessions.upgrade() else {
                    return;
                };
                if !flow.is_generation(generation) {
                    return;
                }
                if code == 0 {
                    sessions.close(id);
                    return;
                }
                let info = {
                    let mut inner = sessions.inner.lock().unwrap();
                    let Some(session) = inner.sessions.get_mut(&id) else {
                        return;
                    };
                    session.pty = None;
                    session.info.status = SessionStatus::Exited { code };
                    session.info.clone()
                };
                (sessions.emit)(SessionEvent {
                    id,
                    session: Some(info),
                });
            })
            .expect("spawning a thread");
    }
}

type Spawned = (Pty, Box<dyn Child + Send + Sync>, Box<dyn Read + Send>);

fn spawn(
    config: &SpawnConfig,
    info: &SessionInfo,
    plane_dir: &Path,
    cwd: &Path,
) -> Result<Spawned, String> {
    let pair = NativePtySystem::default()
        .openpty(DEFAULT_SIZE)
        .map_err(|e| format!("couldn't open a terminal: {e}"))?;

    let mut command = CommandBuilder::new(&config.shell);
    command.arg("-l");
    command.cwd(cwd);
    command.env_clear();
    for (key, value) in &config.env {
        command.env(key, value);
    }
    command.env("BITPLANE_PLANE_ID", &info.plane_id);
    command.env("BITPLANE_PLANE_DIR", plane_dir);
    command.env("BITPLANE_SESSION_ID", &info.key);
    match &config.agents_dir {
        Some(dir) => command.env("BITPLANE_CC_AGENTS", dir),
        None => command.env_remove("BITPLANE_CC_AGENTS"),
    }
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env("TERM_PROGRAM", "bitplane-cc");
    command.env("TERM_PROGRAM_VERSION", &config.app_version);

    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|e| format!("couldn't start {}: {e}", config.shell.display()))?;
    drop(pair.slave);
    let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let pty = Pty {
        master: pair.master,
        writer: Arc::new(Mutex::new(writer)),
        killer: child.clone_killer(),
        shell_pid: child.process_id(),
    };
    Ok((pty, child, reader))
}

/// Never let the PTY fall back to `$HOME` (spec §4.1).
fn check_root(plane_dir: &Path) -> Result<(), SessionError> {
    if plane_dir.is_dir() {
        Ok(())
    } else {
        Err(SessionError::RootMissing(plane_dir.into()))
    }
}

/// SIGHUP the shell and drop the master off the caller's thread.
fn hang_up(mut pty: Pty) {
    let _ = pty.killer.kill();
    std::thread::spawn(move || drop(pty));
}

/// Resize away and back, so the program gets SIGWINCH and redraws even though
/// the size ends up unchanged.
fn nudge_size(master: &dyn MasterPty) {
    if let Ok(size) = master.get_size() {
        let taller = PtySize {
            rows: size.rows.saturating_add(1),
            ..size
        };
        let _ = master.resize(taller);
        let _ = master.resize(size);
    }
}

fn busy_info(pty: &Pty) -> BusyInfo {
    let foreground = pty.master.process_group_leader();
    match (foreground, pty.shell_pid) {
        (Some(fg), Some(shell)) if fg as u32 != shell => BusyInfo {
            busy: true,
            process_name: process_name(fg),
        },
        _ => BusyInfo {
            busy: false,
            process_name: None,
        },
    }
}

#[cfg(target_os = "macos")]
fn process_name(pid: libc::pid_t) -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: proc_name writes at most `buf.len()` bytes into `buf`.
    let len = unsafe { libc::proc_name(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    (len > 0).then(|| String::from_utf8_lossy(&buf[..len as usize]).into_owned())
}

#[cfg(not(target_os = "macos"))]
fn process_name(pid: libc::pid_t) -> Option<String> {
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    Some(comm.trim_end().to_string()).filter(|name| !name.is_empty())
}

/// Output flow control shared by a session's reader, flusher and commands.
///
/// `generation` changes on every restart, so threads of an old PTY notice
/// they're stale and stop.
#[derive(Default)]
struct Flow {
    state: Mutex<FlowState>,
    changed: Condvar,
}

#[derive(Default)]
struct FlowState {
    generation: u64,
    sink: Option<Arc<dyn OutputSink>>,
    attached_before: bool,
    sent: u64,
    acked: u64,
    pending: Vec<u8>,
    eof: bool,
    closed: bool,
}

impl Flow {
    fn next_generation(&self) -> u64 {
        let mut state = self.state.lock().unwrap();
        state.generation += 1;
        state.pending.clear();
        state.eof = false;
        self.changed.notify_all();
        state.generation
    }

    fn is_generation(&self, generation: u64) -> bool {
        let state = self.state.lock().unwrap();
        state.generation == generation && !state.closed
    }

    /// True when a channel was bound before (a reattach after reload).
    fn attach(&self, sink: Arc<dyn OutputSink>) -> bool {
        let mut state = self.state.lock().unwrap();
        let reattached = std::mem::replace(&mut state.attached_before, true);
        state.sink = Some(sink);
        state.sent = 0;
        state.acked = 0;
        self.changed.notify_all();
        reattached
    }

    fn ack(&self, bytes: u64) {
        let mut state = self.state.lock().unwrap();
        state.acked = state.acked.max(bytes.min(state.sent));
        self.changed.notify_all();
    }

    fn close(&self) {
        self.state.lock().unwrap().closed = true;
        self.changed.notify_all();
    }

    /// Output the program didn't write, queued ahead of what it will.
    fn inject(&self, bytes: &[u8]) {
        self.state.lock().unwrap().pending.extend_from_slice(bytes);
        self.changed.notify_all();
    }
}

/// Two threads per PTY: a blocking reader, and a flusher that coalesces and
/// sends frames. The flusher waits on a timeout so a short burst is never
/// held waiting for the next read.
fn spawn_reader(mut reader: Box<dyn Read + Send>, flow: Arc<Flow>, generation: u64) {
    let read_flow = Arc::clone(&flow);
    std::thread::Builder::new()
        .name("pty-read".into())
        .spawn(move || {
            let mut buf = vec![0u8; MAX_FRAME];
            loop {
                let n = match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                let mut state = read_flow.state.lock().unwrap();
                while state.pending.len() >= MAX_PENDING
                    && state.generation == generation
                    && !state.closed
                {
                    state = read_flow.changed.wait(state).unwrap();
                }
                if state.generation != generation || state.closed {
                    return;
                }
                state.pending.extend_from_slice(&buf[..n]);
                read_flow.changed.notify_all();
            }
            let mut state = read_flow.state.lock().unwrap();
            if state.generation == generation {
                state.eof = true;
                read_flow.changed.notify_all();
            }
        })
        .expect("spawning a thread");

    std::thread::Builder::new()
        .name("pty-flush".into())
        .spawn(move || flush_loop(&flow, generation))
        .expect("spawning a thread");
}

fn flush_loop(flow: &Flow, generation: u64) {
    let stale = |state: &FlowState| state.generation != generation || state.closed;
    loop {
        let mut state = flow.state.lock().unwrap();
        while state.pending.is_empty() && !state.eof && !stale(&state) {
            state = flow.changed.wait(state).unwrap();
        }
        if stale(&state) || (state.pending.is_empty() && state.eof) {
            return;
        }
        // Hold a partial frame for one more tick so a burst spread over
        // several reads leaves as one message.
        if state.pending.len() < MAX_FRAME && !state.eof {
            state = flow.changed.wait_timeout(state, COALESCE).unwrap().0;
        }
        // Wait for a channel and for room in the window; never drop bytes.
        if state.sink.is_none() || state.sent - state.acked >= WINDOW {
            while !stale(&state) && (state.sink.is_none() || state.sent - state.acked > LOW_WATER) {
                state = flow.changed.wait(state).unwrap();
            }
        }
        if stale(&state) {
            return;
        }
        let credit = (WINDOW - (state.sent - state.acked)) as usize;
        let take = state.pending.len().min(credit).min(MAX_FRAME);
        let frame: Vec<u8> = state.pending.drain(..take).collect();
        state.sent += take as u64;
        let sink = state.sink.clone().expect("waited for a sink");
        flow.changed.notify_all();
        drop(state);

        if !sink.send(frame) {
            // The webview went away (reload): detach until it attaches again.
            let mut state = flow.state.lock().unwrap();
            if state.sink.as_ref().is_some_and(|s| Arc::ptr_eq(s, &sink)) {
                state.sink = None;
            }
        }
    }
}
