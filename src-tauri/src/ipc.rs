//! Commands, events and specta registration (spec §7.2).
//!
//! `pty_write` and `session_attach` carry raw bytes and a `Channel`, which
//! specta can't describe, so they're registered beside the typed commands and
//! wrapped by hand in `src/ipc/pty.ts`.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::ipc::Invoke;
use tauri::ipc::{Channel, InvokeBody, InvokeResponseBody, Request};
use tauri::{State, Window, Wry};
use tauri_specta::{Builder, ErrorHandlingMode, Event, collect_commands, collect_events};

use crate::agents::{AgentEvent, AgentKindInfo};
use crate::app::{App, PlanesModel, Snapshot};
use crate::bp::{ErrorEnvelope, Project};
use crate::create::{BranchIntent, CreateArgs, CreateEvent};
use crate::daemon::{DaemonInfo, DaemonLink, DaemonStatus};
use crate::diff::{DiffMode, FileContents, FileDiff, FileText};
use crate::editor;
use crate::files::FileEntry;
use crate::git::Repo;
use crate::planes;
use crate::sessions::{
    AgentOpen, BusyInfo, OpenOptions, OutputSink, SessionError, SessionEvent, SessionId,
    SessionInfo,
};
use crate::workspace::{FileTabsEvent, FilesEvent, GitTarget, SourceControlEvent};

pub type AppState = Arc<App>;
pub type DaemonState = Arc<DaemonLink>;

/// Header carrying the session id on `pty_write`.
pub const SESSION_HEADER: &str = "x-session-id";

/// Rust types that `src/ipc/bp-contract.ts`, bp's generated wire types, also
/// defines under the same name: `bindings.ts` imports them from there.
macro_rules! bp_contract {
    ($($name:ident),* $(,)?) => {
        $(impl Type for $name {
            fn definition(_: &mut specta::Types) -> specta::datatype::DataType {
                specta_typescript::define(concat!("import(\"./bp-contract\").", stringify!($name))).into()
            }
        })*

        #[cfg(test)]
        const BP_CONTRACT_TYPES: &[&str] = &[$(stringify!($name)),*];
    };
}

bp_contract!(ErrorEnvelope, BranchIntent);

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct PlanesChanged(pub PlanesModel);

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct SessionChanged(pub SessionEvent);

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct AgentChanged(pub AgentEvent);

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct AgentKindsChanged(pub Vec<AgentKindInfo>);

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct CreateChanged(pub CreateEvent);

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct SourceControlChanged(pub SourceControlEvent);

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct FilesChanged(pub FilesEvent);

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct FileTabsChanged(pub FileTabsEvent);

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct DaemonChanged(pub DaemonStatus);

/// The typed half of the IPC surface.
pub fn builder() -> Builder<Wry> {
    Builder::<Wry>::new()
        .commands(collect_commands![
            snapshot,
            refresh,
            session_open,
            session_restart,
            session_close,
            session_busy,
            session_rename,
            pty_resize,
            pty_ack,
            plane_create_draft,
            plane_create,
            plane_create_cancel,
            plane_create_dismiss,
            plane_open_in_vscode,
            plane_source_control,
            plane_git_watch,
            plane_list_dir,
            plane_files_watch,
            plane_open_file,
            plane_file_diff,
            plane_file_contents,
            plane_read_file,
            project_list,
            ui_state_load,
            ui_state_save,
            daemon_status,
            daemon_retry,
            daemon_info,
        ])
        .events(collect_events![
            PlanesChanged,
            SessionChanged,
            AgentChanged,
            AgentKindsChanged,
            CreateChanged,
            SourceControlChanged,
            FilesChanged,
            FileTabsChanged,
            DaemonChanged
        ])
        .error_handling(ErrorHandlingMode::Throw)
        .dangerously_cast_bigints_to_number()
}

/// Every command: the typed ones plus the raw transport.
pub fn invoke_handler(
    builder: &Builder<Wry>,
) -> impl Fn(Invoke<Wry>) -> bool + Send + Sync + 'static {
    let typed = builder.invoke_handler();
    let raw = raw_handler();
    move |invoke| match invoke.message.command() {
        "pty_write" | "session_attach" => raw(invoke),
        _ => typed(invoke),
    }
}

fn raw_handler() -> impl Fn(Invoke<Wry>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![pty_write, session_attach]
}

#[tauri::command]
#[specta::specta]
async fn snapshot(app: State<'_, AppState>) -> Result<Snapshot, String> {
    Ok(app.snapshot().await)
}

#[tauri::command]
#[specta::specta]
fn daemon_status(daemon: State<'_, DaemonState>) -> DaemonStatus {
    daemon.status()
}

#[tauri::command]
#[specta::specta]
fn daemon_info(daemon: State<'_, DaemonState>) -> DaemonInfo {
    daemon.info()
}

#[tauri::command]
#[specta::specta]
fn daemon_retry(daemon: State<'_, DaemonState>) {
    daemon.retry();
}

#[tauri::command]
#[specta::specta]
async fn refresh(app: State<'_, AppState>) -> Result<(), String> {
    app.refresh().await;
    Ok(())
}

/// Open a session in a plane's root: an agent terminal running `agent`, or
/// without it a regular terminal. A missing root opens nothing and refreshes
/// the list, which marks the plane gone.
#[tauri::command]
#[specta::specta]
async fn session_open(
    app: State<'_, AppState>,
    plane_id: String,
    agent: Option<AgentOpen>,
) -> Result<SessionInfo, OpenError> {
    let dir = app
        .plane_dir(&plane_id)
        .map_err(|message| OpenError::Other { message })?;
    let options = OpenOptions { name: None, agent };
    let result = app.sessions.open(&plane_id, &dir, options).await;
    refresh_if_gone(&app, result).await.map_err(OpenError::from)
}

/// Why `session_open` failed; `agentLive` when the agent session to resume
/// already runs in another session.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum OpenError {
    AgentLive { message: String },
    Other { message: String },
}

impl From<SessionError> for OpenError {
    fn from(error: SessionError) -> Self {
        let message = error.to_string();
        match error {
            SessionError::AgentLive => OpenError::AgentLive { message },
            _ => OpenError::Other { message },
        }
    }
}

#[tauri::command]
#[specta::specta]
async fn session_restart(app: State<'_, AppState>, id: SessionId) -> Result<SessionInfo, String> {
    let result = app.sessions.restart(id).await;
    refresh_if_gone(&app, result)
        .await
        .map_err(|e| e.to_string())
}

/// Closes the session for every client of the daemon.
#[tauri::command]
#[specta::specta]
async fn session_close(app: State<'_, AppState>, id: SessionId) -> Result<(), String> {
    app.sessions.close(id).await.map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
async fn session_busy(app: State<'_, AppState>, id: SessionId) -> Result<BusyInfo, String> {
    app.sessions.busy(id).await.map_err(|e| e.to_string())
}

/// The tab's own name, apart from the title the program sets; `None` clears it.
#[tauri::command]
#[specta::specta]
async fn session_rename(
    app: State<'_, AppState>,
    id: SessionId,
    name: Option<String>,
) -> Result<SessionInfo, String> {
    app.sessions
        .rename(id, name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn pty_resize(app: State<'_, AppState>, id: SessionId, cols: u16, rows: u16) -> Result<(), String> {
    app.sessions
        .resize(id, cols, rows)
        .map_err(|e| e.to_string())
}

/// The cumulative count of bytes the terminal has drawn since it attached.
#[tauri::command]
#[specta::specta]
fn pty_ack(app: State<'_, AppState>, id: SessionId, bytes: u64) {
    app.sessions.ack(id, bytes);
}

#[tauri::command]
#[specta::specta]
async fn plane_create_draft(app: State<'_, AppState>, id: String) -> Result<(), String> {
    app.create_draft(&id).await
}

#[tauri::command]
#[specta::specta]
async fn plane_create(app: State<'_, AppState>, args: CreateArgs) -> Result<(), String> {
    planes::validate_plane_id(&args.id)?;
    app.creates.start(app.bp()?, args)
}

#[tauri::command]
#[specta::specta]
fn plane_create_cancel(app: State<'_, AppState>, id: String) {
    app.creates.cancel(&id);
}

#[tauri::command]
#[specta::specta]
fn plane_create_dismiss(app: State<'_, AppState>, id: String) {
    app.creates.dismiss(&id);
}

#[tauri::command]
#[specta::specta]
async fn plane_open_in_vscode(app: State<'_, AppState>, plane_id: String) -> Result<(), String> {
    editor::open_in_vscode(&app.plane_dir(&plane_id)?, &app.tool_env()).await
}

#[tauri::command]
#[specta::specta]
async fn plane_source_control(
    app: State<'_, AppState>,
    plane_id: String,
) -> Result<Vec<Repo>, String> {
    let (root, members) = app.source_control_target(&plane_id)?;
    app.workspace.source_control(&root, &members).await
}

/// Keep what `target` shows in this window current, through
/// `source_control_changed` and `file_tabs_changed`; `None` stops. Sync, so
/// calls apply in the order the frontend made them.
#[tauri::command]
#[specta::specta]
fn plane_git_watch(app: State<'_, AppState>, window: Window, target: Option<GitTarget>) {
    app.watch_git(window.label(), target);
}

/// One folder of a plane for Files; `path` is relative to the plane root.
#[tauri::command]
#[specta::specta]
async fn plane_list_dir(
    app: State<'_, AppState>,
    plane_id: String,
    path: String,
) -> Result<Vec<FileEntry>, String> {
    let (root, _) = app.source_control_target(&plane_id)?;
    app.workspace.list_dir(&root, &path).await
}

/// Report changes in `dirs` of `plane_id` to this window through
/// `files_changed`; `None` stops. Sync, so calls apply in the order the
/// frontend made them.
#[tauri::command]
#[specta::specta]
fn plane_files_watch(
    app: State<'_, AppState>,
    window: Window,
    plane_id: Option<String>,
    dirs: Vec<String>,
) {
    app.watch_files(window.label(), plane_id.map(|id| (id, dirs)));
}

#[tauri::command]
#[specta::specta]
async fn plane_open_file(
    app: State<'_, AppState>,
    plane_id: String,
    path: String,
    line: Option<u32>,
) -> Result<(), String> {
    editor::open_file_in_vscode(&app.plane_dir(&plane_id)?, &path, line, &app.tool_env()).await
}

/// What a File tab of `path` in `mode` shows; `path` is relative to the plane
/// root. `force` diffs a file even past the size limit ("Show anyway").
#[tauri::command]
#[specta::specta]
async fn plane_file_diff(
    app: State<'_, AppState>,
    plane_id: String,
    path: String,
    mode: DiffMode,
    force: bool,
) -> Result<FileDiff, String> {
    let (root, _) = app.source_control_target(&plane_id)?;
    app.workspace.file_diff(&root, &path, mode, force).await
}

/// Both sides of `path`'s diff in `mode`, for expanding unchanged context.
#[tauri::command]
#[specta::specta]
async fn plane_file_contents(
    app: State<'_, AppState>,
    plane_id: String,
    path: String,
    mode: DiffMode,
) -> Result<FileContents, String> {
    let (root, _) = app.source_control_target(&plane_id)?;
    app.workspace.file_contents(&root, &path, mode).await
}

#[tauri::command]
#[specta::specta]
async fn plane_read_file(
    app: State<'_, AppState>,
    plane_id: String,
    path: String,
) -> Result<FileText, String> {
    let (root, _) = app.source_control_target(&plane_id)?;
    app.workspace.read_file(&root, &path).await
}

#[tauri::command]
#[specta::specta]
async fn project_list(app: State<'_, AppState>) -> Result<Vec<Project>, String> {
    app.bp()?.projects().await.map_err(|e| e.message())
}

/// The frontend's own state (selection, collapsed planes, zoom), opaque here.
#[tauri::command]
#[specta::specta]
fn ui_state_load(app: State<'_, AppState>) -> Option<String> {
    app.ui_state_load()
}

#[tauri::command]
#[specta::specta]
fn ui_state_save(app: State<'_, AppState>, json: String) -> Result<(), String> {
    app.ui_state_save(&json)
}

/// Input: the raw body, with the session id in a header.
#[tauri::command]
fn pty_write(app: State<'_, AppState>, request: Request<'_>) -> Result<(), String> {
    let id: SessionId = request
        .headers()
        .get(SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .ok_or("missing session id header")?;
    let bytes = match request.body() {
        InvokeBody::Raw(bytes) => bytes.clone(),
        InvokeBody::Json(value) => value
            .as_str()
            .ok_or("the body must be raw bytes")?
            .as_bytes()
            .to_vec(),
    };
    app.sessions.write(id, &bytes).map_err(|e| e.to_string())
}

/// Bind an output channel at the terminal's size: it starts with a snapshot,
/// and acks count from zero again.
#[tauri::command]
fn session_attach(
    app: State<'_, AppState>,
    id: SessionId,
    cols: u16,
    rows: u16,
    channel: Channel<InvokeResponseBody>,
) -> Result<(), String> {
    app.sessions
        .attach(id, cols, rows, Arc::new(ChannelSink(channel)))
        .map_err(|e| e.to_string())
}

/// A missing root refreshes the list, which marks the plane gone.
async fn refresh_if_gone(
    app: &App,
    result: Result<SessionInfo, SessionError>,
) -> Result<SessionInfo, SessionError> {
    if let Err(SessionError::RootMissing(_)) = &result {
        app.refresh_list().await;
    }
    result
}

struct ChannelSink(Channel<InvokeResponseBody>);

impl OutputSink for ChannelSink {
    fn send(&self, bytes: Vec<u8>) -> bool {
        self.0.send(InvokeResponseBody::Raw(bytes)).is_ok()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_bp_contract_type_is_exported_by_bp_contract_ts() {
        let contract = include_str!("../../src/ipc/bp-contract.ts");
        for name in super::BP_CONTRACT_TYPES {
            assert!(
                contract.contains(&format!("\nexport type {name} ")),
                "src/ipc/bp-contract.ts has no `{name}`"
            );
        }
    }
}
