//! Watching the folders expanded in Files, each on its own.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::files;
use crate::planes::DEBOUNCE;

/// `files_changed`: folders of a watched plane to list again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FilesEvent {
    pub plane_id: String,
    /// Relative to the plane root; `""` is the root.
    pub dirs: Vec<String>,
}

/// The plane and its folders being watched, keyed by canonical path. Symlinks
/// can reach one folder through several relative paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    pub plane_id: String,
    pub dirs: BTreeMap<PathBuf, BTreeSet<String>>,
}

impl Scope {
    /// The existing folders among `dirs`, skipping anything inside `.git`.
    pub fn of(plane_id: &str, root: &Path, dirs: &[String]) -> Scope {
        let mut scope = Scope {
            plane_id: plane_id.into(),
            dirs: BTreeMap::new(),
        };
        for dir in dirs.iter().filter(|dir| !files::in_git_dir(dir)) {
            let Ok(path) = files::resolve(root, dir) else {
                continue;
            };
            if path.is_dir() {
                scope.dirs.entry(path).or_default().insert(dir.clone());
            }
        }
        scope
    }

    /// The watched folders whose listing a change at `path` can alter.
    pub fn affected(&self, path: &Path) -> BTreeSet<String> {
        [Some(path), path.parent()]
            .into_iter()
            .flatten()
            .filter_map(|dir| self.dirs.get(dir))
            .flatten()
            .cloned()
            .collect()
    }
}

pub struct FilesWatch {
    debouncer: Debouncer<RecommendedWatcher>,
    scope: Arc<Mutex<Scope>>,
}

impl FilesWatch {
    /// `on_change` runs on the debouncer's thread for each batch that touches
    /// a watched folder.
    pub fn new(on_change: Arc<dyn Fn(FilesEvent) + Send + Sync>) -> Result<Self, String> {
        let scope = Arc::new(Mutex::new(Scope::default()));
        let filter = Arc::clone(&scope);
        let debouncer = new_debouncer(DEBOUNCE, move |result: DebounceEventResult| {
            let Ok(events) = result else { return };
            let scope = filter.lock().unwrap();
            let dirs: BTreeSet<String> = events
                .iter()
                .flat_map(|event| scope.affected(&event.path))
                .collect();
            if dirs.is_empty() {
                return;
            }
            let event = FilesEvent {
                plane_id: scope.plane_id.clone(),
                dirs: dirs.into_iter().collect(),
            };
            drop(scope);
            on_change(event);
        })
        .map_err(|e| e.to_string())?;
        Ok(FilesWatch { debouncer, scope })
    }

    /// Watch exactly `scope`; an empty scope stops watching.
    pub fn set_scope(&mut self, scope: Scope) {
        let mut current = self.scope.lock().unwrap();
        if *current == scope {
            return;
        }
        let watcher = self.debouncer.watcher();
        for path in current.dirs.keys().filter(|p| !scope.dirs.contains_key(*p)) {
            let _ = watcher.unwatch(path);
        }
        for path in scope.dirs.keys().filter(|p| !current.dirs.contains_key(*p)) {
            let _ = watcher.watch(path, RecursiveMode::NonRecursive);
        }
        *current = scope;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    const SETTLE: Duration = Duration::from_millis(700);
    const WAIT: Duration = Duration::from_secs(3);

    fn write(path: PathBuf) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "x").unwrap();
    }

    #[test]
    fn skips_git_internals_and_missing_folders() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("web/.git/refs")).unwrap();
        std::fs::create_dir_all(root.path().join("web/src")).unwrap();
        let scope = Scope::of(
            "p",
            root.path(),
            &[
                "".into(),
                "web/src".into(),
                "web/.git/refs".into(),
                "gone".into(),
            ],
        );
        let watched: Vec<&String> = scope.dirs.values().flatten().collect();
        assert_eq!(watched, ["", "web/src"]);
    }

    #[test]
    fn a_change_affects_its_folder_and_a_watched_folder_itself() {
        let scope = Scope {
            plane_id: "p".into(),
            dirs: BTreeMap::from([
                (PathBuf::from("/p"), BTreeSet::from([String::new()])),
                (PathBuf::from("/p/src"), BTreeSet::from(["src".into()])),
            ]),
        };
        assert_eq!(
            scope.affected(Path::new("/p/src/a.rs")),
            BTreeSet::from(["src".into()])
        );
        assert_eq!(
            scope.affected(Path::new("/p/src")),
            BTreeSet::from(["".into(), "src".into()])
        );
        assert!(scope.affected(Path::new("/p/src/deep/a.rs")).is_empty());
    }

    #[test]
    fn keeps_every_path_to_a_symlinked_folder() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        for link in ["a", "b"] {
            std::os::unix::fs::symlink(outside.path(), root.path().join(link)).unwrap();
        }
        let scope = Scope::of("p", root.path(), &["a".into(), "b".into()]);
        let target = outside.path().canonicalize().unwrap();
        assert_eq!(
            scope.affected(&target.join("x.md")),
            BTreeSet::from(["a".into(), "b".into()])
        );
    }

    struct Watched {
        _root: tempfile::TempDir,
        root: PathBuf,
        changes: mpsc::Receiver<FilesEvent>,
        _watch: FilesWatch,
    }

    impl Watched {
        fn new(dirs: &[&str]) -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().canonicalize().unwrap();
            for dir in ["src/deep", "web/.git"] {
                std::fs::create_dir_all(root.join(dir)).unwrap();
            }
            let (sender, changes) = mpsc::channel();
            let sender = Mutex::new(sender);
            let mut watch = FilesWatch::new(Arc::new(move |event| {
                let _ = sender.lock().unwrap().send(event);
            }))
            .unwrap();
            let dirs: Vec<String> = dirs.iter().map(|d| d.to_string()).collect();
            watch.set_scope(Scope::of("p", &root, &dirs));
            std::thread::sleep(SETTLE);
            while changes.try_recv().is_ok() {}
            Watched {
                _root: tmp,
                root,
                changes,
                _watch: watch,
            }
        }

        fn next(&self) -> Option<Vec<String>> {
            let event = self.changes.recv_timeout(WAIT).ok()?;
            assert_eq!(event.plane_id, "p");
            Some(event.dirs)
        }
    }

    #[test]
    fn reports_the_folder_a_file_appeared_in() {
        let watched = Watched::new(&["", "src"]);
        write(watched.root.join("src/new.rs"));
        assert_eq!(watched.next(), Some(vec!["src".into()]));
    }

    #[test]
    fn ignores_folders_that_are_not_expanded_and_git_internals() {
        let watched = Watched::new(&["src", "web/.git"]);
        write(watched.root.join("src/deep/a.rs"));
        write(watched.root.join("web/.git/index"));
        assert_eq!(watched.next(), None);
    }
}
