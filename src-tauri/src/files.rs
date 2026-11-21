//! Files: paths inside a plane and the entries the daemon lists for them
//! (ADR 0007, sessiond ADR 0032).

use std::path::{Component, Path, PathBuf};

use bitplane_protocol::fs::{FsEntry, FsEntryKind};
use serde::{Deserialize, Serialize};
use specta::Type;

pub const GIT_DIR: &str = ".git";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub kind: FileKind,
    /// Matched by the containing repo's ignore rules, or `.git` itself.
    pub ignored: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum FileKind {
    File,
    Directory,
}

impl From<FsEntry> for FileEntry {
    fn from(entry: FsEntry) -> Self {
        FileEntry {
            name: entry.name,
            kind: match entry.kind {
                FsEntryKind::Directory => FileKind::Directory,
                FsEntryKind::File | FsEntryKind::Unknown => FileKind::File,
            },
            ignored: entry.ignored,
        }
    }
}

/// `relative` (`""` for the root) under `root`, refused when it leaves the
/// plane lexically; nothing is read from disk.
pub fn plane_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative_path = Path::new(relative);
    if relative_path
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(format!("{relative} isn't a path inside the plane"));
    }
    if relative.is_empty() {
        return Ok(root.to_path_buf());
    }
    Ok(root.join(relative_path))
}

/// `relative` inside `root`, following symlinks wherever they point.
pub fn resolve(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = plane_path(root, relative)?;
    if !root.is_dir() {
        return Err(format!("{} no longer exists", root.display()));
    }
    path.canonicalize()
        .map_err(|_| format!("{relative} no longer exists"))
}

pub fn in_git_dir(relative: &str) -> bool {
    Path::new(relative)
        .components()
        .any(|c| c.as_os_str() == GIT_DIR)
}

/// The nearest folder from `dir` up to `root` holding a `.git`: the repo the
/// daemon's git module takes.
pub fn containing_repo(root: &Path, dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .take_while(|ancestor| ancestor.starts_with(root))
        .find(|ancestor| ancestor.join(GIT_DIR).exists())
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_paths_stay_inside_the_plane() {
        let root = Path::new("/p");
        assert_eq!(plane_path(root, "").unwrap(), PathBuf::from("/p"));
        assert_eq!(plane_path(root, "a/b").unwrap(), PathBuf::from("/p/a/b"));
        for relative in ["..", "../x", "/etc", "./x", "a/../b"] {
            assert!(plane_path(root, relative).is_err(), "{relative}");
        }
    }

    #[test]
    fn spots_paths_inside_git() {
        assert!(in_git_dir(".git"));
        assert!(in_git_dir("projects/web/.git/refs"));
        assert!(!in_git_dir("projects/web/.github"));
    }

    #[test]
    fn finds_the_nearest_repo_within_the_plane() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        std::fs::create_dir_all(root.join("web/.git")).unwrap();
        std::fs::create_dir_all(root.join("web/app/src")).unwrap();
        std::fs::create_dir_all(root.join("web/app/.git")).unwrap();
        std::fs::create_dir_all(root.join("notes")).unwrap();
        assert_eq!(
            containing_repo(root, &root.join("web/app/src")),
            Some(root.join("web/app"))
        );
        assert_eq!(
            containing_repo(root, &root.join("web")),
            Some(root.join("web"))
        );
        assert_eq!(containing_repo(root, &root.join("notes")), None);
    }
}
