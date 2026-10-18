//! Files: one directory of a plane at a time, with gitignore status (ADR 0007).

use std::path::{Component, Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::io::AsyncWriteExt;

use crate::env::EnvMap;
use crate::git_cli::Git;

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

/// The entries of `relative` (`""` for the plane root), sorted by name.
pub async fn list_dir(root: &Path, relative: &str, env: &EnvMap) -> Result<Vec<FileEntry>, String> {
    let dir = resolve(root, relative)?;
    let read = std::fs::read_dir(&dir).map_err(|e| format!("couldn't read {relative}: {e}"))?;
    let mut entries: Vec<FileEntry> = read
        .filter_map(Result::ok)
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let kind = match std::fs::metadata(entry.path()) {
                Ok(meta) if meta.is_dir() => FileKind::Directory,
                _ => FileKind::File,
            };
            FileEntry {
                ignored: name == GIT_DIR,
                name,
                kind,
            }
        })
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    if in_git_dir(relative) || behind_ignored_link(root, relative, env).await {
        for entry in &mut entries {
            entry.ignored = true;
        }
    } else if let Some(repo) = containing_repo(root, &dir) {
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        let ignored = check_ignore(&repo, &dir, &names, env).await;
        for entry in &mut entries {
            entry.ignored |= ignored.contains(&entry.name);
        }
    }
    Ok(entries)
}

/// `relative` inside `root`, following symlinks wherever they point.
pub fn resolve(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative_path = Path::new(relative);
    if relative_path
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(format!("{relative} isn't a path inside the plane"));
    }
    if !root.is_dir() {
        return Err(format!("{} no longer exists", root.display()));
    }
    root.join(relative_path)
        .canonicalize()
        .map_err(|_| format!("{relative} no longer exists"))
}

pub fn in_git_dir(relative: &str) -> bool {
    Path::new(relative)
        .components()
        .any(|c| c.as_os_str() == GIT_DIR)
}

/// Whether the first symlink on the way to `relative` is ignored. Git won't
/// answer for paths beyond a symlink, so whatever it holds inherits that.
async fn behind_ignored_link(root: &Path, relative: &str, env: &EnvMap) -> bool {
    let Some(link) = first_link(root, relative) else {
        return false;
    };
    let parent = link.parent().unwrap_or(Path::new(""));
    let Ok(dir) = resolve(root, &parent.to_string_lossy()) else {
        return false;
    };
    let Some(repo) = containing_repo(root, &dir) else {
        return false;
    };
    let name = link.file_name().unwrap_or_default().to_string_lossy();
    check_ignore(&repo, &dir, &[&name], env)
        .await
        .iter()
        .any(|ignored| *ignored == name)
}

fn first_link(root: &Path, relative: &str) -> Option<PathBuf> {
    let mut path = PathBuf::new();
    for component in Path::new(relative).components() {
        path.push(component);
        let meta = root.join(&path).symlink_metadata().ok()?;
        if meta.file_type().is_symlink() {
            return Some(path);
        }
    }
    None
}

/// The nearest folder from `dir` up to `root` holding a `.git`.
pub fn containing_repo(root: &Path, dir: &Path) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    dir.ancestors()
        .take_while(|ancestor| ancestor.starts_with(&root))
        .find(|ancestor| ancestor.join(GIT_DIR).exists())
        .map(Path::to_path_buf)
}

/// Which of `names` in `dir` `repo`'s ignore rules match. Any failure,
/// including git missing from `PATH`, leaves everything unignored.
async fn check_ignore(repo: &Path, dir: &Path, names: &[&str], env: &EnvMap) -> Vec<String> {
    let Some(git) = Git::find(env) else {
        return Vec::new();
    };
    let prefix = dir.strip_prefix(repo).unwrap_or(Path::new(""));
    let mut input = Vec::new();
    for name in names.iter().filter(|name| **name != GIT_DIR) {
        input.extend_from_slice(prefix.join(name).to_string_lossy().as_bytes());
        input.push(0);
    }
    if input.is_empty() {
        return Vec::new();
    }
    let Ok(mut child) = git
        .command(
            repo,
            &[
                "-c",
                "core.fsmonitor=false",
                "check-ignore",
                "-z",
                "--stdin",
            ],
        )
        .stdin(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Vec::new();
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&input).await;
    }
    let Ok(output) = child.wait_with_output().await else {
        return Vec::new();
    };
    // 1 means nothing matched; 128 means `repo` isn't usable as a repo.
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .filter_map(|path| Path::new(path).file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .collect()
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
            ] {
                env.insert(key.into(), value.into());
            }
            Plane {
                dir: tempfile::tempdir().unwrap(),
                env,
            }
        }

        fn root(&self) -> &Path {
            self.dir.path()
        }

        fn write(&self, path: &str, contents: &str) {
            let path = self.root().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }

        fn repo(&self, dir: &str) {
            std::fs::create_dir_all(self.root().join(dir)).unwrap();
            let output = StdCommand::new("git")
                .args(["init", "-q", "-b", "main"])
                .current_dir(self.root().join(dir))
                .env_clear()
                .envs(&self.env)
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
        }

        async fn list(&self, relative: &str) -> Vec<(String, FileKind, bool)> {
            list_dir(self.root(), relative, &self.env)
                .await
                .unwrap()
                .into_iter()
                .map(|entry| (entry.name, entry.kind, entry.ignored))
                .collect()
        }
    }

    fn file(name: &str, ignored: bool) -> (String, FileKind, bool) {
        (name.into(), FileKind::File, ignored)
    }

    fn dir(name: &str, ignored: bool) -> (String, FileKind, bool) {
        (name.into(), FileKind::Directory, ignored)
    }

    #[tokio::test]
    async fn lists_the_plane_root_without_ignore_rules() {
        let plane = Plane::new();
        plane.write("plane.toml", "");
        plane.write(".bitplane/state", "");
        plane.write("projects/web/a.txt", "");
        assert_eq!(
            plane.list("").await,
            [
                dir(".bitplane", false),
                file("plane.toml", false),
                dir("projects", false)
            ]
        );
    }

    #[tokio::test]
    async fn marks_ignored_entries_inside_a_member() {
        let plane = Plane::new();
        plane.repo("projects/web");
        plane.write("projects/web/.gitignore", "node_modules\ntarget/\n*.log\n");
        plane.write("projects/web/node_modules/x/index.js", "");
        plane.write("projects/web/target/app", "");
        plane.write("projects/web/src/main.rs", "");
        plane.write("projects/web/src/debug.log", "");
        assert_eq!(
            plane.list("projects/web").await,
            [
                dir(".git", true),
                file(".gitignore", false),
                dir("node_modules", true),
                dir("src", false),
                dir("target", true)
            ]
        );
        assert_eq!(
            plane.list("projects/web/src").await,
            [file("debug.log", true), file("main.rs", false)]
        );
    }

    #[tokio::test]
    async fn applies_nested_gitignores() {
        let plane = Plane::new();
        plane.repo("web");
        plane.write("web/app/.gitignore", "dist\n");
        plane.write("web/app/dist/a.js", "");
        plane.write("web/dist/a.js", "");
        assert_eq!(
            plane.list("web/app").await,
            [file(".gitignore", false), dir("dist", true)]
        );
        assert_eq!(plane.list("web").await[2], dir("dist", false));
    }

    #[tokio::test]
    async fn everything_inside_git_is_ignored() {
        let plane = Plane::new();
        plane.repo("web");
        let listed = plane.list("web/.git").await;
        assert!(!listed.is_empty());
        assert!(listed.iter().all(|(_, _, ignored)| *ignored), "{listed:?}");
    }

    #[tokio::test]
    async fn without_git_nothing_is_ignored_but_dot_git() {
        let mut plane = Plane::new();
        plane.repo("web");
        plane.write("web/.gitignore", "a.txt\n");
        plane.write("web/a.txt", "");
        plane.env.insert("PATH".into(), "/nonexistent".into());
        assert_eq!(
            plane.list("web").await,
            [
                dir(".git", true),
                file(".gitignore", false),
                file("a.txt", false)
            ]
        );
    }

    #[tokio::test]
    async fn refuses_paths_that_leave_the_plane_lexically() {
        let plane = Plane::new();
        for relative in ["..", "../x", "/etc", "./x", "missing"] {
            assert!(
                list_dir(plane.root(), relative, &plane.env).await.is_err(),
                "{relative}"
            );
        }
    }

    #[tokio::test]
    async fn follows_symlinks_out_of_the_plane() {
        let plane = Plane::new();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("a.md"), "").unwrap();
        std::os::unix::fs::symlink(outside.path(), plane.root().join("linked")).unwrap();
        assert_eq!(plane.list("").await, [dir("linked", false)]);
        assert_eq!(plane.list("linked").await, [file("a.md", false)]);
    }

    #[tokio::test]
    async fn a_symlinked_folder_inherits_the_links_ignore_status() {
        let plane = Plane::new();
        plane.repo("web");
        plane.write("web/.gitignore", ".notes\n");
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(outside.path().join("plans")).unwrap();
        std::fs::write(outside.path().join("plans/a.md"), "").unwrap();
        for link in ["web/.notes", "web/docs"] {
            std::os::unix::fs::symlink(outside.path(), plane.root().join(link)).unwrap();
        }
        assert_eq!(plane.list("web").await[2], dir(".notes", true));
        assert_eq!(plane.list("web/.notes").await, [dir("plans", true)]);
        assert_eq!(plane.list("web/.notes/plans").await, [file("a.md", true)]);
        assert_eq!(plane.list("web/docs/plans").await, [file("a.md", false)]);
    }

    #[test]
    fn spots_paths_inside_git() {
        assert!(in_git_dir(".git"));
        assert!(in_git_dir("projects/web/.git/refs"));
        assert!(!in_git_dir("projects/web/.github"));
    }
}
