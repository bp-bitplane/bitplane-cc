//! The backend's state, owned in one place: the plane list, session tabs,
//! agents and creates mirrored from the daemon (ADR 0028), and the sidebar
//! model merged from them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::agents::{AgentEvent, AgentInfo, AgentKindInfo, Agents};
use crate::daemon::DaemonLink;
use crate::env::{self, EnvMap};
use crate::files;
use crate::git;
use crate::jobs::{CreateEvent, CreateInfo, CreateLogEvent, JobEmitters, Jobs};
use crate::migrate::{self, Legacy};
use crate::persist;
use crate::planes::{self, AppStatus, PlaneKind, PlaneRow, Planes};
use crate::sessions::{SessionConfig, SessionEvent, SessionInfo, Sessions};
use crate::workspace::{
    FileTabsScope, FilesScope, GitTarget, SourceControlScope, WindowScope, Workspace,
    WorkspaceEmitters, path_string,
};

/// How long `snapshot` waits for the daemon's sessions, planes and jobs, so
/// the UI state it restores finds its tabs and a running create shows.
const SESSIONS_READY: Duration = Duration::from_secs(3);

/// How long the legacy import waits for the daemon; the import runs again on
/// the next launch if it never answered.
const IMPORT_READY: Duration = Duration::from_secs(30);

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
    pub create_log: Arc<dyn Fn(CreateLogEvent) + Send + Sync>,
    pub workspace: WorkspaceEmitters,
}

pub struct App {
    original_env: EnvMap,
    data_dir: Option<DataDir>,
    daemon: Arc<DaemonLink>,
    pub sessions: Arc<Sessions>,
    pub agents: Arc<Agents>,
    pub planes: Arc<Planes>,
    pub jobs: Arc<Jobs>,
    pub workspace: Arc<Workspace>,
    state: Mutex<State>,
    remerge_lock: Mutex<()>,
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
    /// The login shell's environment, for the editor hand-off.
    captured_env: Option<EnvMap>,
    env_warning: Option<String>,
    model: Option<PlanesModel>,
}

impl App {
    /// Without a data dir nothing is saved.
    pub fn new(
        original_env: EnvMap,
        daemon: &Arc<DaemonLink>,
        data_dir: Option<DataDir>,
        emitters: Emitters,
    ) -> Arc<Self> {
        let shell = env::resolve_shell(original_env.get("SHELL").map(String::as_str));
        // Sessions, planes and jobs all change the sidebar, so they re-merge
        // it; they exist before the app, hence the late binding.
        let this: Arc<OnceLock<Weak<App>>> = Arc::new(OnceLock::new());
        let remerge = {
            let this = Arc::clone(&this);
            Arc::new(move || {
                if let Some(app) = this.get().and_then(Weak::upgrade) {
                    app.remerge();
                }
            })
        };

        let emit_session = emitters.session;
        let on_session = Arc::clone(&remerge);
        let sessions = Sessions::new(
            daemon,
            SessionConfig {
                shell_name: env::shell_name(&shell),
            },
            Arc::new(move |event| {
                emit_session(event);
                on_session();
            }),
        );

        let agents = Agents::new(daemon, emitters.agent, emitters.agent_kinds);
        let workspace = Workspace::new(daemon, emitters.workspace);
        let on_planes = Arc::clone(&remerge);
        let retarget = Arc::clone(&this);
        let planes = Planes::new(
            daemon,
            Arc::new(move || {
                on_planes();
                if let Some(app) = retarget.get().and_then(Weak::upgrade) {
                    app.retarget_all();
                }
            }),
        );
        let jobs = Jobs::new(
            daemon,
            JobEmitters {
                create: emitters.create,
                log: emitters.create_log,
                changed: remerge,
            },
        );

        let app = Arc::new(App {
            original_env,
            data_dir,
            daemon: Arc::clone(daemon),
            sessions,
            agents,
            planes,
            jobs,
            workspace,
            state: Mutex::new(State::default()),
            remerge_lock: Mutex::new(()),
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

    /// The plane list changed: every window's paths may have.
    fn retarget_all(self: &Arc<Self>) {
        let windows: Vec<String> = self.targets.lock().unwrap().keys().cloned().collect();
        for window in windows {
            self.retarget(&window);
        }
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

    /// Capture the login shell's environment for the editor hand-off.
    /// Needs a tokio runtime.
    pub async fn capture_env(&self) {
        let captured = env::capture(&self.original_env, env::CAPTURE_TIMEOUT).await;
        {
            let mut state = self.state.lock().unwrap();
            state.captured_env = Some(captured.vars);
            state.env_warning = captured.warning;
        }
        self.remerge();
    }

    /// Re-read the plane list (the sidebar's Refresh and every Retry); the
    /// shell environment is captured again alongside.
    pub async fn refresh(&self) -> Result<(), String> {
        let (refreshed, ()) = tokio::join!(self.planes.refresh(), self.capture_env());
        refreshed
    }

    /// Rebuild the sidebar model, emitting it if it changed.
    pub fn remerge(&self) {
        // Held through the emit, so a stale model can never land after a newer one.
        let _ordered = self.remerge_lock.lock().unwrap();
        let daemon_model = self.planes.model();
        let with_sessions: BTreeSet<String> = self
            .sessions
            .list()
            .into_iter()
            .map(|session| session.plane_id)
            .collect();
        let creating = self.jobs.creating();
        let capability = self.daemon.capability(planes::MODULE);
        let mut state = self.state.lock().unwrap();
        let model = PlanesModel {
            status: planes::status(capability.as_ref(), daemon_model.as_ref()),
            planes_dir: daemon_model.as_ref().map(|m| m.planes_dir.clone()),
            planes_dir_exists: daemon_model.as_ref().is_some_and(|m| m.planes_dir_exists),
            refreshed_at: daemon_model
                .as_ref()
                .and_then(|m| m.refreshed_at)
                .map(|at| at as f64),
            env_warning: state.env_warning.clone(),
            planes: daemon_model
                .as_ref()
                .map(|m| planes::rows(m, &creating, &with_sessions))
                .unwrap_or_default(),
        };
        if state.model.as_ref() == Some(&model) {
            return;
        }
        state.model = Some(model.clone());
        drop(state);
        (self.emit_planes)(model);
    }

    pub async fn snapshot(&self) -> Snapshot {
        tokio::join!(
            self.sessions.ready(SESSIONS_READY),
            self.planes.ready(SESSIONS_READY),
            self.jobs.ready(SESSIONS_READY),
        );
        self.remerge();
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
            creates: self.jobs.list(),
        }
    }

    /// A plane's root: its row's directory, else `<planes dir>/<id>` (a plane
    /// that was just created may not be listed yet).
    pub fn plane_dir(&self, plane_id: &str) -> Result<PathBuf, String> {
        let model = self
            .planes
            .model()
            .ok_or("the plane list hasn't loaded yet")?;
        Ok(model
            .planes
            .iter()
            .find(|row| row.id == plane_id)
            .map(|row| PathBuf::from(&row.directory))
            .unwrap_or_else(|| PathBuf::from(&model.planes_dir).join(plane_id)))
    }

    /// What Source control reads for a plane: its root and its Members'
    /// worktrees.
    pub fn source_control_target(&self, plane_id: &str) -> Result<(PathBuf, Vec<PathBuf>), String> {
        let shown = self
            .state
            .lock()
            .unwrap()
            .model
            .as_ref()
            .and_then(|m| m.planes.iter().find(|p| p.id == plane_id).map(|p| p.kind))
            .ok_or_else(|| format!("there's no plane called {plane_id}"))?;
        match shown {
            PlaneKind::Gone => return Err(format!("{plane_id} is gone")),
            PlaneKind::Creating => return Err(format!("{plane_id} is still being created")),
            PlaneKind::Plane | PlaneKind::Draft => {}
        }
        let model = self
            .planes
            .model()
            .ok_or("the plane list hasn't loaded yet")?;
        let row = model
            .planes
            .iter()
            .find(|row| row.id == plane_id)
            .ok_or_else(|| format!("there's no plane called {plane_id}"))?;
        let members = row.members.iter().map(|m| PathBuf::from(&m.path)).collect();
        Ok((PathBuf::from(&row.directory), members))
    }

    /// The environment the editor runs in: the captured one once it's known.
    pub fn tool_env(&self) -> EnvMap {
        self.state
            .lock()
            .unwrap()
            .captured_env
            .clone()
            .unwrap_or_else(|| self.original_env.clone())
    }
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
                create_log: Arc::new(|_| {}),
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
}
