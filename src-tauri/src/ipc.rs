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
use tauri::{AppHandle, State, Wry};
use tauri_specta::{Builder, ErrorHandlingMode, Event, collect_commands, collect_events};

use crate::app::{App, PlanesModel, Snapshot};
use crate::bp::Project;
use crate::create::{CreateArgs, CreateEvent};
use crate::hooks::{self, AgentHooks};
use crate::planes;
use crate::quit::{self, QuitInfo};
use crate::sessions::{BusyInfo, OutputSink, SessionError, SessionEvent, SessionId, SessionInfo};

pub type AppState = Arc<App>;

/// Header carrying the session id on `pty_write`.
pub const SESSION_HEADER: &str = "x-session-id";

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct PlanesChanged(pub PlanesModel);

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct SessionChanged(pub SessionEvent);

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct CreateChanged(pub CreateEvent);

/// The user asked to quit (⌘Q or closing the window); the frontend decides
/// whether to confirm.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct QuitRequested;

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
            session_set_title,
            pty_resize,
            pty_ack,
            plane_create_draft,
            plane_create,
            plane_create_cancel,
            plane_create_dismiss,
            project_list,
            quit_check,
            quit,
            ui_state_load,
            ui_state_save,
            agent_hooks_describe,
            agent_hooks_install,
        ])
        .events(collect_events![
            PlanesChanged,
            SessionChanged,
            CreateChanged,
            QuitRequested
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
fn snapshot(app: State<'_, AppState>) -> Snapshot {
    app.snapshot()
}

#[tauri::command]
#[specta::specta]
async fn refresh(app: State<'_, AppState>) -> Result<(), String> {
    app.refresh().await;
    Ok(())
}

/// Spawn a session in a plane's root. A missing root spawns nothing and
/// refreshes the list, which marks the plane gone.
#[tauri::command]
#[specta::specta]
async fn session_open(app: State<'_, AppState>, plane_id: String) -> Result<SessionInfo, String> {
    let dir = app.plane_dir(&plane_id)?;
    match app.sessions.open(&plane_id, &dir) {
        Err(error @ SessionError::RootMissing(_)) => {
            app.refresh_list().await;
            Err(error.to_string())
        }
        result => result.map_err(|e| e.to_string()),
    }
}

#[tauri::command]
#[specta::specta]
async fn session_restart(app: State<'_, AppState>, id: SessionId) -> Result<SessionInfo, String> {
    match app.sessions.restart(id) {
        Err(error @ SessionError::RootMissing(_)) => {
            app.refresh_list().await;
            Err(error.to_string())
        }
        result => result.map_err(|e| e.to_string()),
    }
}

#[tauri::command]
#[specta::specta]
fn session_close(app: State<'_, AppState>, id: SessionId) {
    app.sessions.close(id);
}

#[tauri::command]
#[specta::specta]
fn session_busy(app: State<'_, AppState>, id: SessionId) -> Result<BusyInfo, String> {
    app.sessions.busy(id).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
fn session_set_title(app: State<'_, AppState>, id: SessionId, title: String) {
    app.sessions.set_title(id, title);
}

#[tauri::command]
#[specta::specta]
fn pty_resize(app: State<'_, AppState>, id: SessionId, cols: u16, rows: u16) -> Result<(), String> {
    app.sessions
        .resize(id, cols, rows)
        .map_err(|e| e.to_string())
}

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
async fn project_list(app: State<'_, AppState>) -> Result<Vec<Project>, String> {
    app.bp()?.projects().await.map_err(|e| e.message())
}

#[tauri::command]
#[specta::specta]
fn quit_check(app: State<'_, AppState>) -> QuitInfo {
    quit::check(&app)
}

#[tauri::command]
#[specta::specta]
fn quit(handle: AppHandle, app: State<'_, AppState>) {
    quit::quit(&handle, &app);
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

#[tauri::command]
#[specta::specta]
fn agent_hooks_describe(app: State<'_, AppState>) -> Result<AgentHooks, String> {
    hooks::describe(&agent_settings(&app)?)
}

#[tauri::command]
#[specta::specta]
fn agent_hooks_install(app: State<'_, AppState>) -> Result<(), String> {
    hooks::install(&agent_settings(&app)?)
}

fn agent_settings(app: &App) -> Result<std::path::PathBuf, String> {
    app.home()
        .map(|home| hooks::settings_path(&home))
        .ok_or_else(|| "HOME isn't set".into())
}

/// Input: the raw body, with the session id in a header.
#[tauri::command]
async fn pty_write(app: State<'_, AppState>, request: Request<'_>) -> Result<(), String> {
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
    let app = AppState::clone(&app);
    tauri::async_runtime::spawn_blocking(move || app.sessions.write(id, &bytes))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Bind an output channel; the ack window restarts at zero (spec §7.5).
#[tauri::command]
fn session_attach(
    app: State<'_, AppState>,
    id: SessionId,
    channel: Channel<InvokeResponseBody>,
) -> Result<(), String> {
    app.sessions
        .attach(id, Arc::new(ChannelSink(channel)))
        .map_err(|e| e.to_string())
}

struct ChannelSink(Channel<InvokeResponseBody>);

impl OutputSink for ChannelSink {
    fn send(&self, bytes: Vec<u8>) -> bool {
        self.0.send(InvokeResponseBody::Raw(bytes)).is_ok()
    }
}
