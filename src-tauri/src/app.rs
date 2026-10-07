//! The backend's state, owned in one place (ADR 0004): the plane list and
//! what it's built from, plus the session and create registries.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::Notify;

use crate::bp::{self, Bp, BpError, VersionError};
use crate::create::{CreateEvent, CreateInfo, CreateStatus, Creates};
use crate::env::{self, CapturedEnv, EnvMap};
use crate::planes::{self, PlaneRow, Source, Watcher};
use crate::sessions::{SessionEvent, SessionInfo, Sessions, SpawnConfig};

/// Whether the app can talk to bp, and if not, why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AppStatus {
    Loading,
    Ready,
    BpNotFound,
    #[serde(rename_all = "camelCase")]
    BpTooOld {
        found: String,
        required: String,
    },
    #[serde(rename_all = "camelCase")]
    BpIncompatible {
        version: Option<String>,
        error: String,
    },
    /// git missing or older than 2.36 (bp exit 4).
    #[serde(rename_all = "camelCase")]
    GitMissing {
        message: String,
    },
    /// `bp list` failed for another reason.
    #[serde(rename_all = "camelCase")]
    ListFailed {
        message: String,
    },
}

/// `planes_changed`: the whole sidebar model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanesModel {
    pub status: AppStatus,
    pub planes_dir: Option<String>,
    pub planes_dir_exists: bool,
    /// Unix time in milliseconds.
    pub refreshed_at: Option<f64>,
    pub env_warning: Option<String>,
    pub planes: Vec<PlaneRow>,
}

/// Everything the frontend mirrors, for `snapshot`.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub planes: PlanesModel,
    pub sessions: Vec<SessionInfo>,
    pub creates: Vec<CreateInfo>,
}

pub type EmitPlanes = Arc<dyn Fn(PlanesModel) + Send + Sync>;

/// Where events go; the Tauri layer forwards them to the webview.
pub struct Emitters {
    pub planes: EmitPlanes,
    pub session: Arc<dyn Fn(SessionEvent) + Send + Sync>,
    pub create: Arc<dyn Fn(CreateEvent) + Send + Sync>,
}

pub struct App {
    original_env: EnvMap,
    pub sessions: Arc<Sessions>,
    pub creates: Arc<Creates>,
    state: Mutex<State>,
    watcher: Mutex<Option<Watcher>>,
    remerge_lock: Mutex<()>,
    /// Watcher events land here; the refresh loop merges them.
    refresh_needed: Arc<Notify>,
    emit_planes: EmitPlanes,
}

#[derive(Default)]
struct State {
    bp: Option<Bp>,
    bp_version: Option<String>,
    planes_dir: Option<PathBuf>,
    status: Option<AppStatus>,
    env_warning: Option<String>,
    listed: Vec<Source>,
    drafts: Vec<Source>,
    refreshed_at: Option<f64>,
    model: Option<PlanesModel>,
}

impl App {
    pub fn new(original_env: EnvMap, app_version: String, emitters: Emitters) -> Arc<Self> {
        let shell = env::resolve_shell(original_env.get("SHELL").map(String::as_str));
        // Session and create events change the sidebar too, so they re-merge
        // it; the registries exist before the app, hence the late binding.
        let this: Arc<OnceLock<Weak<App>>> = Arc::new(OnceLock::new());

        let app = Arc::clone(&this);
        let emit_session = emitters.session;
        let sessions = Sessions::new(
            SpawnConfig {
                env: original_env.clone(),
                shell,
                app_version,
            },
            Arc::new(move |event| {
                emit_session(event);
                if let Some(app) = app.get().and_then(Weak::upgrade) {
                    app.remerge();
                }
            }),
        );

        let app = Arc::clone(&this);
        let emit_create = emitters.create;
        let creates = Creates::new(Arc::new(move |event: CreateEvent| {
            let finished = event
                .create
                .as_ref()
                .is_some_and(|c| c.status != CreateStatus::Running);
            emit_create(event);
            if let Some(app) = app.get().and_then(Weak::upgrade) {
                app.remerge();
                if finished {
                    app.refresh_needed.notify_one();
                }
            }
        }));

        let app = Arc::new(App {
            original_env,
            sessions,
            creates,
            state: Mutex::new(State::default()),
            watcher: Mutex::new(None),
            remerge_lock: Mutex::new(()),
            refresh_needed: Arc::new(Notify::new()),
            emit_planes: emitters.planes,
        });
        let _ = this.set(Arc::downgrade(&app));
        app
    }

    /// Start the watcher and the refresh loop. Needs a tokio runtime.
    pub fn start(self: &Arc<Self>) {
        let refresh_needed = Arc::clone(&self.refresh_needed);
        match Watcher::new(Arc::new(move || refresh_needed.notify_one())) {
            Ok(watcher) => *self.watcher.lock().unwrap() = Some(watcher),
            Err(error) => eprintln!("bitplane-cc: file watcher unavailable: {error}"),
        }
        let app = Arc::downgrade(self);
        let refresh_needed = Arc::clone(&self.refresh_needed);
        tokio::spawn(async move {
            loop {
                // A permit stored while a run is in flight becomes exactly one
                // follow-up run, however many events arrived.
                refresh_needed.notified().await;
                let Some(app) = app.upgrade() else { return };
                app.refresh_list().await;
            }
        });
    }

    /// Capture the environment again, resolve bp and the planes directory,
    /// then refresh the list (startup, Retry, manual refresh).
    pub async fn refresh(&self) {
        let captured = env::capture(&self.original_env, env::CAPTURE_TIMEOUT).await;
        let (bp, bp_version, status) = discover_bp(&captured).await;
        {
            let mut state = self.state.lock().unwrap();
            state.planes_dir = Some(planes::resolve_planes_dir(&captured.vars));
            state.env_warning = captured.warning;
            state.bp = bp;
            state.bp_version = bp_version;
            state.status = status;
        }
        self.refresh_list().await;
    }

    /// Run `bp list` and the draft scan.
    pub async fn refresh_list(&self) {
        let (bp, planes_dir) = {
            let state = self.state.lock().unwrap();
            (state.bp.clone(), state.planes_dir.clone())
        };
        let Some(planes_dir) = planes_dir else { return };
        if let Some(bp) = bp {
            let result = bp.list().await;
            let drafts = planes::scan_drafts(&planes_dir);
            let mut state = self.state.lock().unwrap();
            match result {
                Ok(listed) => {
                    state.listed = listed
                        .into_iter()
                        .map(|plane| Source {
                            id: plane.id,
                            directory: plane.directory,
                            findings: plane
                                .health
                                .unwrap_or_default()
                                .findings
                                .into_iter()
                                .map(|f| f.finding)
                                .collect(),
                        })
                        .collect();
                    state.status = Some(AppStatus::Ready);
                }
                Err(error) => {
                    let version = state.bp_version.clone();
                    state.status = Some(status_for_list_error(error, version));
                }
            }
            state.drafts = drafts;
            state.refreshed_at = Some(now_millis());
        }
        self.creates.update_phases();
        self.remerge();
    }

    /// Rebuild the sidebar model from what's cached, emitting it if it changed,
    /// and point the watcher at the current planes.
    pub fn remerge(&self) {
        // Held through the emit, so a stale model can never land after a newer one.
        let _ordered = self.remerge_lock.lock().unwrap();
        let with_sessions = self.sessions.plane_sources();
        let creating = self.creates.sources();
        let (model, watch) = {
            let mut state = self.state.lock().unwrap();
            let rows = planes::merge_rows(&state.listed, &state.drafts, &creating, &with_sessions);
            let planes_dir = state.planes_dir.clone();
            let model = PlanesModel {
                status: state.status.clone().unwrap_or(AppStatus::Loading),
                planes_dir: planes_dir
                    .as_ref()
                    .map(|d| d.to_string_lossy().into_owned()),
                planes_dir_exists: planes_dir.as_ref().is_some_and(|d| d.is_dir()),
                refreshed_at: state.refreshed_at,
                env_warning: state.env_warning.clone(),
                planes: rows,
            };
            if state.model.as_ref() == Some(&model) {
                return;
            }
            state.model = Some(model.clone());
            let dirs: Vec<PathBuf> = model
                .planes
                .iter()
                .map(|p| PathBuf::from(&p.directory))
                .collect();
            (model, planes_dir.map(|d| planes::watch_paths(&d, &dirs)))
        };
        if let (Some(paths), Some(watcher)) = (watch, self.watcher.lock().unwrap().as_mut()) {
            watcher.set_paths(paths);
        }
        (self.emit_planes)(model);
    }

    pub fn snapshot(&self) -> Snapshot {
        let planes = self.state.lock().unwrap().model.clone();
        Snapshot {
            planes: planes.unwrap_or_else(|| PlanesModel {
                status: AppStatus::Loading,
                planes_dir: None,
                planes_dir_exists: false,
                refreshed_at: None,
                env_warning: None,
                planes: Vec::new(),
            }),
            sessions: self.sessions.list(),
            creates: self.creates.list(),
        }
    }

    pub fn bp(&self) -> Result<Bp, String> {
        self.state
            .lock()
            .unwrap()
            .bp
            .clone()
            .ok_or_else(|| "bp isn't available".into())
    }

    pub fn planes_dir(&self) -> Result<PathBuf, String> {
        self.state
            .lock()
            .unwrap()
            .planes_dir
            .clone()
            .ok_or_else(|| "the planes directory isn't resolved yet".into())
    }

    /// A plane's root: its row's directory, else `<planes dir>/<id>` (a plane
    /// that was just created may not be listed yet).
    pub fn plane_dir(&self, plane_id: &str) -> Result<PathBuf, String> {
        let state = self.state.lock().unwrap();
        if let Some(row) = state
            .model
            .as_ref()
            .and_then(|m| m.planes.iter().find(|p| p.id == plane_id))
        {
            return Ok(PathBuf::from(&row.directory));
        }
        state
            .planes_dir
            .as_ref()
            .map(|dir| dir.join(plane_id))
            .ok_or_else(|| "the planes directory isn't resolved yet".into())
    }

    /// `mkdir` a draft plane, creating the planes directory if needed.
    pub async fn create_draft(&self, id: &str) -> Result<(), String> {
        planes::validate_plane_id(id)?;
        let planes_dir = self.planes_dir()?;
        make_draft(&planes_dir, id)?;
        self.refresh_list().await;
        Ok(())
    }
}

/// Find bp on the captured `PATH` and check its version.
async fn discover_bp(captured: &CapturedEnv) -> (Option<Bp>, Option<String>, Option<AppStatus>) {
    let Some(program) = bp::find_on_path("bp", captured.vars.get("PATH").map(String::as_str))
    else {
        return (None, None, Some(AppStatus::BpNotFound));
    };
    let candidate = Bp {
        program,
        env: captured.vars.clone(),
        planes_dir: planes::resolve_planes_dir(&captured.vars),
    };
    match candidate.version().await {
        Ok(version) => (Some(candidate), Some(version), None),
        Err(VersionError::NotFound(_)) => (None, None, Some(AppStatus::BpNotFound)),
        Err(VersionError::TooOld(found)) => (
            None,
            None,
            Some(AppStatus::BpTooOld {
                found,
                required: bp::format_version(bp::MIN_VERSION),
            }),
        ),
        Err(VersionError::Unparsable(error)) => (
            None,
            None,
            Some(AppStatus::BpIncompatible {
                version: None,
                error,
            }),
        ),
    }
}

fn status_for_list_error(error: BpError, version: Option<String>) -> AppStatus {
    match error {
        BpError::Git(message) => AppStatus::GitMissing { message },
        BpError::Incompatible(error) => AppStatus::BpIncompatible { version, error },
        BpError::Spawn(_) => AppStatus::BpNotFound,
        failed @ BpError::Failed { .. } => AppStatus::ListFailed {
            message: failed.message(),
        },
    }
}

fn make_draft(planes_dir: &Path, id: &str) -> Result<(), String> {
    std::fs::create_dir_all(planes_dir)
        .map_err(|e| format!("couldn't create {}: {e}", planes_dir.display()))?;
    let dir = planes_dir.join(id);
    std::fs::create_dir(&dir).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => format!("{id} already exists"),
        _ => format!("couldn't create {}: {e}", dir.display()),
    })
}

fn now_millis() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_creates_the_planes_dir_and_refuses_existing() {
        let dir = tempfile::tempdir().unwrap();
        let planes = dir.path().join("planes");
        make_draft(&planes, "scratch").unwrap();
        assert!(planes.join("scratch").is_dir());
        assert_eq!(
            make_draft(&planes, "scratch"),
            Err("scratch already exists".into())
        );
    }

    #[test]
    fn list_errors_map_to_statuses() {
        assert_eq!(
            status_for_list_error(BpError::Git("old git".into()), None),
            AppStatus::GitMissing {
                message: "old git".into()
            }
        );
        assert_eq!(
            status_for_list_error(BpError::Incompatible("bad".into()), Some("9.0.0".into())),
            AppStatus::BpIncompatible {
                version: Some("9.0.0".into()),
                error: "bad".into()
            }
        );
    }
}
