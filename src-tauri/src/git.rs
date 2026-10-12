//! Source control: the repos in a plane and their `git status` (ADR 0006).

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::process::{Child, Command};

use crate::bp;
use crate::env::EnvMap;

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

/// Every repo in a plane with its status. A missing worktree or a failing
/// `git` is reported on its repo, not as an error for the whole plane.
pub async fn source_control(
    root: &Path,
    members: &[PathBuf],
    env: &EnvMap,
) -> Result<Vec<Repo>, String> {
    if !root.is_dir() {
        return Err(format!("{} no longer exists", root.display()));
    }
    let git = bp::find_on_path("git", env.get("PATH").map(String::as_str))
        .ok_or("git isn't on your PATH")?;
    let env = bp::git_env(env);
    let dirs = repo_dirs(root, members);
    // All spawned before any is awaited, so the repos are read in parallel.
    let children: Vec<_> = dirs
        .iter()
        .map(|dir| dir.is_dir().then(|| spawn_status(&git, dir, &env)))
        .collect();
    let mut repos = Vec::with_capacity(dirs.len());
    for (dir, child) in dirs.iter().zip(children) {
        let state = match child {
            None => RepoState::Missing,
            Some(child) => read_status(child).await,
        };
        repos.push(Repo {
            name: repo_name(root, dir),
            path: dir.to_string_lossy().into_owned(),
            state,
        });
    }
    Ok(repos)
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

/// Parse `git status --porcelain=v2 --branch -z`.
pub fn parse_status(output: &[u8]) -> Result<Status, String> {
    let text = String::from_utf8_lossy(output);
    let mut fields = text.split('\0');
    let (mut oid, mut branch, mut tracking) = (None, None, None);
    let mut status = Status {
        head: Head::Branch {
            name: String::new(),
        },
        tracking: None,
        staged: Vec::new(),
        unstaged: Vec::new(),
        untracked: Vec::new(),
    };
    while let Some(entry) = fields.next() {
        let (tag, rest) = entry.split_once(' ').unwrap_or((entry, ""));
        match tag {
            "#" => match rest.split_once(' ') {
                Some(("branch.oid", value)) => oid = Some(value),
                Some(("branch.head", value)) => branch = Some(value),
                Some(("branch.ab", value)) => tracking = parse_ab(value),
                _ => {}
            },
            "1" => {
                let parts: Vec<&str> = rest.splitn(8, ' ').collect();
                let [xy, .., path] = parts[..] else {
                    return Err(unexpected(entry));
                };
                push_change(&mut status, xy, path, None);
            }
            "2" => {
                let parts: Vec<&str> = rest.splitn(9, ' ').collect();
                let [xy, .., path] = parts[..] else {
                    return Err(unexpected(entry));
                };
                let from = fields.next().ok_or_else(|| unexpected(entry))?;
                push_change(&mut status, xy, path, Some(from));
            }
            "u" => {
                let parts: Vec<&str> = rest.splitn(10, ' ').collect();
                let [.., path] = parts[..] else {
                    return Err(unexpected(entry));
                };
                status.unstaged.push(Change {
                    path: path.into(),
                    kind: ChangeKind::Conflicted,
                    from: None,
                });
            }
            "?" => status.untracked.push(rest.into()),
            _ => {}
        }
    }
    let (Some(oid), Some(branch)) = (oid, branch) else {
        return Err("git status didn't report the branch".into());
    };
    status.head = match branch {
        "(detached)" => Head::Detached {
            sha: oid.chars().take(7).collect(),
        },
        name => Head::Branch { name: name.into() },
    };
    status.tracking = tracking;
    Ok(status)
}

fn spawn_status(git: &Path, dir: &Path, env: &EnvMap) -> std::io::Result<Child> {
    Command::new(git)
        .args([
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
        ])
        .current_dir(dir)
        .env_clear()
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
}

async fn read_status(child: std::io::Result<Child>) -> RepoState {
    let output = match child {
        Ok(child) => child.wait_with_output().await,
        Err(error) => Err(error),
    };
    let result = match output {
        Err(error) => Err(format!("couldn't run git: {error}")),
        Ok(output) if output.status.success() => parse_status(&output.stdout),
        Ok(output) => Err(match String::from_utf8_lossy(&output.stderr).trim() {
            "" => format!("git exited with {}", output.status),
            message => message.to_string(),
        }),
    };
    match result {
        Ok(status) => RepoState::Ready { status },
        Err(message) => RepoState::Failed { message },
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

fn push_change(status: &mut Status, xy: &str, path: &str, from: Option<&str>) {
    let mut codes = xy.chars();
    let (index, worktree) = (codes.next(), codes.next());
    for (code, list) in [
        (index, &mut status.staged),
        (worktree, &mut status.unstaged),
    ] {
        let Some(kind) = code.and_then(change_kind) else {
            continue;
        };
        let from = matches!(kind, ChangeKind::Renamed | ChangeKind::Copied)
            .then(|| from.map(String::from))
            .flatten();
        list.push(Change {
            path: path.into(),
            kind,
            from,
        });
    }
}

fn change_kind(code: char) -> Option<ChangeKind> {
    Some(match code {
        '.' => return None,
        'T' => ChangeKind::TypeChanged,
        'A' => ChangeKind::Added,
        'D' => ChangeKind::Deleted,
        'R' => ChangeKind::Renamed,
        'C' => ChangeKind::Copied,
        'U' => ChangeKind::Conflicted,
        _ => ChangeKind::Modified,
    })
}

fn parse_ab(value: &str) -> Option<Tracking> {
    let (ahead, behind) = value.split_once(' ')?;
    Some(Tracking {
        ahead: ahead.strip_prefix('+')?.parse().ok()?,
        behind: behind.strip_prefix('-')?.parse().ok()?,
    })
}

fn unexpected(entry: &str) -> String {
    format!("unexpected git status line: {entry}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn branch(name: &str) -> Head {
        Head::Branch { name: name.into() }
    }

    fn change(path: &str, kind: ChangeKind) -> Change {
        Change {
            path: path.into(),
            kind,
            from: None,
        }
    }

    #[test]
    fn parses_a_clean_branch_with_an_upstream() {
        let output = "# branch.oid 0123456789abcdef\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0";
        let status = parse_status(output.as_bytes()).unwrap();
        assert_eq!(status.head, branch("main"));
        assert_eq!(
            status.tracking,
            Some(Tracking {
                ahead: 2,
                behind: 1
            })
        );
        assert!(status.staged.is_empty() && status.unstaged.is_empty());
    }

    #[test]
    fn has_no_tracking_without_an_upstream() {
        let output = "# branch.oid 0123456789abcdef\0# branch.head main\0";
        assert_eq!(parse_status(output.as_bytes()).unwrap().tracking, None);
    }

    #[test]
    fn shortens_a_detached_head() {
        let output = "# branch.oid 0123456789abcdef\0# branch.head (detached)\0";
        let status = parse_status(output.as_bytes()).unwrap();
        assert_eq!(
            status.head,
            Head::Detached {
                sha: "0123456".into()
            }
        );
    }

    #[test]
    fn reads_a_branch_without_commits() {
        let output = "# branch.oid (initial)\0# branch.head main\0? a b.txt\0";
        let status = parse_status(output.as_bytes()).unwrap();
        assert_eq!(status.head, branch("main"));
        assert_eq!(status.untracked, ["a b.txt"]);
    }

    #[test]
    fn splits_changes_into_staged_and_unstaged() {
        let z = "0000000000000000000000000000000000000000";
        let output = format!(
            "# branch.oid {z}\0# branch.head main\0\
             1 MM N... 100644 100644 100644 {z} {z} src/both.rs\0\
             1 A. N... 000000 100644 100644 {z} {z} new file.rs\0\
             1 .D N... 100644 100644 000000 {z} {z} gone.rs\0\
             2 R. N... 100644 100644 100644 {z} {z} R100 to.rs\0from.rs\0\
             u UU N... 100644 100644 100644 100644 {z} {z} {z} clash.rs\0\
             ? notes.md\0"
        );
        let status = parse_status(output.as_bytes()).unwrap();
        assert_eq!(
            status.staged,
            [
                change("src/both.rs", ChangeKind::Modified),
                change("new file.rs", ChangeKind::Added),
                Change {
                    from: Some("from.rs".into()),
                    ..change("to.rs", ChangeKind::Renamed)
                },
            ]
        );
        assert_eq!(
            status.unstaged,
            [
                change("src/both.rs", ChangeKind::Modified),
                change("gone.rs", ChangeKind::Deleted),
                change("clash.rs", ChangeKind::Conflicted),
            ]
        );
        assert_eq!(status.untracked, ["notes.md"]);
    }

    #[test]
    fn refuses_output_without_a_branch() {
        assert!(parse_status(b"? a\0").is_err());
    }

    #[test]
    fn names_repos_by_their_path_in_the_plane() {
        let root = Path::new("/planes/p");
        assert_eq!(repo_name(root, &root.join("projects/web")), "web");
        assert_eq!(repo_name(root, &root.join("sos/api")), "sos/api");
        assert_eq!(repo_name(root, root), "p");
    }

    mod with_git {
        use std::process::Command as StdCommand;

        use super::*;

        struct Plane {
            dir: tempfile::TempDir,
            env: EnvMap,
        }

        impl Plane {
            fn new() -> Self {
                let dir = tempfile::tempdir().unwrap();
                let mut env: EnvMap = std::env::vars()
                    .filter(|(key, _)| !key.starts_with("GIT_"))
                    .collect();
                for (key, value) in [
                    ("GIT_CONFIG_GLOBAL", "/dev/null"),
                    ("GIT_CONFIG_NOSYSTEM", "1"),
                    ("GIT_AUTHOR_NAME", "Test"),
                    ("GIT_AUTHOR_EMAIL", "test@example.com"),
                    ("GIT_COMMITTER_NAME", "Test"),
                    ("GIT_COMMITTER_EMAIL", "test@example.com"),
                ] {
                    env.insert(key.into(), value.into());
                }
                Plane { dir, env }
            }

            fn root(&self) -> &Path {
                self.dir.path()
            }

            fn git(&self, dir: &str, args: &[&str]) -> String {
                let output = StdCommand::new("git")
                    .args(args)
                    .current_dir(self.root().join(dir))
                    .env_clear()
                    .envs(&self.env)
                    .output()
                    .unwrap();
                assert!(output.status.success(), "git {args:?}: {output:?}");
                String::from_utf8(output.stdout).unwrap().trim().into()
            }

            fn write(&self, path: &str, contents: &str) {
                let path = self.root().join(path);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, contents).unwrap();
            }

            /// A repo at `dir` with one commit on `main`.
            fn repo(&self, dir: &str) {
                std::fs::create_dir_all(self.root().join(dir)).unwrap();
                self.git(dir, &["init", "-q", "-b", "main"]);
                self.write(&format!("{dir}/a.txt"), "a\n");
                self.write(&format!("{dir}/b.txt"), "b\n");
                self.git(dir, &["add", "."]);
                self.git(dir, &["commit", "-q", "-m", "first"]);
            }

            async fn repos(&self, members: &[&str]) -> Vec<Repo> {
                let members: Vec<PathBuf> = members.iter().map(PathBuf::from).collect();
                source_control(self.root(), &members, &self.env)
                    .await
                    .unwrap()
            }
        }

        fn ready(repo: &Repo) -> &Status {
            match &repo.state {
                RepoState::Ready { status } => status,
                other => panic!("{} isn't ready: {other:?}", repo.name),
            }
        }

        #[tokio::test]
        async fn reports_changes_on_a_branch_without_an_upstream() {
            let plane = Plane::new();
            plane.repo("projects/web");
            plane.write("projects/web/a.txt", "changed\n");
            plane.write("projects/web/c.txt", "c\n");
            plane.write("projects/web/u.txt", "u\n");
            plane.git("projects/web", &["add", "c.txt"]);
            plane.git("projects/web", &["mv", "b.txt", "d.txt"]);

            let repos = plane.repos(&["projects/web"]).await;
            assert_eq!(repos.len(), 1);
            assert_eq!(repos[0].name, "web");
            let status = ready(&repos[0]);
            assert_eq!(status.head, branch("main"));
            assert_eq!(status.tracking, None);
            assert_eq!(
                status.staged,
                [
                    change("c.txt", ChangeKind::Added),
                    Change {
                        from: Some("b.txt".into()),
                        ..change("d.txt", ChangeKind::Renamed)
                    },
                ]
            );
            assert_eq!(status.unstaged, [change("a.txt", ChangeKind::Modified)]);
            assert_eq!(status.untracked, ["u.txt"]);
        }

        #[tokio::test]
        async fn reports_a_detached_head_as_a_short_sha() {
            let plane = Plane::new();
            plane.repo("web");
            plane.git("web", &["checkout", "-q", "--detach"]);
            let sha = plane.git("web", &["rev-parse", "--short=7", "HEAD"]);

            let repos = plane.repos(&["web"]).await;
            assert_eq!(ready(&repos[0]).head, Head::Detached { sha });
        }

        #[tokio::test]
        async fn counts_ahead_and_behind_an_upstream() {
            let plane = Plane::new();
            plane.repo("origin");
            plane.git("", &["clone", "-q", "origin", "projects/web"]);
            plane.write("origin/c.txt", "c\n");
            plane.git("origin", &["add", "."]);
            plane.git("origin", &["commit", "-q", "-m", "upstream"]);
            plane.git("projects/web", &["fetch", "-q"]);
            plane.write("projects/web/d.txt", "d\n");
            plane.git("projects/web", &["add", "."]);
            plane.git("projects/web", &["commit", "-q", "-m", "local"]);

            let repos = plane.repos(&["projects/web"]).await;
            assert_eq!(
                ready(&repos[0]).tracking,
                Some(Tracking {
                    ahead: 1,
                    behind: 1
                })
            );
        }

        #[tokio::test]
        async fn marks_a_missing_worktree_without_failing_the_plane() {
            let plane = Plane::new();
            plane.repo("projects/web");
            std::fs::create_dir_all(plane.root().join("projects/empty")).unwrap();

            let repos = plane
                .repos(&["projects/gone", "projects/web", "projects/empty"])
                .await;
            assert_eq!(repos[0].state, RepoState::Missing);
            ready(&repos[1]);
            assert!(
                matches!(&repos[2].state, RepoState::Failed { message } if message.contains("not a git repository")),
                "{:?}",
                repos[2].state
            );
        }

        #[tokio::test]
        async fn finds_repos_in_a_plane_without_members() {
            let plane = Plane::new();
            plane.repo("scratch");
            plane.repo("projects/web");
            plane.repo(".hidden");
            std::fs::create_dir_all(plane.root().join("notes")).unwrap();

            let names: Vec<String> = plane.repos(&[]).await.into_iter().map(|r| r.name).collect();
            assert_eq!(names, ["web", "scratch"]);
        }

        #[tokio::test]
        async fn needs_git_on_the_path() {
            let plane = Plane::new();
            let bin = tempfile::tempdir().unwrap();
            let env = EnvMap::from([("PATH".into(), bin.path().display().to_string())]);
            let error = source_control(plane.root(), &[], &env).await.unwrap_err();
            assert!(error.contains("git isn't on your PATH"), "{error}");
        }
    }
}
