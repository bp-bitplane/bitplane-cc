//! The backend's state, owned in one place: the plane list and what it's
//! built from, the session tabs and agents mirrored from the daemon (ADR 0028)
//! and the create registry.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::Notify;

use crate::agents::{AgentEvent, AgentInfo, AgentKindInfo, Agents};
use crate::bp::{self, Bp, BpError, VersionError};
use crate::create::{CreateEvent, CreateInfo, CreateStatus, Creates};
use crate::daemon::DaemonLink;
use crate::env::{self, CapturedEnv, EnvMap};
use crate::files;
use crate::git;
use crate::migrate::{self, Legacy};
use crate::persist;
use crate::planes::{self, PlaneKind, PlaneRow, Source, Watcher};
use crate::sessions::{SessionConfig, SessionEvent, SessionInfo, Sessions};
use crate::workspace::{
    FileTabsScope, FilesScope, GitTarget, SourceControlScope, WindowScope, Workspace,
    WorkspaceEmitters, path_string,
};

/// How long `snapshot` waits for the daemon's session list, so the UI state
/// it restores finds its tabs.
const SESSIONS_READY: Duration = Duration::from_secs(3);

/// How long the legacy import waits for the daemon; the import runs again on
/// the next launch if it never answered.
const IMPORT_READY: Duration = Duration::from_secs(30);

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
    pub agents: Vec<AgentInfo>,
    pub agent_kinds: Vec<AgentKindInfo>,
    pub creates: Vec<CreateInfo>,
}

/// Where the app keeps what outlives it.
#[derive(Debug, Clone)]
pub struct DataDir(pub PathBuf);

impl DataDir {
    pub fn ui_state(&self) -> PathBuf {
        self.0.join("ui-state.json")
    }

    /// What cc restored itself before the daemon owned sessions (ADR 0028).
    pub fn legacy(&self, home: Option<&Path>) -> Legacy {
        Legacy {
            sessions: self.0.join("sessions.json"),
            agents: self.0.join("agents"),
            claude_settings: home.map(|home| home.join(".claude").join("settings.json")),
        }
    }
}

pub type EmitPlanes = Arc<dyn Fn(PlanesModel) + Send + Sync>;

/// Where events go; the Tauri layer forwards them to the webview.
pub struct Emitters {
    pub planes: EmitPlanes,
    pub session: Arc<dyn Fn(SessionEvent) + Send + Sync>,
    pub agent: Arc<dyn Fn(AgentEvent) + Send + Sync>,
    pub agent_kinds: Arc<dyn Fn(Vec<AgentKindInfo>) + Send + Sync>,
    pub create: Arc<dyn Fn(CreateEvent) + Send + Sync>,
    pub workspace: WorkspaceEmitters,
}

pub struct App {
    original_env: EnvMap,
    data_dir: Option<DataDir>,
    pub sessions: Arc<Sessions>,
    pub agents: Arc<Agents>,
    pub creates: Arc<Creates>,
    pub workspace: Arc<Workspace>,
    state: Mutex<State>,
    watcher: Mutex<Option<Watcher>>,
    remerge_lock: Mutex<()>,
    /// Watcher events land here; the refresh loop merges them.
    refresh_needed: Arc<Notify>,
    emit_planes: EmitPlanes,
    /// What each window asked to watch, by window label; resolved again
    /// whenever the plane list changes.
    targets: Mutex<BTreeMap<String, WindowTargets>>,
    /// Held while a window's targets are resolved, so the latest always wins.
    retarget_lock: Mutex<()>,
}

/// One window's watch requests, as the frontend sent them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct WindowTargets {
    /// The plane Files is showing and its expanded folders.
    files: Option<(String, Vec<String>)>,
    git: Option<GitTarget>,
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
    /// Without a data dir nothing is saved.
    pub fn new(
        original_env: EnvMap,
        daemon: &DaemonLink,
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
            daemon,
            SessionConfig {
                shell_name: env::shell_name(&shell),
            },
            Arc::new(move |event| {
                emit_session(event);
                if let Some(app) = app.get().and_then(Weak::upgrade) {
                    app.remerge();
                }
            }),
        );

        let agents = Agents::new(daemon, emitters.agent, emitters.agent_kinds);
        let workspace = Workspace::new(daemon, emitters.workspace);

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
            sessions,
            agents,
            creates,
            workspace,
            state: Mutex::new(State::default()),
            watcher: Mutex::new(None),
            remerge_lock: Mutex::new(()),
            refresh_needed: Arc::new(Notify::new()),
            emit_planes: emitters.planes,
            targets: Mutex::default(),
            retarget_lock: Mutex::new(()),
        });
        let _ = this.set(Arc::downgrade(&app));
        app
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

    /// Import the tabs and agent links cc kept before the daemon, once the
    /// daemon's own sessions are mirrored.
    pub async fn import_legacy(&self) {
        let Some(dir) = &self.data_dir else { return };
        self.sessions.ready(IMPORT_READY).await;
        migrate::import(&dir.legacy(self.home().as_deref()), &self.sessions).await;
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

    /// Watch `plane_id`'s expanded folders for `window`'s Files, or stop with `None`.
    pub fn watch_files(self: &Arc<Self>, window: &str, target: Option<(String, Vec<String>)>) {
        self.targets
            .lock()
            .unwrap()
            .entry(window.into())
            .or_default()
            .files = target;
        self.retarget(window);
    }

    /// Watch what `target` shows in `window`: Source control and the open
    /// File tabs; `None` stops.
    pub fn watch_git(self: &Arc<Self>, window: &str, target: Option<GitTarget>) {
        self.targets
            .lock()
            .unwrap()
            .entry(window.into())
            .or_default()
            .git = target;
        self.retarget(window);
    }

    /// The window closed: drop everything it watched.
    pub fn close_window(&self, window: &str) {
        self.targets.lock().unwrap().remove(window);
        self.workspace.close_window(window);
    }

    /// Resolve `window`'s targets to paths off the calling thread: finding a
    /// plane's repos reads the disk.
    fn retarget(self: &Arc<Self>, window: &str) {
        let app = Arc::clone(self);
        let window = window.to_string();
        tauri::async_runtime::spawn_blocking(move || app.resolve_targets(&window));
    }

    /// The latest targets always win, as they're read under the lock.
    fn resolve_targets(&self, window: &str) {
        let _ordered = self.retarget_lock.lock().unwrap();
        let targets = self
            .targets
            .lock()
            .unwrap()
            .get(window)
            .cloned()
            .unwrap_or_default();
        let files = targets.files.and_then(|(plane_id, dirs)| {
            let (root, _) = self.source_control_target(&plane_id).ok()?;
            Some(files_scope(&plane_id, &root, &dirs))
        });
        let (source_control, file_tabs) = targets
            .git
            .and_then(|target| {
                let (root, members) = self.source_control_target(&target.plane_id).ok()?;
                let source_control = target.source_control.then(|| SourceControlScope {
                    plane_id: target.plane_id.clone(),
                    repos: git::repo_dirs(&root, &members),
                    root: root.clone(),
                });
                let file_tabs = (!target.file_tabs.is_empty())
                    .then(|| file_tabs_scope(&target.plane_id, &root, &target.file_tabs));
                Some((source_control, file_tabs))
            })
            .unwrap_or_default();
        self.workspace.set_scope(
            window,
            WindowScope {
                files,
                source_control,
                file_tabs,
            },
        );
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
        let windows: Vec<String> = self.targets.lock().unwrap().keys().cloned().collect();
        for window in windows {
            self.resolve_targets(&window);
        }
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

    pub async fn snapshot(&self) -> Snapshot {
        self.sessions.ready(SESSIONS_READY).await;
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
            agents: self.agents.list(),
            agent_kinds: self.agents.kinds(),
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

/// Files' expanded folders as daemon paths, skipping anything inside `.git`.
fn files_scope(plane_id: &str, root: &Path, dirs: &[String]) -> FilesScope {
    FilesScope {
        plane_id: plane_id.into(),
        dirs: dirs
            .iter()
            .filter(|dir| !files::in_git_dir(dir))
            .filter_map(|dir| {
                let path = files::plane_path(root, dir).ok()?;
                Some((path_string(&path), dir.clone()))
            })
            .collect(),
    }
}

/// What open File tabs need watched: each tab's folder, and the repo
/// holding it.
fn file_tabs_scope(plane_id: &str, root: &Path, tabs: &[String]) -> FileTabsScope {
    let mut scope = FileTabsScope {
        plane_id: plane_id.into(),
        ..FileTabsScope::default()
    };
    for relative in tabs.iter().filter(|path| !files::in_git_dir(path)) {
        let parent = Path::new(relative).parent().unwrap_or(Path::new(""));
        let Ok(folder) = files::plane_path(root, &parent.to_string_lossy()) else {
            continue;
        };
        if let Some(repo) = files::containing_repo(root, &folder).map(|repo| path_string(&repo))
            && !scope.repos.contains(&repo)
        {
            scope.repos.push(repo);
        }
        let folder = path_string(&folder);
        if !scope.folders.contains(&folder) {
            scope.folders.push(folder);
        }
    }
    scope
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

    fn quiet_app() -> Arc<App> {
        let daemon = DaemonLink::new(Err("no daemon".into()), PathBuf::new(), Arc::new(|_| {}));
        App::new(
            EnvMap::new(),
            &daemon,
            None,
            Emitters {
                planes: Arc::new(|_| {}),
                session: Arc::new(|_| {}),
                agent: Arc::new(|_| {}),
                agent_kinds: Arc::new(|_| {}),
                create: Arc::new(|_| {}),
                workspace: WorkspaceEmitters {
                    files: Arc::new(|_, _| {}),
                    source_control: Arc::new(|_, _| {}),
                    file_tabs: Arc::new(|_, _| {}),
                },
            },
        )
    }

    /// Sync commands run on the main thread, outside any Tokio runtime.
    #[test]
    fn watch_commands_run_outside_a_runtime() {
        let app = quiet_app();
        app.watch_git(
            "main",
            Some(GitTarget {
                plane_id: "alpha".into(),
                source_control: true,
                file_tabs: vec!["a.txt".into()],
            }),
        );
        app.watch_files("main", Some(("alpha".into(), vec![String::new()])));
        app.watch_git("main", None);
        app.watch_files("main", None);
        app.close_window("main");
    }

    #[test]
    fn file_tabs_watch_their_folders_and_repos() {
        let plane = tempfile::tempdir().unwrap();
        let root = plane.path();
        std::fs::create_dir_all(root.join("web/.git")).unwrap();
        std::fs::create_dir_all(root.join("web/src")).unwrap();
        let tabs = [
            "web/src/a.rs".to_string(),
            "web/src/b.rs".into(),
            "notes.md".into(),
            "web/.git/HEAD".into(),
        ];
        let scope = file_tabs_scope("p", root, &tabs);
        assert_eq!(
            scope.folders,
            [path_string(&root.join("web/src")), path_string(root)]
        );
        assert_eq!(scope.repos, [path_string(&root.join("web"))]);
    }

    #[test]
    fn files_skips_folders_inside_git() {
        let scope = files_scope(
            "p",
            Path::new("/planes/p"),
            &[
                "".into(),
                "web/src".into(),
                "web/.git/refs".into(),
                "../x".into(),
            ],
        );
        assert_eq!(
            scope.dirs,
            [
                ("/planes/p".to_string(), String::new()),
                ("/planes/p/web/src".into(), "web/src".into())
            ]
        );
    }

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
