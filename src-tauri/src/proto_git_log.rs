//! PROTOTYPE (alfonz/proto-git-graph): runs `git log` in cc instead of going
//! through sessiond's `git.*` protocol. The real build moves this to a
//! `git.log` method in bitplane-protocol + sessiond.

use std::path::Path;

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::process::Command;

use crate::bp;
use crate::env::EnvMap;
use crate::git;

pub const LIMIT: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LogCommit {
    pub hash: String,
    pub parents: Vec<String>,
    /// `%D`, split: `HEAD -> main`, `origin/main`, `tag: v1`, …
    pub refs: Vec<String>,
    pub author: String,
    /// Unix seconds.
    pub time: u32,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GitLog {
    pub commits: Vec<LogCommit>,
    /// More commits exist past `LIMIT`.
    pub truncated: bool,
}

pub async fn log(
    root: &Path,
    members: &[std::path::PathBuf],
    repo: &str,
    env: &EnvMap,
) -> Result<GitLog, String> {
    let dir = git::repo_dirs(root, members)
        .into_iter()
        .find(|dir| dir.to_string_lossy() == repo)
        .ok_or_else(|| format!("{repo} isn't a repo of this plane"))?;
    let program = bp::find_on_path("git", env.get("PATH").map(String::as_str))
        .ok_or("git isn't on your PATH")?;
    let output = Command::new(&program)
        .args([
            "-c",
            "log.showSignature=false",
            "log",
            "--all",
            "--date-order",
            "--no-color",
            "-z",
            "--format=%H%x1f%P%x1f%D%x1f%an%x1f%at%x1f%s",
            &format!("-n{}", LIMIT + 1),
        ])
        .env_clear()
        .envs(env)
        .current_dir(&dir)
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| format!("couldn't run git: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut commits: Vec<LogCommit> = text
        .split('\0')
        .filter(|record| !record.trim().is_empty())
        .filter_map(|record| {
            let mut f = record.trim_start_matches('\n').split('\x1f');
            Some(LogCommit {
                hash: f.next()?.to_string(),
                parents: f.next()?.split_whitespace().map(String::from).collect(),
                refs: f
                    .next()?
                    .split(", ")
                    .filter(|r| !r.is_empty())
                    .map(String::from)
                    .collect(),
                author: f.next()?.to_string(),
                time: f.next()?.parse().unwrap_or(0),
                subject: f.next().unwrap_or("").to_string(),
            })
        })
        .collect();
    let truncated = commits.len() > LIMIT;
    commits.truncate(LIMIT);
    Ok(GitLog { commits, truncated })
}
