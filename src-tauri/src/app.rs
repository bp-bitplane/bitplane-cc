//! The backend's state, owned in one place (ADR 0004): the plane list and
//! what it's built from, plus the session and create registries and what
//! survives a restart (ADR 0005).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::Notify;

use crate::agents;
use crate::bp::{self, Bp, BpError, VersionError};
use crate::create::{CreateEvent, CreateInfo, CreateStatus, Creates};
use crate::env::{self, CapturedEnv, EnvMap};
use crate::persist::{self, Persister};
use crate::planes::{self, PlaneKind, PlaneRow, Source, Watcher};
use crate::sessions::{Restore, SessionEvent, SessionInfo, Sessions, SpawnConfig};

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
    /// Planes whose sessions weren't restored because their root is gone.
    pub restore_skipped: Vec<String>,
}

/// Where the app keeps what outlives it.
#[derive(Debug, Clone)]
pub struct DataDir(pub PathBuf);

impl DataDir {
    pub fn sessions(&self) -> PathBuf {
        self.0.join("sessions.json")
    }

    pub fn ui_state(&self) -> PathBuf {
        self.0.join("ui-state.json")
    }

    pub fn agents(&self) -> PathBuf {
        self.0.join("agents")
    }
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
    data_dir: Option<DataDir>,
    persister: Option<Arc<Persister>>,
    restore_skipped: Mutex<Vec<String>>,
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
    captured_env: Option<EnvMap>,
    listed: Vec<Source>,
    drafts: Vec<Source>,
    refreshed_at: Option<f64>,
    model: Option<PlanesModel>,
}

impl App {
    /// Without a data dir nothing is saved or restored.
    pub fn new(
        original_env: EnvMap,
        app_version: String,
        data_dir: Option<DataDir>,
        emitters: Emitters,
    ) -> Arc<Self> {
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
                agents_dir: data_dir.as_ref().map(DataDir::agents),
            },
            Arc::new(move |event| {
                emit_session(event);
                if let Some(app) = app.get().and_then(Weak::upgrade) {
                    if let Some(persister) = &app.persister {
                        persister.mark();
                    }
                    app.remerge();
                }
            }),
        );
        let persister = data_dir.as_ref().map(|dir| {
            let sessions = Arc::downgrade(&sessions);
            Persister::new(
                dir.sessions(),
                Arc::new(move || sessions.upgrade().map(|s| s.entries()).unwrap_or_default()),
            )
        });

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
            data_dir,
            persister,
            restore_skipped: Mutex::default(),
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

    /// Reopen the sessions open when the app last stopped, resuming their
    /// agent sessions. Runs before the frontend first asks for a snapshot.
    pub fn restore(&self) {
        let Some(dir) = &self.data_dir else { return };
        let agents_dir = dir.agents();
        if let Err(error) = std::fs::create_dir_all(&agents_dir) {
            eprintln!(
                "bitplane-cc: couldn't create {}: {error}",
                agents_dir.display()
            );
        }
        let mut restored = HashSet::new();
        let mut skipped: Vec<String> = Vec::new();
        for entry in persist::load(&dir.sessions()) {
            if !entry.plane_dir.is_dir() {
                if !skipped.contains(&entry.plane_id) {
                    skipped.push(entry.plane_id);
                }
                continue;
            }
            let link = agents::read(&agents_dir, &entry.key);
            let restore = plan_restore(entry, link);
            let key = restore.key.clone();
            if self.sessions.restore(restore).is_ok() {
                restored.insert(key);
            }
        }
        agents::sweep(&agents_dir, &restored);
        *self.restore_skipped.lock().unwrap() = skipped;
    }

    /// Save the sessions now and stop saving, then keep agent links from
    /// being deleted by the hang-up that quitting sends.
    pub fn prepare_quit(&self) {
        if let Some(persister) = &self.persister {
            persister.flush();
        }
        if let Some(dir) = &self.data_dir {
            agents::mark_quitting(&dir.agents());
        }
    }

    pub fn agent_link(&self, key: &str) -> Option<agents::AgentLink> {
        agents::read(&self.data_dir.as_ref()?.agents(), key)
    }

    pub fn ui_state_load(&self) -> Option<String> {
        std::fs::read_to_string(self.data_dir.as_ref()?.ui_state()).ok()
    }

    pub fn ui_state_save(&self, json: &str) -> Result<(), String> {
        let Some(dir) = &self.data_dir else {
            return Ok(());
        };
        persist::write_atomic(&dir.ui_state(), json.as_bytes()).map_err(|e| e.to_string())
    }

    pub fn home(&self) -> Option<PathBuf> {
        self.original_env.get("HOME").map(PathBuf::from)
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
            state.captured_env = Some(captured.vars);
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
                            member_paths: plane
                                .members
                                .iter()
                                .filter_map(|m| m.path.clone())
                                .collect(),
                            members: plane.members.into_iter().map(|m| m.member).collect(),
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
            // Taken, so a webview reload doesn't repeat the notice.
            restore_skipped: std::mem::take(&mut *self.restore_skipped.lock().unwrap()),
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

    /// What Source control reads for a plane: its root and its Members'
    /// worktrees, relative to the root.
    pub fn source_control_target(&self, plane_id: &str) -> Result<(PathBuf, Vec<PathBuf>), String> {
        let state = self.state.lock().unwrap();
        let row = state
            .model
            .as_ref()
            .and_then(|m| m.planes.iter().find(|p| p.id == plane_id))
            .ok_or_else(|| format!("there's no plane called {plane_id}"))?;
        match row.kind {
            PlaneKind::Gone => return Err(format!("{plane_id} is gone")),
            PlaneKind::Creating => return Err(format!("{plane_id} is still being created")),
            PlaneKind::Plane | PlaneKind::Draft => {}
        }
        let members = state
            .listed
            .iter()
            .find(|source| source.id == plane_id)
            .map(|source| source.member_paths.clone())
            .unwrap_or_default();
        Ok((PathBuf::from(&row.directory), members))
    }

    /// The environment external tools run in: the captured one once it's known.
    pub fn tool_env(&self) -> EnvMap {
        self.state
            .lock()
            .unwrap()
            .captured_env
            .clone()
            .unwrap_or_else(|| self.original_env.clone())
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

/// Resume in the agent session's directory when it's still inside the plane;
/// otherwise start in the root and say how to resume by hand.
fn plan_restore(entry: persist::Entry, link: Option<agents::AgentLink>) -> Restore {
    let mut restore = Restore {
        key: entry.key,
        plane_id: entry.plane_id,
        cwd: entry.plane_dir.clone(),
        plane_dir: entry.plane_dir,
        title: entry.title,
        input: None,
        notice: None,
    };
    let Some(link) = link else { return restore };
    if link.resumable_in(&restore.plane_dir) {
        restore.cwd = link.cwd.clone();
        restore.input = Some(link.resume_command());
    } else {
        restore.notice = Some(format!(
            "bitplane-cc: not resuming the agent session from {}, which is gone or outside this plane; it was: claude --resume {}",
            link.cwd.display(),
            link.session_id
        ));
    }
    restore
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

    fn entry(plane_dir: &Path) -> persist::Entry {
        persist::Entry {
            key: "k1".into(),
            plane_id: "alpha".into(),
            plane_dir: plane_dir.into(),
            title: Some("claude".into()),
            seq: 1,
        }
    }

    fn link(cwd: &Path) -> agents::AgentLink {
        agents::AgentLink {
            session_id: "abc-123".into(),
            cwd: cwd.into(),
            working: false,
        }
    }

    #[test]
    fn restore_resumes_in_the_agent_directory() {
        let dir = tempfile::tempdir().unwrap();
        let api = dir.path().join("api");
        std::fs::create_dir(&api).unwrap();

        let plain = plan_restore(entry(dir.path()), None);
        assert_eq!(plain.cwd, dir.path());
        assert_eq!((plain.input, plain.notice), (None, None));

        let resumed = plan_restore(entry(dir.path()), Some(link(&api)));
        assert_eq!(resumed.cwd, api);
        assert_eq!(resumed.input.as_deref(), Some("claude --resume abc-123\r"));
        assert_eq!(resumed.title.as_deref(), Some("claude"));
    }

    #[test]
    fn restore_without_the_agent_directory_starts_in_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        for cwd in [dir.path().join("gone"), outside.path().to_path_buf()] {
            let restore = plan_restore(entry(dir.path()), Some(link(&cwd)));
            assert_eq!(restore.cwd, dir.path());
            assert_eq!(restore.input, None);
            assert!(restore.notice.unwrap().contains("claude --resume abc-123"));
        }
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
