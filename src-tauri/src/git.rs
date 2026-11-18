//! Source control: the repos in a plane and their status, read by the
//! daemon's git module (sessiond ADR 0032).

use std::path::{Path, PathBuf};

use bitplane_protocol::git::{GitChange, GitChangeKind, GitHead, GitRepo, GitStatus};
use serde::{Deserialize, Serialize};
use specta::Type;

pub const PROJECTS_DIR: &str = "projects";

/// One repo's section in Source control.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Repo {
    pub name: String,
    pub path: String,
    pub state: RepoState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RepoState {
    Ready {
        status: Status,
    },
    /// The worktree folder is gone (bp's `member_worktree_missing`).
    Missing,
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub head: Head,
    /// Absent without an upstream, or when the upstream is gone.
    pub tracking: Option<Tracking>,
    pub staged: Vec<Change>,
    pub unstaged: Vec<Change>,
    pub untracked: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Head {
    Branch { name: String },
    Detached { sha: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Tracking {
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub path: String,
    pub kind: ChangeKind,
    /// The original path of a rename or copy.
    pub from: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    Modified,
    TypeChanged,
    Added,
    Deleted,
    Renamed,
    Copied,
    Conflicted,
}

/// One repo's section from the daemon's read of `dir`, a repo of the plane
/// at `root`.
pub fn repo(root: &Path, dir: &Path, read: GitRepo) -> Repo {
    let state = if read.gone {
        RepoState::Missing
    } else if let Some(status) = read.status {
        RepoState::Ready {
            status: status.into(),
        }
    } else {
        RepoState::Failed {
            message: read
                .error
                .unwrap_or_else(|| "git reported nothing for this repo".into()),
        }
    };
    Repo {
        name: repo_name(root, dir),
        path: dir.to_string_lossy().into_owned(),
        state,
    }
}

/// The Members' worktrees, or for a plane without Members, the git repos at
/// its root, among its top-level folders and in `projects/*`.
pub fn repo_dirs(root: &Path, members: &[PathBuf]) -> Vec<PathBuf> {
    if !members.is_empty() {
        return members.iter().map(|member| root.join(member)).collect();
    }
    let mut dirs: Vec<PathBuf> = std::iter::once(root.to_path_buf())
        .chain(subdirs(root))
        .chain(subdirs(&root.join(PROJECTS_DIR)))
        .filter(|dir| dir.join(".git").exists())
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

impl From<GitStatus> for Status {
    fn from(status: GitStatus) -> Self {
        Status {
            head: match status.head {
                GitHead::Branch { name } => Head::Branch { name },
                GitHead::Detached { sha } => Head::Detached { sha },
                GitHead::Unknown => Head::Branch {
                    name: String::new(),
                },
            },
            tracking: status.tracking.map(|tracking| Tracking {
                ahead: tracking.ahead,
                behind: tracking.behind,
            }),
            staged: status.staged.into_iter().map(Change::from).collect(),
            unstaged: status.unstaged.into_iter().map(Change::from).collect(),
            untracked: status.untracked,
        }
    }
}

impl From<GitChange> for Change {
    fn from(change: GitChange) -> Self {
        Change {
            path: change.path,
            kind: match change.kind {
                GitChangeKind::TypeChanged => ChangeKind::TypeChanged,
                GitChangeKind::Added => ChangeKind::Added,
                GitChangeKind::Deleted => ChangeKind::Deleted,
                GitChangeKind::Renamed => ChangeKind::Renamed,
                GitChangeKind::Copied => ChangeKind::Copied,
                GitChangeKind::Conflicted => ChangeKind::Conflicted,
                GitChangeKind::Modified | GitChangeKind::Unknown => ChangeKind::Modified,
            },
            from: change.from,
        }
    }
}

/// The repo's path inside the plane, without the `projects/` prefix.
fn repo_name(root: &Path, dir: &Path) -> String {
    let relative = dir.strip_prefix(root).unwrap_or(dir);
    let relative = relative.strip_prefix(PROJECTS_DIR).unwrap_or(relative);
    if relative.as_os_str().is_empty() {
        let name = dir.file_name().unwrap_or(dir.as_os_str());
        return name.to_string_lossy().into_owned();
    }
    relative.to_string_lossy().into_owned()
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
        .map(|entry| entry.path())
        .collect()
}

#[cfg(test)]
mod tests {
    use bitplane_protocol::git::GitTracking;

    use super::*;

    fn read(path: &str) -> GitRepo {
        GitRepo {
            path: path.into(),
            status: None,
            gone: false,
            error: None,
        }
    }

    #[test]
    fn names_repos_by_their_path_in_the_plane() {
        let root = Path::new("/planes/p");
        assert_eq!(repo_name(root, &root.join("projects/web")), "web");
        assert_eq!(repo_name(root, &root.join("sos/api")), "sos/api");
        assert_eq!(repo_name(root, root), "p");
    }

    #[test]
    fn maps_the_daemons_read_onto_the_repo_states() {
        let root = Path::new("/planes/p");
        let dir = root.join("projects/web");
        let status = GitStatus {
            head: GitHead::Detached {
                sha: "abc1234".into(),
            },
            tracking: Some(GitTracking {
                ahead: 1,
                behind: 2,
            }),
            staged: vec![GitChange {
                path: "b.txt".into(),
                kind: GitChangeKind::Renamed,
                from: Some("a.txt".into()),
            }],
            unstaged: vec![GitChange {
                path: "c.txt".into(),
                kind: GitChangeKind::Unknown,
                from: None,
            }],
            untracked: vec!["d.txt".into()],
        };
        let ready = repo(
            root,
            &dir,
            GitRepo {
                status: Some(status),
                ..read("/planes/p/projects/web")
            },
        );
        assert_eq!(ready.name, "web");
        assert_eq!(ready.path, "/planes/p/projects/web");
        assert_eq!(
            ready.state,
            RepoState::Ready {
                status: Status {
                    head: Head::Detached {
                        sha: "abc1234".into()
                    },
                    tracking: Some(Tracking {
                        ahead: 1,
                        behind: 2
                    }),
                    staged: vec![Change {
                        path: "b.txt".into(),
                        kind: ChangeKind::Renamed,
                        from: Some("a.txt".into()),
                    }],
                    unstaged: vec![Change {
                        path: "c.txt".into(),
                        kind: ChangeKind::Modified,
                        from: None,
                    }],
                    untracked: vec!["d.txt".into()],
                }
            }
        );
        let gone = GitRepo {
            gone: true,
            ..read("/planes/p/projects/web")
        };
        assert_eq!(repo(root, &dir, gone).state, RepoState::Missing);
        let failed = GitRepo {
            error: Some("fatal: bad index".into()),
            ..read("/planes/p/projects/web")
        };
        assert_eq!(
            repo(root, &dir, failed).state,
            RepoState::Failed {
                message: "fatal: bad index".into()
            }
        );
    }

    #[test]
    fn finds_repos_in_a_plane_without_members() {
        let plane = tempfile::tempdir().unwrap();
        let root = plane.path();
        for dir in ["scratch/.git", "projects/web/.git", ".hidden/.git", "notes"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        assert_eq!(
            repo_dirs(root, &[]),
            [root.join("projects/web"), root.join("scratch")]
        );
    }

    #[test]
    fn members_are_the_repos_of_a_plane_with_members() {
        let root = Path::new("/planes/p");
        assert_eq!(
            repo_dirs(root, &[PathBuf::from("projects/web")]),
            [root.join("projects/web")]
        );
    }
}
