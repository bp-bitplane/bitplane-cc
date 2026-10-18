//! Watching the shown plane's repos so Source control and open File tabs
//! stay current.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::files;
use crate::git::Repo;
use crate::planes::DEBOUNCE;

/// Folders whose churn never changes what Source control shows.
pub const IGNORED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".venv",
    "__pycache__",
    ".next",
    ".turbo",
    ".cache",
];

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

/// What the frontend has on screen that git changes can make stale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GitTarget {
    pub plane_id: String,
    pub source_control: bool,
    /// The plane's open File tabs, relative to its root.
    pub file_tabs: Vec<String>,
}

/// What is watched: each worktree recursively, each git dir and each folder
/// of an outside-repo File tab on its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    pub worktrees: Vec<PathBuf>,
    pub git_dirs: Vec<PathBuf>,
    pub folders: Vec<PathBuf>,
}

impl Scope {
    /// The existing repos among `dirs`, canonicalized to match event paths.
    pub fn of(dirs: &[PathBuf]) -> Scope {
        let mut scope = Scope::default();
        for worktree in dirs.iter().filter_map(|dir| dir.canonicalize().ok()) {
            if scope.worktrees.contains(&worktree) {
                continue;
            }
            for git_dir in git_dirs(&worktree) {
                if !scope.git_dirs.contains(&git_dir) {
                    scope.git_dirs.push(git_dir);
                }
            }
            scope.worktrees.push(worktree);
        }
        scope
    }

    /// `repos`, plus whatever holds each File tab: its repo, or for a file
    /// outside any repo, its folder.
    pub fn with_file_tabs(root: &Path, repos: &[PathBuf], file_tabs: &[String]) -> Scope {
        let mut dirs = repos.to_vec();
        let mut folders = Vec::new();
        for relative in file_tabs.iter().filter(|path| !files::in_git_dir(path)) {
            let parent = Path::new(relative).parent().unwrap_or(Path::new(""));
            let Ok(dir) = files::resolve(root, &parent.to_string_lossy()) else {
                continue;
            };
            match files::containing_repo(root, &dir) {
                Some(repo) => dirs.push(repo),
                None if !folders.contains(&dir) => folders.push(dir),
                None => {}
            }
        }
        Scope {
            folders,
            ..Scope::of(&dirs)
        }
    }

    /// Whether a change at `path` can change a repo's status.
    pub fn is_relevant(&self, path: &Path) -> bool {
        let in_git_dir = path
            .parent()
            .is_some_and(|parent| self.git_dirs.iter().any(|dir| dir == parent));
        if in_git_dir {
            return !path.extension().is_some_and(|ext| ext == "lock");
        }
        if path
            .parent()
            .is_some_and(|parent| self.folders.iter().any(|dir| dir == parent))
        {
            return true;
        }
        self.worktrees.iter().any(|worktree| {
            path.strip_prefix(worktree).is_ok_and(|relative| {
                !relative
                    .components()
                    .any(|c| IGNORED_DIRS.iter().any(|ignored| c.as_os_str() == *ignored))
            })
        })
    }
}

/// A debounced watcher over one plane's repos.
pub struct GitWatch {
    debouncer: Debouncer<RecommendedWatcher>,
    scope: Arc<Mutex<Scope>>,
}

impl GitWatch {
    /// `on_change` runs on the debouncer's thread after a batch holding at
    /// least one relevant event.
    pub fn new(on_change: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        let scope = Arc::new(Mutex::new(Scope::default()));
        let filter = Arc::clone(&scope);
        let debouncer = new_debouncer(DEBOUNCE, move |result: DebounceEventResult| {
            let Ok(events) = result else { return };
            let scope = filter.lock().unwrap();
            if events.iter().any(|event| scope.is_relevant(&event.path)) {
                drop(scope);
                on_change();
            }
        })
        .map_err(|e| e.to_string())?;
        Ok(GitWatch { debouncer, scope })
    }

    /// Watch exactly `scope`; an empty scope stops watching.
    pub fn set_scope(&mut self, scope: Scope) {
        let mut current = self.scope.lock().unwrap();
        if *current == scope {
            return;
        }
        let watcher = self.debouncer.watcher();
        for path in current
            .worktrees
            .iter()
            .chain(&current.git_dirs)
            .chain(&current.folders)
        {
            let _ = watcher.unwatch(path);
        }
        for worktree in &scope.worktrees {
            let _ = watcher.watch(worktree, RecursiveMode::Recursive);
        }
        for dir in scope.git_dirs.iter().chain(&scope.folders) {
            let _ = watcher.watch(dir, RecursiveMode::NonRecursive);
        }
        *current = scope;
    }
}

/// The worktree's git dir (holding `HEAD` and `index`) and, for a linked
/// worktree, the shared one (holding refs).
fn git_dirs(worktree: &Path) -> Vec<PathBuf> {
    let dot_git = worktree.join(".git");
    let git_dir = if dot_git.is_dir() {
        dot_git
    } else {
        let Some(pointer) = std::fs::read_to_string(&dot_git).ok().and_then(|text| {
            text.strip_prefix("gitdir:")
                .map(|path| worktree.join(path.trim()))
        }) else {
            return Vec::new();
        };
        pointer
    };
    let common = std::fs::read_to_string(git_dir.join("commondir"))
        .ok()
        .map(|path| git_dir.join(path.trim()));
    std::iter::once(git_dir)
        .chain(common)
        .filter_map(|dir| dir.canonicalize().ok())
        .fold(Vec::new(), |mut dirs, dir| {
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
            dirs
        })
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    const SETTLE: Duration = Duration::from_millis(700);
    const WAIT: Duration = Duration::from_secs(3);

    fn git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}: {output:?}");
    }

    fn write(path: PathBuf, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    fn repo(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "-q", "-b", "main"]);
        write(dir.join("a.txt"), "a\n");
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "first"]);
    }

    #[test]
    fn resolves_a_linked_worktree_to_its_own_and_the_shared_git_dir() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        repo(&source);
        git(
            &source,
            &["worktree", "add", "-q", "../linked", "-b", "other"],
        );
        let scope = Scope::of(&[root.path().join("linked"), root.path().join("gone")]);
        let source = source.canonicalize().unwrap();
        assert_eq!(
            scope.worktrees,
            [root.path().join("linked").canonicalize().unwrap()]
        );
        assert_eq!(
            scope.git_dirs,
            [source.join(".git/worktrees/linked"), source.join(".git")]
        );
    }

    #[test]
    fn ignores_build_folders_and_git_internals() {
        let scope = Scope {
            worktrees: vec![PathBuf::from("/p/web")],
            git_dirs: vec![PathBuf::from("/p/web/.git")],
            folders: vec![PathBuf::from("/p")],
        };
        assert!(scope.is_relevant(Path::new("/p/plane.toml")));
        assert!(!scope.is_relevant(Path::new("/p/.bitplane/state")));
        assert!(scope.is_relevant(Path::new("/p/web/src/a.rs")));
        assert!(scope.is_relevant(Path::new("/p/web/.git/index")));
        assert!(scope.is_relevant(Path::new("/p/web/.git/HEAD")));
        assert!(!scope.is_relevant(Path::new("/p/web/.git/index.lock")));
        assert!(!scope.is_relevant(Path::new("/p/web/.git/objects/ab/cdef")));
        assert!(!scope.is_relevant(Path::new("/p/web/node_modules/x/index.js")));
        assert!(!scope.is_relevant(Path::new("/p/web/target/debug/app")));
        assert!(!scope.is_relevant(Path::new("/p/other/a.rs")));
    }

    #[test]
    fn ignored_names_only_count_inside_the_worktree() {
        let scope = Scope {
            worktrees: vec![PathBuf::from("/target/web")],
            ..Scope::default()
        };
        assert!(scope.is_relevant(Path::new("/target/web/a.rs")));
    }

    struct Watched {
        _root: tempfile::TempDir,
        repo: PathBuf,
        changes: mpsc::Receiver<()>,
        _watch: GitWatch,
    }

    impl Watched {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let repo_dir = root.path().join("web");
            repo(&repo_dir);
            let (sender, changes) = mpsc::channel();
            let sender = Mutex::new(sender);
            let mut watch = GitWatch::new(Arc::new(move || {
                let _ = sender.lock().unwrap().send(());
            }))
            .unwrap();
            watch.set_scope(Scope::of(std::slice::from_ref(&repo_dir)));
            let watched = Watched {
                _root: root,
                repo: repo_dir,
                changes,
                _watch: watch,
            };
            watched.settle();
            watched
        }

        /// Wait out events from before the watch began.
        fn settle(&self) {
            std::thread::sleep(SETTLE);
            while self.changes.try_recv().is_ok() {}
        }

        fn changed(&self) -> bool {
            let changed = self.changes.recv_timeout(WAIT).is_ok();
            self.settle();
            changed
        }
    }

    #[test]
    fn an_edit_a_stage_and_a_branch_switch_each_trigger() {
        let watched = Watched::new();
        write(watched.repo.join("a.txt"), "changed\n");
        assert!(watched.changed(), "edit");
        git(&watched.repo, &["add", "a.txt"]);
        assert!(watched.changed(), "stage");
        git(&watched.repo, &["switch", "-q", "-c", "other"]);
        assert!(watched.changed(), "branch switch");
    }

    #[test]
    fn edits_under_ignored_folders_do_not_trigger() {
        let watched = Watched::new();
        write(watched.repo.join("node_modules/x/index.js"), "x\n");
        write(watched.repo.join("target/debug/app"), "x\n");
        assert!(!watched.changed());
    }

    /// A plane with a `web` repo and a `plane.toml`, watched only for the
    /// given File tabs, as with Source control closed.
    fn watch_file_tabs(file_tabs: &[&str]) -> (tempfile::TempDir, mpsc::Receiver<()>, GitWatch) {
        let root = tempfile::tempdir().unwrap();
        repo(&root.path().join("web"));
        write(root.path().join("plane.toml"), "version = 1\n");
        write(root.path().join("notes/todo.md"), "x\n");
        let (sender, changes) = mpsc::channel();
        let sender = Mutex::new(sender);
        let mut watch = GitWatch::new(Arc::new(move || {
            let _ = sender.lock().unwrap().send(());
        }))
        .unwrap();
        let tabs: Vec<String> = file_tabs.iter().map(|tab| tab.to_string()).collect();
        watch.set_scope(Scope::with_file_tabs(root.path(), &[], &tabs));
        std::thread::sleep(SETTLE);
        while changes.try_recv().is_ok() {}
        (root, changes, watch)
    }

    fn changed(changes: &mpsc::Receiver<()>) -> bool {
        let changed = changes.recv_timeout(WAIT).is_ok();
        std::thread::sleep(SETTLE);
        while changes.try_recv().is_ok() {}
        changed
    }

    #[test]
    fn a_file_tab_follows_an_edit_a_stage_and_a_commit_without_source_control() {
        let (root, changes, _watch) = watch_file_tabs(&["web/a.txt"]);
        let web = root.path().join("web");
        write(web.join("a.txt"), "edited\n");
        assert!(changed(&changes), "edit");
        git(&web, &["add", "a.txt"]);
        assert!(changed(&changes), "stage");
        git(&web, &["commit", "-q", "-m", "second"]);
        assert!(changed(&changes), "commit");
    }

    #[test]
    fn an_outside_repo_file_tab_watches_only_its_folder() {
        let (root, changes, _watch) = watch_file_tabs(&["plane.toml"]);
        write(root.path().join("plane.toml"), "version = 2\n");
        assert!(changed(&changes), "edit");
        write(root.path().join("notes/todo.md"), "y\n");
        assert!(!changed(&changes), "a subfolder isn't watched");
    }

    #[test]
    fn file_tabs_scope_their_repo_or_folder() {
        let root = tempfile::tempdir().unwrap();
        repo(&root.path().join("web"));
        write(root.path().join("plane.toml"), "");
        let tabs = [
            "web/a.txt",
            "web/src/new.rs",
            "plane.toml",
            "web/.git/HEAD",
            "gone/x",
        ]
        .map(String::from);
        let scope = Scope::with_file_tabs(root.path(), &[], &tabs);
        let canonical = root.path().canonicalize().unwrap();
        assert_eq!(scope.worktrees, [canonical.join("web")]);
        assert_eq!(scope.folders, [canonical.clone()]);
        assert_eq!(scope.git_dirs, [canonical.join("web/.git")]);
    }
}
