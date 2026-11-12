//! Session tabs (ADR 0028): one per daemon session, mirrored from
//! `session.list` and the session events, and the relay between each tab's
//! webview channel and its daemon attachment (ADR 0003, amended).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use bitplane_protocol::session::{
    AttachmentAckParams, AttachmentError, AttachmentId, AttachmentOpenParams, AttachmentOpenResult,
    AttachmentRef, AttachmentResizeParams, Session, SessionBusyResult, SessionListResult,
    SessionOpenAgent, SessionOpenParams, SessionRef, SessionRenameParams, SessionResult,
};
use bitplane_protocol::{ErrorCode, Frame, Notification, method};
use bitplane_sessiond::client::{ClientError, Handle};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use tokio::sync::watch;

use crate::daemon::{DaemonLink, Subscriber};
use crate::planes::Source;

/// A tab's handle, per launch; `SessionInfo::key` is the daemon's id.
pub type SessionId = u32;

/// The first byte of every channel message: the terminal starts over from a
/// snapshot, or continues with output.
pub const FRAME_SNAPSHOT: u8 = 0;
pub const FRAME_OUTPUT: u8 = 1;

/// One session tab, as the frontend sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: SessionId,
    /// The daemon's session id, also the shell's `BITPLANE_SESSION_ID`;
    /// stable across launches of cc and restarts of the daemon.
    pub key: String,
    pub plane_id: String,
    /// Creation order on this host; a plane's tabs are sorted by it.
    pub seq: u32,
    pub status: SessionStatus,
    /// Set by the program via OSC 0/2.
    pub title: Option<String>,
    /// The user's name for the tab, set by `session.rename`.
    pub name: Option<String>,
    /// The fallback title.
    pub shell_name: String,
    /// The session root no longer exists; restarting it fails.
    pub root_gone: bool,
    /// The agent kind of a Claude terminal, whose process is the agent
    /// itself; `None` for a regular terminal.
    pub agent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionStatus {
    Running,
    /// The process exited; a regular terminal's shell exiting 0 closes its
    /// tab instead.
    Exited {
        code: i32,
    },
    /// The process couldn't be started.
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

/// Where a tab's output goes. `send` returns false once the receiver is gone.
pub trait OutputSink: Send + Sync {
    fn send(&self, bytes: Vec<u8>) -> bool;
}

pub type EmitSession = Arc<dyn Fn(SessionEvent) + Send + Sync>;

#[derive(Debug, Clone, Default)]
pub struct SessionConfig {
    pub shell_name: String,
}

/// What a new session starts with besides its plane.
#[derive(Debug, Clone, Default)]
pub struct OpenOptions {
    /// The tab's name, as `session.rename` would set it.
    pub name: Option<String>,
    /// Makes it a Claude terminal.
    pub agent: Option<AgentOpen>,
}

/// The agent a Claude terminal launches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentOpen {
    pub kind: String,
    /// The agent session to continue.
    pub resume: Option<String>,
    /// Where the agent starts; a resume needs the cwd it ran in, or it
    /// starts in the plane root.
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    NotFound(SessionId),
    /// The plane root is gone; nothing was started.
    RootMissing(PathBuf),
    /// The agent session to resume already runs in another session.
    AgentLive,
    Daemon(String),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::NotFound(id) => write!(f, "there is no session {id}"),
            SessionError::RootMissing(dir) => {
                write!(f, "the plane root {} no longer exists", dir.display())
            }
            SessionError::AgentLive => {
                f.write_str("that agent session is already running in another session")
            }
            SessionError::Daemon(message) => f.write_str(message),
        }
    }
}

pub struct Sessions {
    me: Weak<Sessions>,
    config: SessionConfig,
    emit: EmitSession,
    state: Mutex<State>,
    /// Held while frames are forwarded, so they reach a channel in order
    /// without holding `state` while a sink runs.
    forwarding: Mutex<()>,
    ready: watch::Sender<bool>,
}

#[derive(Default)]
struct State {
    next_id: SessionId,
    ids: HashMap<String, SessionId>,
    tabs: BTreeMap<SessionId, Tab>,
    handle: Option<Handle>,
    /// Counts connections; replies from an older one are dropped.
    connection: u64,
    next_binding: u64,
    /// The current connection's attachments.
    attachments: HashMap<AttachmentId, SessionId>,
    /// Frames that beat their `session.attachment.open` reply.
    early: HashMap<AttachmentId, Vec<Frame>>,
    /// While a `session.list` is in flight: what the events said meanwhile,
    /// which is newer than the list.
    listing: Option<Touched>,
}

#[derive(Default)]
struct Touched {
    changed: HashSet<String>,
    removed: HashSet<String>,
}

struct Tab {
    info: SessionInfo,
    root: PathBuf,
    binding: Option<Binding>,
}

/// A tab's webview channel and the attachment feeding it.
struct Binding {
    sink: Arc<dyn OutputSink>,
    cols: u16,
    rows: u16,
    id: u64,
    attachment: Option<AttachmentId>,
    /// Data bytes sent on the channel; the webview's acks count these.
    forwarded: u64,
    /// `forwarded` when the current attachment's first frame went out.
    base: u64,
    /// Typed while the attachment was still opening.
    input: Vec<u8>,
}

impl Sessions {
    /// Follows `link` from its next connection on.
    pub fn new(link: &DaemonLink, config: SessionConfig, emit: EmitSession) -> Arc<Self> {
        let sessions = Arc::new_cyclic(|me| Sessions {
            me: me.clone(),
            config,
            emit,
            state: Mutex::default(),
            forwarding: Mutex::new(()),
            ready: watch::Sender::new(false),
        });
        link.subscribe(Arc::new(Subscription(Arc::downgrade(&sessions))));
        sessions
    }

    /// Resolves once the first `session.list` is mirrored, or after `timeout`.
    pub async fn ready(&self, timeout: Duration) {
        let mut ready = self.ready.subscribe();
        let _ = tokio::time::timeout(timeout, ready.wait_for(|ready| *ready)).await;
    }

    pub fn list(&self) -> Vec<SessionInfo> {
        let state = self.state.lock().unwrap();
        state.tabs.values().map(|tab| tab.info.clone()).collect()
    }

    /// The planes that have sessions, for merging gone planes into the sidebar.
    pub fn plane_sources(&self) -> Vec<Source> {
        let state = self.state.lock().unwrap();
        let mut sources: BTreeMap<&str, Source> = BTreeMap::new();
        for tab in state.tabs.values() {
            sources
                .entry(&tab.info.plane_id)
                .or_insert_with(|| Source::new(&tab.info.plane_id, &tab.root));
        }
        sources.into_values().collect()
    }

    /// Open a session in `plane_dir`; a missing root opens none.
    pub async fn open(
        &self,
        plane_id: &str,
        plane_dir: &Path,
        options: OpenOptions,
    ) -> Result<SessionInfo, SessionError> {
        if !plane_dir.is_dir() {
            return Err(SessionError::RootMissing(plane_dir.into()));
        }
        let params = SessionOpenParams {
            root: plane_dir.display().to_string(),
            env: None,
            plane: Some(plane_id.into()),
            name: options.name.filter(|name| !name.trim().is_empty()),
            agent: options.agent.map(|agent| SessionOpenAgent {
                kind: agent.kind,
                resume: agent.resume,
                cwd: agent.cwd,
            }),
        };
        match self.call(method::SESSION_OPEN, params).await {
            Ok(SessionResult { session }) => Ok(self.mirror(session)),
            Err(Failure::Rpc(ErrorCode::AgentLive, _)) => Err(SessionError::AgentLive),
            Err(Failure::Rpc(ErrorCode::NotFound, _)) => {
                Err(SessionError::RootMissing(plane_dir.into()))
            }
            Err(failure) => Err(failure.into()),
        }
    }

    /// A fresh process under the same tab; its terminal starts over. A
    /// Claude terminal resumes its agent session when it has one.
    pub async fn restart(&self, id: SessionId) -> Result<SessionInfo, SessionError> {
        let (key, root) = self.key_and_root(id)?;
        let result = self
            .call::<SessionResult>(method::SESSION_RESTART, SessionRef { session: key })
            .await;
        match result {
            Ok(SessionResult { session }) => Ok(self.mirror(session)),
            Err(Failure::Rpc(ErrorCode::RootGone, _)) => Err(SessionError::RootMissing(root)),
            Err(failure) => Err(failure.into()),
        }
    }

    /// Close the session for every client.
    pub async fn close(&self, id: SessionId) -> Result<(), SessionError> {
        let (key, _) = self.key_and_root(id)?;
        self.close_key(&key).await
    }

    pub async fn busy(&self, id: SessionId) -> Result<BusyInfo, SessionError> {
        let (key, _) = self.key_and_root(id)?;
        let busy: SessionBusyResult = self
            .call(method::SESSION_BUSY, SessionRef { session: key })
            .await?;
        Ok(BusyInfo {
            busy: busy.busy,
            process_name: busy.process_name,
        })
    }

    /// Name the tab; `None` or a blank name clears it.
    pub async fn rename(
        &self,
        id: SessionId,
        name: Option<String>,
    ) -> Result<SessionInfo, SessionError> {
        let (key, _) = self.key_and_root(id)?;
        let params = SessionRenameParams {
            session: key,
            name: name.filter(|name| !name.trim().is_empty()),
        };
        let SessionResult { session } = self.call(method::SESSION_RENAME, params).await?;
        Ok(self.mirror(session))
    }

    /// Bind a fresh channel: the tab's previous attachment closes and a new
    /// one opens, which starts the channel with a snapshot.
    pub fn attach(
        &self,
        id: SessionId,
        cols: u16,
        rows: u16,
        sink: Arc<dyn OutputSink>,
    ) -> Result<(), SessionError> {
        let mut state = self.state.lock().unwrap();
        state.next_binding += 1;
        let binding = state.next_binding;
        let tab = state.tabs.get_mut(&id).ok_or(SessionError::NotFound(id))?;
        let previous = tab.binding.take().and_then(|b| b.attachment);
        tab.binding = Some(Binding {
            sink,
            cols,
            rows,
            id: binding,
            attachment: None,
            forwarded: 0,
            base: 0,
            input: Vec::new(),
        });
        if let Some(attachment) = previous {
            state.attachments.remove(&attachment);
            close_attachment(&state, attachment);
        }
        self.open_attachment(&state, id);
        Ok(())
    }

    /// Input for the tab's attachment, held while it is still opening.
    pub fn write(&self, id: SessionId, bytes: &[u8]) -> Result<(), SessionError> {
        let mut state = self.state.lock().unwrap();
        let handle = state.handle.clone();
        let tab = state.tabs.get_mut(&id).ok_or(SessionError::NotFound(id))?;
        let binding = tab
            .binding
            .as_mut()
            .ok_or_else(|| SessionError::Daemon("the session isn't attached".into()))?;
        match (binding.attachment, handle) {
            (Some(attachment), Some(handle)) => handle.send_frame(Frame::Input {
                attachment,
                data: bytes.to_vec(),
            }),
            _ => binding.input.extend_from_slice(bytes),
        }
        Ok(())
    }

    pub fn resize(&self, id: SessionId, cols: u16, rows: u16) -> Result<(), SessionError> {
        let mut state = self.state.lock().unwrap();
        let handle = state.handle.clone();
        let tab = state.tabs.get_mut(&id).ok_or(SessionError::NotFound(id))?;
        let Some(binding) = tab.binding.as_mut() else {
            return Ok(());
        };
        (binding.cols, binding.rows) = (cols, rows);
        if let (Some(attachment), Some(handle)) = (binding.attachment, handle) {
            let params = AttachmentResizeParams {
                attachment,
                cols,
                rows,
            };
            tauri::async_runtime::spawn(async move {
                let _ = call::<Value>(&handle, method::SESSION_ATTACHMENT_RESIZE, params).await;
            });
        }
        Ok(())
    }

    /// The cumulative count of data bytes the channel has drawn.
    pub fn ack(&self, id: SessionId, bytes: u64) {
        let state = self.state.lock().unwrap();
        let Some(binding) = state.tabs.get(&id).and_then(|tab| tab.binding.as_ref()) else {
            return;
        };
        // Acks below `base` are for an attachment that is gone.
        if let (Some(attachment), Some(handle), Some(bytes)) = (
            binding.attachment,
            &state.handle,
            bytes.checked_sub(binding.base),
        ) {
            let params = AttachmentAckParams { attachment, bytes };
            handle.notify(
                method::SESSION_ATTACHMENT_ACK,
                Some(serde_json::to_value(params).expect("protocol types serialize")),
            );
        }
    }

    fn key_and_root(&self, id: SessionId) -> Result<(String, PathBuf), SessionError> {
        let state = self.state.lock().unwrap();
        let tab = state.tabs.get(&id).ok_or(SessionError::NotFound(id))?;
        Ok((tab.info.key.clone(), tab.root.clone()))
    }

    async fn close_key(&self, key: &str) -> Result<(), SessionError> {
        let params = SessionRef {
            session: key.into(),
        };
        match self.call::<Value>(method::SESSION_CLOSE, params).await {
            Ok(_) | Err(Failure::Rpc(ErrorCode::UnknownSession, _)) => {
                self.forget(key);
                Ok(())
            }
            Err(failure) => Err(failure.into()),
        }
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        params: impl Serialize,
    ) -> Result<T, Failure> {
        let handle = self.state.lock().unwrap().handle.clone();
        call(&handle.ok_or(Failure::Disconnected)?, method, params).await
    }

    /// Updates the mirror from the daemon's view of one session.
    fn mirror(&self, session: Session) -> SessionInfo {
        let (info, event) = self.upsert(&mut self.state.lock().unwrap(), session);
        self.emit_all(event);
        info
    }

    fn upsert(&self, state: &mut State, session: Session) -> (SessionInfo, Option<SessionEvent>) {
        let next = state.next_id + 1;
        let id = *state.ids.entry(session.id.clone()).or_insert(next);
        if id == next {
            state.next_id = next;
        }
        let info = SessionInfo {
            id,
            plane_id: session
                .plane
                .clone()
                .unwrap_or_else(|| session.root.clone()),
            key: session.id,
            seq: session.seq,
            status: status(session.status),
            title: session.title.filter(|title| !title.trim().is_empty()),
            name: session.name,
            shell_name: self.config.shell_name.clone(),
            root_gone: session.root_gone,
            agent: session.agent.map(|agent| agent.kind),
        };
        let event = SessionEvent {
            id,
            session: Some(info.clone()),
        };
        match state.tabs.get_mut(&id) {
            Some(tab) if tab.info == info => (info, None),
            Some(tab) => {
                tab.info = info.clone();
                (info, Some(event))
            }
            None => {
                let root = PathBuf::from(&session.root);
                let tab = Tab {
                    info: info.clone(),
                    root,
                    binding: None,
                };
                state.tabs.insert(id, tab);
                (info, Some(event))
            }
        }
    }

    fn forget(&self, key: &str) {
        let event = remove(&mut self.state.lock().unwrap(), key);
        self.emit_all(event);
    }

    fn emit_all(&self, events: impl IntoIterator<Item = SessionEvent>) {
        for event in events {
            // A shell that exits 0 closes its tab, as it did before the daemon;
            // the exited state isn't shown on the way. A Claude terminal stays.
            let clean_exit = event
                .session
                .as_ref()
                .filter(|s| s.agent.is_none() && s.status == SessionStatus::Exited { code: 0 })
                .map(|s| s.key.clone());
            let Some(key) = clean_exit else {
                (self.emit)(event);
                continue;
            };
            let me = self.me.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(sessions) = me.upgrade() {
                    let _ = sessions.close_key(&key).await;
                }
            });
        }
    }

    fn connected(&self, handle: Handle) {
        let connection = {
            let mut state = self.state.lock().unwrap();
            state.connection += 1;
            state.handle = Some(handle.clone());
            state.attachments.clear();
            state.early.clear();
            state.listing = Some(Touched::default());
            for binding in state.tabs.values_mut().filter_map(|t| t.binding.as_mut()) {
                binding.attachment = None;
            }
            state.connection
        };
        let me = self.me.clone();
        tauri::async_runtime::spawn(async move {
            if let Some(sessions) = me.upgrade() {
                sessions.resync(handle, connection).await;
            }
        });
    }

    /// Diff `session.list` by id against the tabs, so they keep their ids,
    /// then reattach every bound tab on the new connection.
    async fn resync(&self, handle: Handle, connection: u64) {
        let listed = call::<SessionListResult>(&handle, method::SESSION_LIST, ()).await;
        let mut events = Vec::new();
        {
            let mut state = self.state.lock().unwrap();
            if state.connection != connection {
                return;
            }
            let touched = state.listing.take().unwrap_or_default();
            let Ok(SessionListResult { sessions }) = listed else {
                return;
            };
            let listed: HashSet<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
            let gone: Vec<String> = state
                .ids
                .keys()
                .filter(|key| !listed.contains(key.as_str()) && !touched.changed.contains(*key))
                .cloned()
                .collect();
            for key in gone {
                events.extend(remove(&mut state, &key));
            }
            for session in sessions {
                if !touched.changed.contains(&session.id) && !touched.removed.contains(&session.id)
                {
                    events.extend(self.upsert(&mut state, session).1);
                }
            }
            let bound: Vec<SessionId> = state
                .tabs
                .iter()
                .filter(|(_, tab)| tab.binding.is_some())
                .map(|(&id, _)| id)
                .collect();
            for id in bound {
                state.next_binding += 1;
                let next = state.next_binding;
                if let Some(binding) = state.tabs.get_mut(&id).and_then(|t| t.binding.as_mut()) {
                    binding.id = next;
                }
                self.open_attachment(&state, id);
            }
        }
        self.emit_all(events);
        self.ready.send_replace(true);
    }

    /// Opens an attachment for the tab's current binding, when connected.
    fn open_attachment(&self, state: &State, id: SessionId) {
        let (Some(handle), Some(tab)) = (state.handle.clone(), state.tabs.get(&id)) else {
            return;
        };
        let Some(binding) = &tab.binding else { return };
        let params = AttachmentOpenParams {
            session: tab.info.key.clone(),
            cols: binding.cols,
            rows: binding.rows,
            read_only: false,
            ignore_size: false,
        };
        let (binding, connection) = (binding.id, state.connection);
        let me = self.me.clone();
        tauri::async_runtime::spawn(async move {
            let opened =
                call::<AttachmentOpenResult>(&handle, method::SESSION_ATTACHMENT_OPEN, params)
                    .await;
            match (opened, me.upgrade()) {
                (Ok(opened), Some(sessions)) => {
                    sessions.attached(id, binding, connection, opened);
                }
                (Err(failure), _) => {
                    eprintln!("bitplane-cc: couldn't attach session {id}: {failure}");
                }
                (Ok(_), None) => {}
            }
        });
    }

    /// The attachment's reply: frames that came before it go out first.
    fn attached(&self, id: SessionId, binding: u64, connection: u64, opened: AttachmentOpenResult) {
        let attachment = opened.attachment;
        let forwarding = self.forwarding.lock().unwrap();
        let (sink, messages, event) = {
            let mut state = self.state.lock().unwrap();
            if state.connection != connection {
                return;
            }
            let early = state.early.remove(&attachment).unwrap_or_default();
            let handle = state.handle.clone();
            let current = state
                .tabs
                .get_mut(&id)
                .and_then(|tab| tab.binding.as_mut())
                .filter(|b| b.id == binding);
            let Some(current) = current else {
                close_attachment(&state, attachment);
                return;
            };
            current.attachment = Some(attachment);
            current.base = current.forwarded;
            let input = std::mem::take(&mut current.input);
            if let (false, Some(handle)) = (input.is_empty(), handle) {
                handle.send_frame(Frame::Input {
                    attachment,
                    data: input,
                });
            }
            let messages: Vec<Vec<u8>> = early
                .into_iter()
                .filter_map(|frame| message(current, frame))
                .collect();
            let sink = Arc::clone(&current.sink);
            state.attachments.insert(attachment, id);
            let (_, event) = self.upsert(&mut state, opened.session);
            (sink, messages, event)
        };
        for message in messages {
            sink.send(message);
        }
        drop(forwarding);
        self.emit_all(event);
    }

    fn frame(&self, frame: Frame) {
        let attachment = match &frame {
            Frame::Snapshot { attachment, .. } | Frame::Output { attachment, .. } => *attachment,
            Frame::Json(_) | Frame::Input { .. } => return,
        };
        let _forwarding = self.forwarding.lock().unwrap();
        let (sink, message) = {
            let mut state = self.state.lock().unwrap();
            let Some(&id) = state.attachments.get(&attachment) else {
                state.early.entry(attachment).or_default().push(frame);
                return;
            };
            let Some(binding) = state.tabs.get_mut(&id).and_then(|t| t.binding.as_mut()) else {
                return;
            };
            let Some(message) = message(binding, frame) else {
                return;
            };
            (Arc::clone(&binding.sink), message)
        };
        sink.send(message);
    }

    fn event(&self, event: Notification) {
        match event.method.as_str() {
            method::SESSION_CHANGED => {
                let Some(SessionResult { session }) = params(event.params) else {
                    return;
                };
                let event = {
                    let mut state = self.state.lock().unwrap();
                    if let Some(touched) = state.listing.as_mut() {
                        touched.changed.insert(session.id.clone());
                    }
                    self.upsert(&mut state, session).1
                };
                self.emit_all(event);
            }
            method::SESSION_REMOVED => {
                let Some(SessionRef { session }) = params(event.params) else {
                    return;
                };
                if let Some(touched) = self.state.lock().unwrap().listing.as_mut() {
                    touched.removed.insert(session.clone());
                }
                self.forget(&session);
            }
            method::SESSION_ATTACHMENT_ERROR => {
                if let Some(error) = params::<AttachmentError>(event.params) {
                    eprintln!(
                        "bitplane-cc: attachment {}: {}",
                        error.attachment, error.message
                    );
                }
            }
            _ => {}
        }
    }
}

/// What `Sessions` subscribes with; weak, so the link doesn't keep it alive.
struct Subscription(Weak<Sessions>);

impl Subscriber for Subscription {
    fn connected(&self, handle: Handle) {
        if let Some(sessions) = self.0.upgrade() {
            sessions.connected(handle);
        }
    }

    fn event(&self, event: Notification) {
        if let Some(sessions) = self.0.upgrade() {
            sessions.event(event);
        }
    }

    fn frame(&self, frame: Frame) {
        if let Some(sessions) = self.0.upgrade() {
            sessions.frame(frame);
        }
    }
}

#[derive(Debug)]
enum Failure {
    Disconnected,
    Rpc(ErrorCode, String),
    Other(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Disconnected => f.write_str("the session daemon isn't connected"),
            Failure::Rpc(_, message) | Failure::Other(message) => f.write_str(message),
        }
    }
}

impl From<Failure> for SessionError {
    fn from(failure: Failure) -> Self {
        SessionError::Daemon(failure.to_string())
    }
}

async fn call<T: DeserializeOwned>(
    handle: &Handle,
    method: &str,
    params: impl Serialize,
) -> Result<T, Failure> {
    let params = serde_json::to_value(params).expect("protocol types serialize");
    let params = (!params.is_null()).then_some(params);
    match handle.call(method, params).await {
        Ok(value) => serde_json::from_value(value).map_err(|e| Failure::Other(e.to_string())),
        Err(ClientError::Rpc(error)) => Err(match error.data {
            Some(data) => Failure::Rpc(data.code, error.message),
            None => Failure::Other(error.message),
        }),
        Err(ClientError::Closed) => Err(Failure::Disconnected),
        Err(error) => Err(Failure::Other(error.to_string())),
    }
}

fn close_attachment(state: &State, attachment: AttachmentId) {
    let Some(handle) = state.handle.clone() else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        let params = AttachmentRef { attachment };
        let _ = call::<Value>(&handle, method::SESSION_ATTACHMENT_CLOSE, params).await;
    });
}

fn params<T: DeserializeOwned>(params: Option<Value>) -> Option<T> {
    serde_json::from_value(params?).ok()
}

/// The channel message for an attachment frame, counted as forwarded.
fn message(binding: &mut Binding, frame: Frame) -> Option<Vec<u8>> {
    let (kind, data) = match frame {
        Frame::Snapshot { data, .. } => (FRAME_SNAPSHOT, data),
        Frame::Output { data, .. } => (FRAME_OUTPUT, data),
        Frame::Json(_) | Frame::Input { .. } => return None,
    };
    binding.forwarded += data.len() as u64;
    let mut message = Vec::with_capacity(data.len() + 1);
    message.push(kind);
    message.extend_from_slice(&data);
    Some(message)
}

fn remove(state: &mut State, key: &str) -> Option<SessionEvent> {
    let id = state.ids.remove(key)?;
    let tab = state.tabs.remove(&id)?;
    if let Some(attachment) = tab.binding.and_then(|b| b.attachment) {
        state.attachments.remove(&attachment);
    }
    Some(SessionEvent { id, session: None })
}

fn status(status: bitplane_protocol::session::SessionStatus) -> SessionStatus {
    use bitplane_protocol::session::SessionStatus as Daemon;
    match status {
        Daemon::Running | Daemon::Unknown => SessionStatus::Running,
        Daemon::Exited { code } => SessionStatus::Exited { code },
        Daemon::SpawnFailed { error } => SessionStatus::SpawnFailed { error },
    }
}
