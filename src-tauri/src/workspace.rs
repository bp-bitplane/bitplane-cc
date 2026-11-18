//! Files, Source control and File tabs over the daemon's fs and git modules
//! (sessiond ADR 0032): their reads, and every window's watches merged into
//! one fs and one git scope on cc's one connection (ADR 0028), with each
//! event routed back to the windows that asked for it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use bitplane_protocol::fs::{
    Blob, FsChanged, FsListResult, FsPathParams, FsReadParams, READ_LIMIT, WatchParams,
};
use bitplane_protocol::git::{GitRepo, GitRev, GitShowParams, GitStatusParams, GitStatusResult};
use bitplane_protocol::{ErrorCode, Frame, Notification, method};
use bitplane_sessiond::client::{ClientError, Handle};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

use crate::daemon::{DaemonLink, Subscriber};
use crate::diff::{self, DiffMode, FileContents, FileDiff, FileText, MAX_FILE_BYTES, Side};
use crate::files::{self, FileEntry};
use crate::git::{self, Repo};

/// `files_changed`: folders of a watched plane to list again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FilesEvent {
    pub plane_id: String,
    /// Relative to the plane root; `""` is the root.
    pub dirs: Vec<String>,
}

/// `source_control_changed`: a fresh read of a watched plane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SourceControlEvent {
    pub plane_id: String,
    pub repos: Vec<Repo>,
}

/// `file_tabs_changed`: the open File tabs of a watched plane may be stale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileTabsEvent {
    pub plane_id: String,
}

/// What a window has on screen that git changes can make stale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GitTarget {
    pub plane_id: String,
    pub source_control: bool,
    /// The plane's open File tabs, relative to its root.
    pub file_tabs: Vec<String>,
}

/// Emits an event to the window with this label.
pub type EmitTo<E> = Arc<dyn Fn(&str, E) + Send + Sync>;

/// Where events go: to the window that asked for them.
pub struct WorkspaceEmitters {
    pub files: EmitTo<FilesEvent>,
    pub source_control: EmitTo<SourceControlEvent>,
    pub file_tabs: EmitTo<FileTabsEvent>,
}

/// The folders Files shows in one plane: absolute path and the relative dir
/// the window knows it by.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilesScope {
    pub plane_id: String,
    pub dirs: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceControlScope {
    pub plane_id: String,
    pub root: PathBuf,
    pub repos: Vec<PathBuf>,
}

/// What a plane's open File tabs need watched: their folders, for edits, and
/// their repos, for staging and commits, which touch only the git dir.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileTabsScope {
    pub plane_id: String,
    pub folders: Vec<String>,
    pub repos: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowScope {
    pub files: Option<FilesScope>,
    pub source_control: Option<SourceControlScope>,
    pub file_tabs: Option<FileTabsScope>,
}

/// Where one daemon event goes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Route {
    Files {
        window: String,
        plane_id: String,
        dir: String,
    },
    SourceControl {
        window: String,
    },
    FileTabs {
        window: String,
        plane_id: String,
    },
}

/// Every window's scope, and the paths the daemon reported gone: those stay
/// out of the merged scopes until no window asks for them any more.
#[derive(Debug, Default)]
pub struct Scopes {
    windows: BTreeMap<String, WindowScope>,
    gone_fs: BTreeSet<String>,
    gone_git: BTreeSet<String>,
}

pub struct Workspace {
    me: Weak<Workspace>,
    emit: WorkspaceEmitters,
    state: Mutex<State>,
    /// Held while a watch call is out, so the scopes reach the daemon in order.
    sending: tokio::sync::Mutex<()>,
}

#[derive(Default)]
struct State {
    handle: Option<Handle>,
    scopes: Scopes,
    sent_fs: BTreeSet<String>,
    sent_git: BTreeSet<String>,
    /// The latest read of each watched repo, for whole-plane Source control events.
    repos: BTreeMap<String, GitRepo>,
}

/// A failed daemon call: the protocol code when the daemon gave one.
#[derive(Debug)]
struct Failure {
    code: Option<ErrorCode>,
    message: String,
}

impl Scopes {
    pub fn set(&mut self, window: &str, scope: WindowScope) {
        if scope == WindowScope::default() {
            self.windows.remove(window);
        } else {
            self.windows.insert(window.into(), scope);
        }
        self.forget_unwatched_gone();
    }

    pub fn get(&self, window: &str) -> WindowScope {
        self.windows.get(window).cloned().unwrap_or_default()
    }

    pub fn close(&mut self, window: &str) {
        self.set(window, WindowScope::default());
    }

    /// The fs scope: every window's Files folders and File tab folders.
    pub fn fs_paths(&self) -> BTreeSet<String> {
        let mut paths = self.wanted_fs();
        paths.retain(|path| !self.gone_fs.contains(path));
        paths
    }

    /// The git scope: every window's Source control repos and File tab repos.
    pub fn git_paths(&self) -> BTreeSet<String> {
        let mut paths = self.wanted_git();
        paths.retain(|path| !self.gone_git.contains(path));
        paths
    }

    pub fn fs_changed(&mut self, changed: &FsChanged) -> Vec<Route> {
        if changed.gone {
            self.gone_fs.insert(changed.path.clone());
        }
        let mut routes = BTreeSet::new();
        for (window, scope) in &self.windows {
            if let Some(files) = &scope.files {
                for (path, dir) in &files.dirs {
                    if *path == changed.path {
                        routes.insert(Route::Files {
                            window: window.clone(),
                            plane_id: files.plane_id.clone(),
                            dir: dir.clone(),
                        });
                    }
                }
            }
            if let Some(tabs) = &scope.file_tabs
                && tabs.folders.contains(&changed.path)
            {
                routes.insert(file_tabs_route(window, tabs));
            }
        }
        routes.into_iter().collect()
    }

    pub fn git_changed(&mut self, changed: &GitRepo) -> Vec<Route> {
        if changed.gone {
            self.gone_git.insert(changed.path.clone());
        }
        let mut routes = BTreeSet::new();
        for (window, scope) in &self.windows {
            if let Some(source_control) = &scope.source_control
                && source_control
                    .repos
                    .iter()
                    .any(|repo| path_string(repo) == changed.path)
            {
                routes.insert(Route::SourceControl {
                    window: window.clone(),
                });
            }
            if let Some(tabs) = &scope.file_tabs
                && tabs.repos.contains(&changed.path)
            {
                routes.insert(file_tabs_route(window, tabs));
            }
        }
        routes.into_iter().collect()
    }

    /// After a reconnect the daemon's scopes are empty, and what was gone
    /// may be back.
    pub fn reset_gone(&mut self) {
        self.gone_fs.clear();
        self.gone_git.clear();
    }

    fn wanted_fs(&self) -> BTreeSet<String> {
        self.windows
            .values()
            .flat_map(|scope| {
                let files = scope.files.iter().flat_map(|f| f.dirs.iter().map(|d| &d.0));
                let tabs = scope.file_tabs.iter().flat_map(|t| &t.folders);
                files.chain(tabs).cloned()
            })
            .collect()
    }

    fn wanted_git(&self) -> BTreeSet<String> {
        self.windows
            .values()
            .flat_map(|scope| {
                let repos = scope
                    .source_control
                    .iter()
                    .flat_map(|s| s.repos.iter().map(|repo| path_string(repo)));
                let tabs = scope.file_tabs.iter().flat_map(|t| t.repos.iter().cloned());
                repos.chain(tabs)
            })
            .collect()
    }

    fn forget_unwatched_gone(&mut self) {
        let (fs, git) = (self.wanted_fs(), self.wanted_git());
        self.gone_fs.retain(|path| fs.contains(path));
        self.gone_git.retain(|path| git.contains(path));
    }
}

impl Workspace {
    /// Follows `link` from its next connection on.
    pub fn new(link: &DaemonLink, emit: WorkspaceEmitters) -> Arc<Self> {
        let workspace = Arc::new_cyclic(|me| Workspace {
            me: me.clone(),
            emit,
            state: Mutex::default(),
            sending: tokio::sync::Mutex::new(()),
        });
        link.subscribe(Arc::new(Subscription(Arc::downgrade(&workspace))));
        workspace
    }

    /// Replace what `window` watches; the merged scopes follow.
    pub fn set_scope(&self, window: &str, scope: WindowScope) {
        let changed = {
            let mut state = self.state.lock().unwrap();
            if state.scopes.get(window) == scope {
                false
            } else {
                state.scopes.set(window, scope);
                true
            }
        };
        if changed {
            self.sync();
        }
    }

    /// The window closed: nothing it watched is needed for it any more.
    pub fn close_window(&self, window: &str) {
        self.set_scope(window, WindowScope::default());
    }

    /// One folder of a plane for Files.
    pub async fn list_dir(&self, root: &Path, relative: &str) -> Result<Vec<FileEntry>, String> {
        let path = files::plane_path(root, relative)?;
        let listed: FsListResult = self
            .call(
                method::FS_LIST,
                FsPathParams {
                    path: path_string(&path),
                },
            )
            .await
            .map_err(|failure| failure.describe(relative))?;
        Ok(listed.entries.into_iter().map(FileEntry::from).collect())
    }

    /// Every repo of a plane with its status, also kept for the events
    /// that follow.
    pub async fn source_control(
        &self,
        root: &Path,
        members: &[PathBuf],
    ) -> Result<Vec<Repo>, String> {
        if !root.is_dir() {
            return Err(format!("{} no longer exists", root.display()));
        }
        let dirs = git::repo_dirs(root, members);
        let params = GitStatusParams {
            paths: dirs.iter().map(|dir| path_string(dir)).collect(),
        };
        let read: GitStatusResult = self
            .call(method::GIT_STATUS, params)
            .await
            .map_err(|failure| failure.message)?;
        {
            let mut state = self.state.lock().unwrap();
            let watched = state.sent_git.clone();
            for repo in &read.repos {
                if watched.contains(&repo.path) {
                    state.repos.insert(repo.path.clone(), repo.clone());
                }
            }
        }
        Ok(dirs
            .iter()
            .zip(read.repos)
            .map(|(dir, repo)| git::repo(root, dir, repo))
            .collect())
    }

    /// A file's contents for a File tab with no diff to show.
    pub async fn read_file(&self, root: &Path, relative: &str) -> Result<FileText, String> {
        let path = files::plane_path(root, relative)?;
        match self.read(&path, MAX_FILE_BYTES).await {
            Ok(Side::Missing) => Err(format!("{relative} no longer exists")),
            Ok(side) => Ok(diff::file_text(&side)),
            Err(failure) => Err(failure.describe(relative)),
        }
    }

    /// What a File tab of `relative` in `mode` shows. `force` diffs past the
    /// size limit ("Show anyway").
    pub async fn file_diff(
        &self,
        root: &Path,
        relative: &str,
        mode: DiffMode,
        force: bool,
    ) -> Result<FileDiff, String> {
        let limit = if force { READ_LIMIT } else { MAX_FILE_BYTES };
        let target = self.locate(root, relative).await?;
        let Some(repo) = &target.repo else {
            return self.plain(&target.path, relative).await;
        };
        let unchanged = match mode {
            DiffMode::File => FileDiff::Plain,
            DiffMode::Index | DiffMode::Worktree => FileDiff::None,
        };
        let Some(change) = repo.change(mode) else {
            return match mode {
                DiffMode::File => self.plain(&target.path, relative).await,
                DiffMode::Index | DiffMode::Worktree => Ok(FileDiff::None),
            };
        };
        let (old, new) = self.sides(&target, repo, &change, mode, limit).await?;
        if old == Side::Missing && new == Side::Missing {
            return Err(format!("{relative} no longer exists"));
        }
        Ok(diff::diff(
            &change.old_path,
            &repo.path,
            &old,
            &new,
            unchanged,
        ))
    }

    /// Both sides of a diff, so unchanged context can be expanded; a side
    /// that doesn't exist is empty.
    pub async fn file_contents(
        &self,
        root: &Path,
        relative: &str,
        mode: DiffMode,
    ) -> Result<FileContents, String> {
        let target = self.locate(root, relative).await?;
        let Some(repo) = &target.repo else {
            let new = self
                .read(&target.path, READ_LIMIT)
                .await
                .map_err(|failure| failure.describe(relative))?;
            return Ok(FileContents {
                old: String::new(),
                new: new.text(),
            });
        };
        let change = repo.change(mode).unwrap_or_else(|| Change {
            old_path: repo.path.clone(),
            untracked: false,
        });
        let (old, new) = self.sides(&target, repo, &change, mode, READ_LIMIT).await?;
        Ok(FileContents {
            old: old.text(),
            new: new.text(),
        })
    }

    fn connected(&self, handle: Handle) {
        {
            let mut state = self.state.lock().unwrap();
            state.handle = Some(handle);
            state.sent_fs.clear();
            state.sent_git.clear();
            state.repos.clear();
            state.scopes.reset_gone();
        }
        self.sync();
    }

    fn event(&self, event: Notification) {
        match event.method.as_str() {
            method::FS_CHANGED => {
                let Some(changed) = params::<FsChanged>(event.params) else {
                    return;
                };
                let routes = {
                    let mut state = self.state.lock().unwrap();
                    if changed.gone {
                        state.sent_fs.remove(&changed.path);
                    }
                    state.scopes.fs_changed(&changed)
                };
                self.deliver(routes);
            }
            method::GIT_CHANGED => {
                let Some(changed) = params::<GitRepo>(event.params) else {
                    return;
                };
                let routes = {
                    let mut state = self.state.lock().unwrap();
                    if changed.gone {
                        state.sent_git.remove(&changed.path);
                    }
                    state.repos.insert(changed.path.clone(), changed.clone());
                    state.scopes.git_changed(&changed)
                };
                self.deliver(routes);
            }
            _ => {}
        }
    }

    fn deliver(&self, routes: Vec<Route>) {
        let mut files: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
        for route in routes {
            match route {
                Route::Files {
                    window,
                    plane_id,
                    dir,
                } => files.entry((window, plane_id)).or_default().push(dir),
                Route::FileTabs { window, plane_id } => {
                    (self.emit.file_tabs)(&window, FileTabsEvent { plane_id });
                }
                Route::SourceControl { window } => {
                    if let Some(event) = self.source_control_event(&window) {
                        (self.emit.source_control)(&window, event);
                    }
                }
            }
        }
        for ((window, plane_id), dirs) in files {
            (self.emit.files)(&window, FilesEvent { plane_id, dirs });
        }
    }

    /// The window's whole Source control from the latest reads, once every
    /// repo has one.
    fn source_control_event(&self, window: &str) -> Option<SourceControlEvent> {
        let state = self.state.lock().unwrap();
        let scope = state.scopes.get(window).source_control?;
        let repos = scope
            .repos
            .iter()
            .map(|dir| {
                let read = state.repos.get(&path_string(dir))?.clone();
                Some(git::repo(&scope.root, dir, read))
            })
            .collect::<Option<Vec<Repo>>>()?;
        Some(SourceControlEvent {
            plane_id: scope.plane_id,
            repos,
        })
    }

    /// Send whichever merged scope differs from what the daemon holds; the
    /// latest scopes win, as they're read once the previous call is done.
    fn sync(&self) {
        let me = self.me.clone();
        tauri::async_runtime::spawn(async move {
            let Some(workspace) = me.upgrade() else {
                return;
            };
            let _sending = workspace.sending.lock().await;
            for (module, watch) in [
                (Module::Fs, method::FS_WATCH),
                (Module::Git, method::GIT_WATCH),
            ] {
                let (handle, paths) = {
                    let mut state = workspace.state.lock().unwrap();
                    let paths = match module {
                        Module::Fs => state.scopes.fs_paths(),
                        Module::Git => state.scopes.git_paths(),
                    };
                    let sent = match module {
                        Module::Fs => &state.sent_fs,
                        Module::Git => &state.sent_git,
                    };
                    if *sent == paths {
                        continue;
                    }
                    if module == Module::Git {
                        state.repos.retain(|path, _| paths.contains(path));
                    }
                    (state.handle.clone(), paths)
                };
                let Some(handle) = handle else { return };
                let params = WatchParams {
                    paths: paths.iter().cloned().collect(),
                };
                if call::<Value>(&handle, watch, params).await.is_ok() {
                    let mut state = workspace.state.lock().unwrap();
                    match module {
                        Module::Fs => state.sent_fs = paths,
                        Module::Git => state.sent_git = paths,
                    }
                }
            }
        });
    }

    async fn locate(&self, root: &Path, relative: &str) -> Result<Target, String> {
        let path = files::plane_path(root, relative)?;
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return Err(format!("{relative} isn't a file"));
        };
        if relative.is_empty() {
            return Err(format!("{relative} isn't a file"));
        }
        let repo = if files::in_git_dir(relative) {
            None
        } else {
            files::containing_repo(root, parent)
        };
        let Some(repo_dir) = repo else {
            return Ok(Target { path, repo: None });
        };
        let listed = self.list_dir(root, &relative_parent(relative)).await?;
        let name = name.to_string_lossy();
        if listed
            .iter()
            .any(|entry| entry.name == name && entry.ignored)
        {
            return Ok(Target { path, repo: None });
        }
        let inside = path
            .strip_prefix(&repo_dir)
            .map_err(|_| format!("{relative} isn't inside its repo"))?
            .to_string_lossy()
            .into_owned();
        let read: GitStatusResult = self
            .call(
                method::GIT_STATUS,
                GitStatusParams {
                    paths: vec![path_string(&repo_dir)],
                },
            )
            .await
            .map_err(|failure| failure.message)?;
        let Some(read) = read.repos.into_iter().next() else {
            return Err("the daemon reported no status".into());
        };
        if let Some(error) = read.error {
            return Err(error);
        }
        let Some(status) = read.status else {
            return Ok(Target { path, repo: None });
        };
        Ok(Target {
            path,
            repo: Some(RepoFile {
                dir: path_string(&repo_dir),
                path: inside,
                status,
            }),
        })
    }

    async fn sides(
        &self,
        target: &Target,
        repo: &RepoFile,
        change: &Change,
        mode: DiffMode,
        limit: u64,
    ) -> Result<(Side, Side), String> {
        let describe = |failure: Failure| failure.describe(&repo.path);
        let disk = || self.read(&target.path, limit);
        if change.untracked {
            return Ok((Side::Missing, disk().await.map_err(describe)?));
        }
        let head = || self.show(&repo.dir, &change.old_path, GitRev::Head, limit);
        let index = |path| self.show(&repo.dir, path, GitRev::Index, limit);
        let (old, new) = match mode {
            DiffMode::Index => (head().await, index(&repo.path).await),
            DiffMode::Worktree => (index(&repo.path).await, disk().await),
            DiffMode::File => (head().await, disk().await),
        };
        Ok((old.map_err(describe)?, new.map_err(describe)?))
    }

    async fn plain(&self, path: &Path, relative: &str) -> Result<FileDiff, String> {
        match self.read(path, 0).await {
            Ok(Side::Missing) => Err(format!("{relative} no longer exists")),
            Ok(_) => Ok(FileDiff::Plain),
            Err(failure) => Err(failure.describe(relative)),
        }
    }

    async fn read(&self, path: &Path, limit: u64) -> Result<Side, Failure> {
        let params = FsReadParams {
            path: path_string(path),
            max_bytes: Some(limit),
        };
        side(self.call(method::FS_READ, params).await)
    }

    async fn show(&self, repo: &str, path: &str, rev: GitRev, limit: u64) -> Result<Side, Failure> {
        let params = GitShowParams {
            repo: repo.into(),
            path: path.into(),
            rev,
            max_bytes: Some(limit),
        };
        side(self.call(method::GIT_SHOW, params).await)
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        params: impl Serialize,
    ) -> Result<T, Failure> {
        let handle = self.state.lock().unwrap().handle.clone();
        let Some(handle) = handle else {
            return Err(Failure {
                code: None,
                message: "cc isn't connected to the daemon".into(),
            });
        };
        call(&handle, method, params).await
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Module {
    Fs,
    Git,
}

struct Target {
    /// The file on disk; it may not exist when it was deleted.
    path: PathBuf,
    repo: Option<RepoFile>,
}

struct RepoFile {
    dir: String,
    /// Relative to `dir`, as git names it.
    path: String,
    status: bitplane_protocol::git::GitStatus,
}

/// What `mode` compares for a file with a change in it.
struct Change {
    /// Where the old side lives: a staged rename's source.
    old_path: String,
    untracked: bool,
}

impl RepoFile {
    /// `None` when nothing differs in `mode`.
    fn change(&self, mode: DiffMode) -> Option<Change> {
        let untracked = self.status.untracked.iter().any(|entry| {
            *entry == self.path || (entry.ends_with('/') && self.path.starts_with(entry.as_str()))
        });
        if untracked {
            return (mode != DiffMode::Index).then(|| Change {
                old_path: self.path.clone(),
                untracked: true,
            });
        }
        let staged = self.status.staged.iter().find(|c| c.path == self.path);
        let unstaged = self.status.unstaged.iter().any(|c| c.path == self.path);
        let changed = match mode {
            DiffMode::Index => staged.is_some(),
            DiffMode::Worktree => unstaged,
            DiffMode::File => staged.is_some() || unstaged,
        };
        let renamed_from = staged.and_then(|c| c.from.clone());
        changed.then(|| Change {
            old_path: match mode {
                DiffMode::Worktree => self.path.clone(),
                DiffMode::Index | DiffMode::File => {
                    renamed_from.unwrap_or_else(|| self.path.clone())
                }
            },
            untracked: false,
        })
    }
}

impl Failure {
    /// `NOT_FOUND` names `relative`; anything else is the daemon's message.
    fn describe(self, relative: &str) -> String {
        match self.code {
            Some(ErrorCode::NotFound) => format!("{relative} no longer exists"),
            _ => self.message,
        }
    }
}

/// What `Workspace` subscribes with; weak, so the link doesn't keep it alive.
struct Subscription(Weak<Workspace>);

impl Subscriber for Subscription {
    fn connected(&self, handle: Handle) {
        if let Some(workspace) = self.0.upgrade() {
            workspace.connected(handle);
        }
    }

    fn event(&self, event: Notification) {
        if let Some(workspace) = self.0.upgrade() {
            workspace.event(event);
        }
    }

    fn frame(&self, _frame: Frame) {}
}

fn file_tabs_route(window: &str, tabs: &FileTabsScope) -> Route {
    Route::FileTabs {
        window: window.into(),
        plane_id: tabs.plane_id.clone(),
    }
}

/// A blob as a diff side: `NOT_FOUND` is a side that doesn't exist.
fn side(read: Result<Blob, Failure>) -> Result<Side, Failure> {
    match read {
        Ok(Blob {
            data: Some(data), ..
        }) => BASE64
            .decode(data)
            .map(Side::Bytes)
            .map_err(|error| Failure {
                code: None,
                message: format!("the daemon sent an unreadable blob: {error}"),
            }),
        Ok(Blob { size, data: None }) => Ok(Side::TooLarge { size }),
        Err(Failure {
            code: Some(ErrorCode::NotFound),
            ..
        }) => Ok(Side::Missing),
        Err(failure) => Err(failure),
    }
}

fn relative_parent(relative: &str) -> String {
    Path::new(relative)
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

async fn call<T: DeserializeOwned>(
    handle: &Handle,
    method: &str,
    params: impl Serialize,
) -> Result<T, Failure> {
    let params = serde_json::to_value(params).expect("protocol types serialize");
    match handle.call(method, Some(params)).await {
        Ok(value) => serde_json::from_value(value).map_err(|error| Failure {
            code: None,
            message: format!("unexpected reply to {method}: {error}"),
        }),
        Err(ClientError::Rpc(error)) => Err(Failure {
            code: error.protocol_code(),
            message: error.message,
        }),
        Err(error) => Err(Failure {
            code: None,
            message: error.to_string(),
        }),
    }
}

fn params<T: DeserializeOwned>(params: Option<Value>) -> Option<T> {
    serde_json::from_value(params?).ok()
}

#[cfg(test)]
mod tests {
    use bitplane_protocol::git::{GitChange, GitChangeKind, GitHead, GitStatus};

    use super::*;

    fn files(plane_id: &str, dirs: &[(&str, &str)]) -> Option<FilesScope> {
        Some(FilesScope {
            plane_id: plane_id.into(),
            dirs: dirs
                .iter()
                .map(|(path, dir)| (path.to_string(), dir.to_string()))
                .collect(),
        })
    }

    fn source_control(plane_id: &str, repos: &[&str]) -> Option<SourceControlScope> {
        Some(SourceControlScope {
            plane_id: plane_id.into(),
            root: PathBuf::from(format!("/planes/{plane_id}")),
            repos: repos.iter().map(PathBuf::from).collect(),
        })
    }

    fn fs_changed(path: &str, gone: bool) -> FsChanged {
        FsChanged {
            path: path.into(),
            gone,
        }
    }

    fn git_repo(path: &str, gone: bool) -> GitRepo {
        GitRepo {
            path: path.into(),
            status: None,
            gone,
            error: None,
        }
    }

    fn set<const N: usize>(paths: [&str; N]) -> BTreeSet<String> {
        paths.iter().map(|path| path.to_string()).collect()
    }

    #[test]
    fn merges_every_windows_paths_into_one_scope_per_module() {
        let mut scopes = Scopes::default();
        scopes.set(
            "main",
            WindowScope {
                files: files("a", &[("/planes/a", ""), ("/planes/a/src", "src")]),
                source_control: source_control("a", &["/planes/a/web"]),
                file_tabs: Some(FileTabsScope {
                    plane_id: "a".into(),
                    folders: vec!["/planes/a/web/src".into()],
                    repos: vec!["/planes/a/web".into()],
                }),
            },
        );
        scopes.set(
            "other",
            WindowScope {
                files: files("b", &[("/planes/b", "")]),
                ..WindowScope::default()
            },
        );
        assert_eq!(
            scopes.fs_paths(),
            set([
                "/planes/a",
                "/planes/a/src",
                "/planes/a/web/src",
                "/planes/b"
            ])
        );
        assert_eq!(scopes.git_paths(), set(["/planes/a/web"]));

        scopes.close("main");
        assert_eq!(scopes.fs_paths(), set(["/planes/b"]));
        assert!(scopes.git_paths().is_empty());
    }

    #[test]
    fn routes_each_event_only_to_the_windows_that_asked() {
        let mut scopes = Scopes::default();
        scopes.set(
            "one",
            WindowScope {
                files: files("a", &[("/planes/a/src", "src")]),
                source_control: source_control("a", &["/planes/a/web"]),
                ..WindowScope::default()
            },
        );
        scopes.set(
            "two",
            WindowScope {
                files: files("b", &[("/planes/b", "")]),
                file_tabs: Some(FileTabsScope {
                    plane_id: "b".into(),
                    folders: vec!["/planes/b".into()],
                    repos: vec![],
                }),
                ..WindowScope::default()
            },
        );
        assert_eq!(
            scopes.fs_changed(&fs_changed("/planes/a/src", false)),
            [Route::Files {
                window: "one".into(),
                plane_id: "a".into(),
                dir: "src".into()
            }]
        );
        assert_eq!(
            scopes.fs_changed(&fs_changed("/planes/b", false)),
            [
                Route::Files {
                    window: "two".into(),
                    plane_id: "b".into(),
                    dir: String::new()
                },
                Route::FileTabs {
                    window: "two".into(),
                    plane_id: "b".into()
                }
            ]
        );
        assert_eq!(
            scopes.git_changed(&git_repo("/planes/a/web", false)),
            [Route::SourceControl {
                window: "one".into()
            }]
        );
        assert!(
            scopes
                .fs_changed(&fs_changed("/elsewhere", false))
                .is_empty()
        );
    }

    #[test]
    fn a_gone_path_leaves_the_scope_until_no_window_wants_it() {
        let mut scopes = Scopes::default();
        let watching = WindowScope {
            files: files("a", &[("/planes/a", ""), ("/planes/a/old", "old")]),
            source_control: source_control("a", &["/planes/a/web"]),
            ..WindowScope::default()
        };
        scopes.set("main", watching.clone());

        let routes = scopes.fs_changed(&fs_changed("/planes/a/old", true));
        scopes.git_changed(&git_repo("/planes/a/web", true));

        assert_eq!(
            routes.len(),
            1,
            "the window re-lists the vanished folder once"
        );
        assert_eq!(scopes.fs_paths(), set(["/planes/a"]));
        assert!(scopes.git_paths().is_empty());

        scopes.set(
            "main",
            WindowScope {
                files: files("a", &[("/planes/a", "")]),
                ..WindowScope::default()
            },
        );
        scopes.set("main", watching);
        assert_eq!(scopes.fs_paths(), set(["/planes/a", "/planes/a/old"]));
        assert_eq!(scopes.git_paths(), set(["/planes/a/web"]));
    }

    fn repo_file(status: GitStatus) -> RepoFile {
        RepoFile {
            dir: "/r".into(),
            path: "new.txt".into(),
            status,
        }
    }

    fn status() -> GitStatus {
        GitStatus {
            head: GitHead::Branch {
                name: "main".into(),
            },
            tracking: None,
            staged: vec![],
            unstaged: vec![],
            untracked: vec![],
        }
    }

    #[test]
    fn a_staged_rename_diffs_against_its_old_path() {
        let file = repo_file(GitStatus {
            staged: vec![GitChange {
                path: "new.txt".into(),
                kind: GitChangeKind::Renamed,
                from: Some("old.txt".into()),
            }],
            ..status()
        });
        assert_eq!(file.change(DiffMode::Index).unwrap().old_path, "old.txt");
        assert_eq!(file.change(DiffMode::File).unwrap().old_path, "old.txt");
        assert!(file.change(DiffMode::Worktree).is_none());
    }

    #[test]
    fn an_untracked_file_has_no_index_change() {
        let file = repo_file(GitStatus {
            untracked: vec!["new.txt".into()],
            ..status()
        });
        assert!(file.change(DiffMode::Index).is_none());
        assert!(file.change(DiffMode::Worktree).unwrap().untracked);
        let in_folder = RepoFile {
            path: "dir/a.txt".into(),
            ..repo_file(GitStatus {
                untracked: vec!["dir/".into()],
                ..status()
            })
        };
        assert!(in_folder.change(DiffMode::File).unwrap().untracked);
    }

    #[test]
    fn an_unchanged_file_has_nothing_to_compare() {
        let file = repo_file(status());
        for mode in [DiffMode::Index, DiffMode::Worktree, DiffMode::File] {
            assert!(file.change(mode).is_none());
        }
    }
}
