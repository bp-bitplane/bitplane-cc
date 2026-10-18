//! What a File tab shows: one file's diff, both sides of it, or its plain
//! contents, all read through the git CLI (ADR 0006).

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::env::EnvMap;
use crate::files;
use crate::git_cli::Git;

/// Past this on either side, a file is a placeholder instead of a diff.
pub const MAX_FILE_BYTES: u64 = 1_000_000;

/// Git's own heuristic: a NUL in the first 8000 bytes means binary.
const BINARY_SNIFF_BYTES: usize = 8000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DiffMode {
    /// HEAD ↔ index.
    Index,
    /// Index ↔ worktree.
    Worktree,
    /// HEAD ↔ worktree.
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FileDiff {
    Diff {
        patch: String,
    },
    /// Nothing to compare: ignored, outside any repo, or unchanged in `file` mode.
    Plain,
    #[serde(rename_all = "camelCase")]
    Binary {
        old_size: u64,
        new_size: u64,
    },
    TooLarge {
        size: u64,
    },
    /// The change this `index` or `worktree` tab showed is gone.
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileContents {
    pub old: String,
    pub new: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FileText {
    Text { contents: String },
    Binary { size: u64 },
    TooLarge { size: u64 },
}

pub async fn file_diff(
    root: &Path,
    relative: &str,
    mode: DiffMode,
    env: &EnvMap,
) -> Result<FileDiff, String> {
    let target = locate(root, relative)?;
    let Some(repo) = &target.repo else {
        return plain(&target, relative);
    };
    let git = Git::find(env).ok_or(NO_GIT)?;
    let (dir, path) = (repo.dir.as_path(), repo.path.as_str());
    if git.succeeds(dir, &["check-ignore", "-q", "--", path]).await {
        return plain(&target, relative);
    }
    if !is_tracked(&git, dir, path).await? {
        return untracked_diff(&git, &target, repo, mode, relative).await;
    }
    let base = mode.diff_args();
    let from = find_rename(&git, dir, base, path).await?;
    let old_path = from.as_deref().unwrap_or(path);
    let old_size = blob_size(&git, dir, &mode.old_spec(old_path)).await;
    let new_size = match mode {
        DiffMode::Index => blob_size(&git, dir, &format!(":{path}")).await,
        DiffMode::Worktree | DiffMode::File => disk_size(&target.path).unwrap_or(0),
    };
    let size = old_size.max(new_size);
    if size > MAX_FILE_BYTES {
        return Ok(FileDiff::TooLarge { size });
    }
    let mut pathspec = vec!["--", path];
    pathspec.extend(from.as_deref());
    let numstat = git
        .run(dir, &[base, &["--numstat", "-M"], &pathspec].concat(), &[])
        .await?;
    if numstat.is_empty() {
        return Ok(match mode {
            DiffMode::File => FileDiff::Plain,
            DiffMode::Index | DiffMode::Worktree => FileDiff::None,
        });
    }
    if is_binary(&numstat) {
        return Ok(FileDiff::Binary { old_size, new_size });
    }
    let patch = git
        .run(dir, &[base, PATCH_ARGS, &["-M"], &pathspec].concat(), &[])
        .await?;
    Ok(FileDiff::Diff {
        patch: String::from_utf8_lossy(&patch).into_owned(),
    })
}

/// Both sides of a diff, so unchanged context can be expanded; a side that
/// doesn't exist is empty.
pub async fn file_contents(
    root: &Path,
    relative: &str,
    mode: DiffMode,
    env: &EnvMap,
) -> Result<FileContents, String> {
    let target = locate(root, relative)?;
    let disk = || {
        std::fs::read(&target.path)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default()
    };
    let Some(repo) = &target.repo else {
        return Ok(FileContents {
            old: String::new(),
            new: disk(),
        });
    };
    let git = Git::find(env).ok_or(NO_GIT)?;
    let (dir, path) = (repo.dir.as_path(), repo.path.as_str());
    let from = find_rename(&git, dir, mode.diff_args(), path).await?;
    let old = show(&git, dir, &mode.old_spec(from.as_deref().unwrap_or(path))).await;
    let new = match mode {
        DiffMode::Index => show(&git, dir, &format!(":{path}")).await,
        DiffMode::Worktree | DiffMode::File => disk(),
    };
    Ok(FileContents { old, new })
}

/// A file's contents for a File tab with no diff to show.
pub fn read_file(root: &Path, relative: &str) -> Result<FileText, String> {
    let path = files::resolve(root, relative)?;
    let size = disk_size(&path).ok_or_else(|| format!("{relative} isn't a file"))?;
    if size > MAX_FILE_BYTES {
        return Ok(FileText::TooLarge { size });
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("couldn't read {relative}: {e}"))?;
    if looks_binary(&bytes) {
        return Ok(FileText::Binary { size });
    }
    Ok(FileText::Text {
        contents: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

const NO_GIT: &str = "git isn't on your PATH";

/// Fixed prefixes, whatever the user's config says, so the patch parses.
const PATCH_ARGS: &[&str] = &[
    "--no-color",
    "--no-ext-diff",
    "--src-prefix=a/",
    "--dst-prefix=b/",
];

impl DiffMode {
    fn diff_args(self) -> &'static [&'static str] {
        match self {
            DiffMode::Index => &["diff", "--cached"],
            DiffMode::Worktree => &["diff"],
            DiffMode::File => &["diff", "HEAD"],
        }
    }

    fn old_spec(self, path: &str) -> String {
        match self {
            DiffMode::Index | DiffMode::File => format!("HEAD:{path}"),
            DiffMode::Worktree => format!(":{path}"),
        }
    }
}

struct Target {
    /// The file on disk; it may not exist when it was deleted.
    path: PathBuf,
    repo: Option<RepoPath>,
}

struct RepoPath {
    dir: PathBuf,
    /// Relative to `dir`, `/`-separated, as git names it.
    path: String,
}

/// Only the folder must exist: a deleted file still has a diff.
fn locate(root: &Path, relative: &str) -> Result<Target, String> {
    let relative_path = Path::new(relative);
    let name = match relative_path.components().next_back() {
        Some(Component::Normal(name)) => name,
        _ => return Err(format!("{relative} isn't a path inside the plane")),
    };
    let parent = relative_path.parent().unwrap_or(Path::new(""));
    let dir = files::resolve(root, &parent.to_string_lossy())?;
    let path = dir.join(name);
    let repo = if files::in_git_dir(relative) {
        None
    } else {
        files::containing_repo(root, &dir).and_then(|repo| {
            let inside = path.strip_prefix(&repo).ok()?.to_str()?.to_string();
            Some(RepoPath {
                dir: repo,
                path: inside,
            })
        })
    };
    Ok(Target { path, repo })
}

fn plain(target: &Target, relative: &str) -> Result<FileDiff, String> {
    match disk_size(&target.path) {
        Some(_) => Ok(FileDiff::Plain),
        None if target.path.exists() => Err(format!("{relative} isn't a file")),
        None => Err(format!("{relative} no longer exists")),
    }
}

/// In the index or in HEAD; a staged deletion is only in HEAD.
async fn is_tracked(git: &Git, dir: &Path, path: &str) -> Result<bool, String> {
    let listed = git.run(dir, &["ls-files", "-z", "--", path], &[]).await?;
    Ok(!listed.is_empty()
        || git
            .succeeds(dir, &["cat-file", "-e", &format!("HEAD:{path}")])
            .await)
}

/// The whole file as added; there's nothing staged to show in `index` mode.
async fn untracked_diff(
    git: &Git,
    target: &Target,
    repo: &RepoPath,
    mode: DiffMode,
    relative: &str,
) -> Result<FileDiff, String> {
    if mode == DiffMode::Index {
        return Ok(FileDiff::None);
    }
    let size = disk_size(&target.path).ok_or_else(|| format!("{relative} no longer exists"))?;
    if size > MAX_FILE_BYTES {
        return Ok(FileDiff::TooLarge { size });
    }
    let (dir, path) = (repo.dir.as_path(), repo.path.as_str());
    let pathspec = ["--", "/dev/null", path];
    // `--no-index` exits 1 when the sides differ, which they always do here.
    let numstat = git
        .run(
            dir,
            &[&["diff", "--no-index", "--numstat"], &pathspec[..]].concat(),
            &[1],
        )
        .await?;
    if is_binary(&numstat) {
        return Ok(FileDiff::Binary {
            old_size: 0,
            new_size: size,
        });
    }
    let patch = git
        .run(
            dir,
            &[&["diff", "--no-index"], PATCH_ARGS, &pathspec].concat(),
            &[1],
        )
        .await?;
    Ok(FileDiff::Diff {
        patch: String::from_utf8_lossy(&patch).into_owned(),
    })
}

/// Where `path` was renamed from. Git only pairs a rename when both paths
/// are in the pathspec, so the whole repo's renames are listed to find it.
async fn find_rename(
    git: &Git,
    dir: &Path,
    base: &[&str],
    path: &str,
) -> Result<Option<String>, String> {
    let listed = git
        .run(dir, &[base, &["--name-status", "-M", "-z"]].concat(), &[])
        .await?;
    let listed = String::from_utf8_lossy(&listed);
    let mut fields = listed.split('\0');
    while let Some(status) = fields.next() {
        if status.starts_with('R') || status.starts_with('C') {
            let (Some(from), Some(to)) = (fields.next(), fields.next()) else {
                break;
            };
            if status.starts_with('R') && to == path {
                return Ok(Some(from.to_string()));
            }
        } else {
            fields.next();
        }
    }
    Ok(None)
}

/// `0` when the blob doesn't exist.
async fn blob_size(git: &Git, dir: &Path, spec: &str) -> u64 {
    git.run(dir, &["cat-file", "-s", spec], &[])
        .await
        .ok()
        .and_then(|size| String::from_utf8_lossy(&size).trim().parse().ok())
        .unwrap_or(0)
}

/// Empty when the blob doesn't exist.
async fn show(git: &Git, dir: &Path, spec: &str) -> String {
    git.run(dir, &["show", spec], &[])
        .await
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

fn disk_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .filter(|meta| meta.is_file())
        .map(|meta| meta.len())
}

/// `--numstat` counts a binary file as `-\t-`.
fn is_binary(numstat: &[u8]) -> bool {
    numstat.starts_with(b"-\t-\t")
}

fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(BINARY_SNIFF_BYTES).any(|byte| *byte == 0)
}

#[cfg(test)]
mod tests {
    use std::process::Command as StdCommand;

    use super::*;

    struct Plane {
        dir: tempfile::TempDir,
        env: EnvMap,
    }

    impl Plane {
        fn new() -> Self {
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
            Plane {
                dir: tempfile::tempdir().unwrap(),
                env,
            }
        }

        /// A plane with one Member, `web`, holding a committed `a.txt`.
        fn with_repo() -> Self {
            let plane = Plane::new();
            std::fs::create_dir_all(plane.root().join("web")).unwrap();
            plane.git(&["init", "-q", "-b", "main"]);
            plane.write("web/a.txt", "one\ntwo\n");
            plane.git(&["add", "."]);
            plane.git(&["commit", "-q", "-m", "init"]);
            plane
        }

        fn root(&self) -> &Path {
            self.dir.path()
        }

        fn git(&self, args: &[&str]) {
            let output = StdCommand::new("git")
                .args(args)
                .current_dir(self.root().join("web"))
                .env_clear()
                .envs(&self.env)
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
        }

        fn write(&self, path: &str, contents: impl AsRef<[u8]>) {
            let path = self.root().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }

        async fn diff(&self, path: &str, mode: DiffMode) -> FileDiff {
            file_diff(self.root(), path, mode, &self.env).await.unwrap()
        }

        async fn contents(&self, path: &str, mode: DiffMode) -> (String, String) {
            let contents = file_contents(self.root(), path, mode, &self.env)
                .await
                .unwrap();
            (contents.old, contents.new)
        }
    }

    fn patch(diff: FileDiff) -> String {
        match diff {
            FileDiff::Diff { patch } => patch,
            other => panic!("expected a diff, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn each_mode_compares_its_own_sides() {
        let plane = Plane::with_repo();
        plane.write("web/a.txt", "one\nstaged\n");
        plane.git(&["add", "a.txt"]);
        plane.write("web/a.txt", "one\nstaged\nunstaged\n");

        let index = patch(plane.diff("web/a.txt", DiffMode::Index).await);
        assert!(index.contains("-two\n+staged\n"), "{index}");
        assert!(!index.contains("unstaged"), "{index}");

        let worktree = patch(plane.diff("web/a.txt", DiffMode::Worktree).await);
        assert!(worktree.contains(" staged\n+unstaged\n"), "{worktree}");
        assert!(!worktree.contains("-two"), "{worktree}");

        let file = patch(plane.diff("web/a.txt", DiffMode::File).await);
        assert!(file.contains("-two\n+staged\n+unstaged\n"), "{file}");
        assert!(file.starts_with("diff --git a/a.txt b/a.txt\n"), "{file}");
    }

    #[tokio::test]
    async fn an_unchanged_file_is_plain_or_has_no_change() {
        let plane = Plane::with_repo();
        assert_eq!(
            plane.diff("web/a.txt", DiffMode::File).await,
            FileDiff::Plain
        );
        assert_eq!(
            plane.diff("web/a.txt", DiffMode::Index).await,
            FileDiff::None
        );
        assert_eq!(
            plane.diff("web/a.txt", DiffMode::Worktree).await,
            FileDiff::None
        );
    }

    #[tokio::test]
    async fn an_untracked_file_is_all_added() {
        let plane = Plane::with_repo();
        plane.write("web/new/b.txt", "fresh\n");
        let diff = patch(plane.diff("web/new/b.txt", DiffMode::Worktree).await);
        assert!(diff.contains("new file mode"), "{diff}");
        assert!(diff.contains("+fresh\n"), "{diff}");
        assert!(matches!(
            plane.diff("web/new/b.txt", DiffMode::File).await,
            FileDiff::Diff { .. }
        ));
        assert_eq!(
            plane.diff("web/new/b.txt", DiffMode::Index).await,
            FileDiff::None
        );
        assert_eq!(
            plane.contents("web/new/b.txt", DiffMode::File).await,
            (String::new(), "fresh\n".into())
        );
    }

    #[tokio::test]
    async fn ignored_and_outside_repo_files_are_plain() {
        let plane = Plane::with_repo();
        plane.write("web/.gitignore", "*.log\n");
        plane.write("web/debug.log", "noise\n");
        plane.write("plane.toml", "version = 1\n");
        plane.write(".bitplane/state", "");
        assert_eq!(
            plane.diff("web/debug.log", DiffMode::File).await,
            FileDiff::Plain
        );
        assert_eq!(
            plane.diff("plane.toml", DiffMode::File).await,
            FileDiff::Plain
        );
        assert_eq!(
            plane.diff(".bitplane/state", DiffMode::File).await,
            FileDiff::Plain
        );
        assert_eq!(
            plane.diff("web/.git/HEAD", DiffMode::File).await,
            FileDiff::Plain
        );
        assert!(
            file_diff(plane.root(), "missing.toml", DiffMode::File, &plane.env)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_staged_rename_diffs_against_its_old_path() {
        let plane = Plane::with_repo();
        plane.git(&["mv", "a.txt", "b.txt"]);
        let diff = patch(plane.diff("web/b.txt", DiffMode::Index).await);
        assert!(
            diff.contains("rename from a.txt\nrename to b.txt"),
            "{diff}"
        );
        assert_eq!(
            plane.contents("web/b.txt", DiffMode::Index).await,
            ("one\ntwo\n".into(), "one\ntwo\n".into())
        );
    }

    #[tokio::test]
    async fn a_deleted_file_is_all_removed() {
        let plane = Plane::with_repo();
        std::fs::remove_file(plane.root().join("web/a.txt")).unwrap();
        let worktree = patch(plane.diff("web/a.txt", DiffMode::Worktree).await);
        assert!(worktree.contains("deleted file mode"), "{worktree}");
        assert!(worktree.contains("-one\n-two\n"), "{worktree}");
        plane.git(&["rm", "-q", "a.txt"]);
        let index = patch(plane.diff("web/a.txt", DiffMode::Index).await);
        assert!(index.contains("deleted file mode"), "{index}");
        assert_eq!(
            plane.contents("web/a.txt", DiffMode::Index).await,
            ("one\ntwo\n".into(), String::new())
        );
    }

    #[tokio::test]
    async fn binary_files_report_their_sizes() {
        let plane = Plane::with_repo();
        plane.write("web/img.bin", [0u8, 1, 2, 3]);
        plane.git(&["add", "img.bin"]);
        plane.git(&["commit", "-q", "-m", "bin"]);
        plane.write("web/img.bin", [0u8, 1, 2, 3, 4, 5]);
        assert_eq!(
            plane.diff("web/img.bin", DiffMode::Worktree).await,
            FileDiff::Binary {
                old_size: 4,
                new_size: 6
            }
        );
        plane.write("web/new.bin", [0u8, 9]);
        assert_eq!(
            plane.diff("web/new.bin", DiffMode::Worktree).await,
            FileDiff::Binary {
                old_size: 0,
                new_size: 2
            }
        );
        assert_eq!(
            read_file(plane.root(), "web/new.bin").unwrap(),
            FileText::Binary { size: 2 }
        );
    }

    #[tokio::test]
    async fn large_files_are_too_large() {
        let plane = Plane::with_repo();
        let big = "x".repeat(MAX_FILE_BYTES as usize + 1);
        plane.write("web/a.txt", &big);
        plane.write("web/big.txt", &big);
        let size = MAX_FILE_BYTES + 1;
        assert_eq!(
            plane.diff("web/a.txt", DiffMode::Worktree).await,
            FileDiff::TooLarge { size }
        );
        assert_eq!(
            plane.diff("web/big.txt", DiffMode::File).await,
            FileDiff::TooLarge { size }
        );
        assert_eq!(
            read_file(plane.root(), "web/big.txt").unwrap(),
            FileText::TooLarge { size }
        );
    }

    #[tokio::test]
    async fn refuses_paths_that_leave_the_plane() {
        let plane = Plane::with_repo();
        for relative in ["..", "../x", "/etc/passwd", "web/../../x", "", "web/"] {
            assert!(
                file_diff(plane.root(), relative, DiffMode::File, &plane.env)
                    .await
                    .is_err(),
                "{relative}"
            );
            assert!(read_file(plane.root(), relative).is_err(), "{relative}");
        }
    }

    #[tokio::test]
    async fn a_missing_worktree_is_an_error() {
        let plane = Plane::new();
        assert!(
            file_diff(plane.root(), "web/a.txt", DiffMode::File, &plane.env)
                .await
                .is_err()
        );
    }

    #[test]
    fn reads_plain_text() {
        let plane = Plane::new();
        plane.write("plane.toml", "version = 1\n");
        assert_eq!(
            read_file(plane.root(), "plane.toml").unwrap(),
            FileText::Text {
                contents: "version = 1\n".into()
            }
        );
    }
}
