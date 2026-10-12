//! The bp CLI: one spawner, JSON and envelope parsing, version check
//! (spec §6.3, §6.4; ADR 0002).

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::process::Command;

use crate::env::{EnvMap, detach_from_terminal};

/// The first bp version with the `--json` contract the app relies on.
pub const MIN_VERSION: (u64, u64, u64) = (0, 1, 0);

pub const EXIT_OK: i32 = 0;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_FINDINGS: i32 = 3;
pub const EXIT_GIT: i32 = 4;
pub const EXIT_LOCK_TIMEOUT: i32 = 5;
pub const EXIT_SIGINT: i32 = 130;

/// How the app runs bp: which binary, in which environment, against which
/// planes directory.
#[derive(Debug, Clone)]
pub struct Bp {
    pub program: PathBuf,
    pub env: EnvMap,
    pub planes_dir: PathBuf,
}

/// bp's failure envelope, the last line of stderr.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Envelope {
    pub error: String,
    pub code: i32,
    pub message: String,
    #[serde(default)]
    pub problems: Vec<Problem>,
    #[serde(default)]
    pub remedy: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Problem {
    #[serde(default)]
    pub subject: Option<String>,
    pub message: String,
}

/// What one bp run left behind.
#[derive(Debug, Clone)]
pub struct Output {
    /// `None` when bp was killed by a signal.
    pub code: Option<i32>,
    pub stdout: String,
    /// stderr text before the envelope: lock waits and script output.
    pub log: String,
    pub envelope: Option<Envelope>,
}

/// Why a bp call didn't produce the result the app asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BpError {
    /// bp couldn't be started.
    Spawn(String),
    /// git missing or older than 2.36 (exit 4).
    Git(String),
    /// A response that doesn't parse: bp is incompatible.
    Incompatible(String),
    /// bp reported a failure.
    Failed {
        code: Option<i32>,
        envelope: Option<Envelope>,
    },
}

impl BpError {
    pub fn message(&self) -> String {
        match self {
            BpError::Spawn(message) | BpError::Git(message) | BpError::Incompatible(message) => {
                message.clone()
            }
            BpError::Failed {
                envelope: Some(envelope),
                ..
            } => envelope.message.clone(),
            BpError::Failed { code, .. } => match code {
                Some(code) => format!("bp failed with exit code {code}"),
                None => "bp was killed".into(),
            },
        }
    }
}

/// One plane as `bp list` reports it; only the fields the app reads.
#[derive(Debug, Clone, Deserialize)]
pub struct ListedPlane {
    pub id: String,
    pub directory: PathBuf,
    #[serde(default)]
    pub health: Option<Health>,
    #[serde(default)]
    pub members: Vec<ListedMember>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ListedMember {
    /// `@project` or a repo path, as `bp create` takes it.
    pub member: String,
    /// The worktree, relative to the plane's root.
    #[serde(default)]
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Health {
    /// Finding tags; the enum is `#[non_exhaustive]` upstream, so unknown
    /// tags are kept as text.
    #[serde(default)]
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Finding {
    #[serde(default)]
    pub finding: String,
}

/// One project from `bp project list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub name: String,
    pub source_repo_present: bool,
}

/// `bp create`'s result on stdout.
#[derive(Debug, Clone, Deserialize)]
pub struct PlaneCreated {
    pub id: String,
    pub directory: PathBuf,
    #[serde(default)]
    pub interrupted: bool,
    #[serde(default)]
    pub remnant: bool,
}

impl Bp {
    /// A bp command with `--planes-dir` and `--json`, in the captured
    /// environment, detached from any terminal (spec §6.3).
    pub fn command<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let mut command = Command::new(&self.program);
        command
            .args(args)
            .arg("--planes-dir")
            .arg(&self.planes_dir)
            .arg("--json")
            .env_clear()
            .envs(git_env(&self.env))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        detach_from_terminal(&mut command);
        command
    }

    /// Run bp to completion.
    pub async fn run<I, S>(&self, args: I) -> Result<Output, BpError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let output = self
            .command(args)
            .output()
            .await
            .map_err(|e| BpError::Spawn(format!("couldn't run bp: {e}")))?;
        Ok(finish(
            output.status.code(),
            &output.stdout,
            &String::from_utf8_lossy(&output.stderr),
        ))
    }

    /// `bp --version`, checked against [`MIN_VERSION`].
    pub async fn version(&self) -> Result<String, VersionError> {
        let output = Command::new(&self.program)
            .arg("--version")
            .env_clear()
            .envs(&self.env)
            .stdin(Stdio::null())
            .output()
            .await
            .map_err(|e| VersionError::NotFound(e.to_string()))?;
        let text = String::from_utf8_lossy(&output.stdout);
        let version = parse_version(&text).ok_or_else(|| {
            VersionError::Unparsable(format!("unexpected `bp --version` output: {}", text.trim()))
        })?;
        let found = format_version(version);
        if version < MIN_VERSION {
            return Err(VersionError::TooOld(found));
        }
        Ok(found)
    }

    /// `bp list` at the default `cheap` health level. Exit 3 means findings,
    /// and its stdout is still valid.
    pub async fn list(&self) -> Result<Vec<ListedPlane>, BpError> {
        #[derive(Deserialize)]
        struct PlaneList {
            planes: Vec<ListedPlane>,
        }
        let output = self.run(["list"]).await?;
        let value = expect_success(output, &[EXIT_OK, EXIT_FINDINGS])?;
        let list: PlaneList = parse_action(&value, "plane_list")?;
        Ok(list.planes)
    }

    /// `bp project list`.
    pub async fn projects(&self) -> Result<Vec<Project>, BpError> {
        #[derive(Deserialize)]
        struct ProjectList {
            projects: Vec<ProjectEntry>,
        }
        #[derive(Deserialize)]
        struct ProjectEntry {
            project: String,
            #[serde(default)]
            detail: Option<serde_json::Value>,
        }
        let output = self.run(["project", "list"]).await?;
        let value = expect_success(output, &[EXIT_OK, EXIT_FINDINGS])?;
        let list: ProjectList = parse_action(&value, "project_list")?;
        Ok(list
            .projects
            .into_iter()
            .map(|entry| Project {
                source_repo_present: entry
                    .detail
                    .as_ref()
                    .and_then(|d| d.get("source_repo_present"))
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                name: entry.project,
            })
            .collect())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionError {
    NotFound(String),
    TooOld(String),
    Unparsable(String),
}

/// Find `bp` on a `PATH` value.
pub fn find_on_path(name: &str, path_var: Option<&str>) -> Option<PathBuf> {
    std::env::split_paths(path_var?)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

/// Parse `bp X.Y.Z` (any trailing text ignored).
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let rest = text.trim().strip_prefix("bp ")?;
    let core = rest
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

pub fn format_version((major, minor, patch): (u64, u64, u64)) -> String {
    format!("{major}.{minor}.{patch}")
}

/// Split stderr into log text and the envelope.
///
/// The envelope is on the last line, but script output without a trailing
/// newline can be glued in front of it, so it starts at the last `{"error":`
/// on that line.
pub fn split_stderr(stderr: &str) -> (String, Option<Envelope>) {
    let trimmed = stderr.trim_end_matches(['\n', '\r']);
    let line_start = trimmed.rfind('\n').map_or(0, |i| i + 1);
    let last_line = &trimmed[line_start..];
    if let Some(offset) = last_line.rfind("{\"error\":")
        && let Ok(envelope) = serde_json::from_str::<Envelope>(&last_line[offset..])
    {
        let log = trimmed[..line_start + offset].to_string();
        return (log, Some(envelope));
    }
    (stderr.to_string(), None)
}

/// Assemble an [`Output`] from a finished process.
pub fn finish(code: Option<i32>, stdout: &[u8], stderr: &str) -> Output {
    let (log, envelope) = split_stderr(stderr);
    Output {
        code,
        stdout: String::from_utf8_lossy(stdout).into_owned(),
        log,
        envelope,
    }
}

/// The JSON value on stdout, if the exit code is one of `ok_codes`.
pub fn expect_success(output: Output, ok_codes: &[i32]) -> Result<serde_json::Value, BpError> {
    match output.code {
        Some(code) if ok_codes.contains(&code) => serde_json::from_str(output.stdout.trim())
            .map_err(|e| BpError::Incompatible(format!("bp's output isn't valid JSON: {e}"))),
        Some(EXIT_GIT) => Err(BpError::Git(output.envelope.map_or_else(
            || "bp needs git 2.36 or newer".into(),
            |envelope| envelope.message,
        ))),
        code => Err(BpError::Failed {
            code,
            envelope: output.envelope,
        }),
    }
}

/// Deserialize a response, checking its `action` tag.
pub fn parse_action<T: serde::de::DeserializeOwned>(
    value: &serde_json::Value,
    action: &str,
) -> Result<T, BpError> {
    let found = value.get("action").and_then(serde_json::Value::as_str);
    if found != Some(action) {
        return Err(BpError::Incompatible(format!(
            "expected a `{action}` response, got {}",
            found.map_or_else(|| "no action".into(), |a| format!("`{a}`"))
        )));
    }
    serde_json::from_value(value.clone())
        .map_err(|e| BpError::Incompatible(format!("couldn't read `{action}`: {e}")))
}

/// The captured environment plus the variables that stop git from prompting.
pub fn git_env(env: &EnvMap) -> EnvMap {
    let mut env = env.clone();
    env.insert("GIT_TERMINAL_PROMPT".into(), "0".into());
    let ssh = match env.get("GIT_SSH_COMMAND").filter(|s| !s.trim().is_empty()) {
        Some(theirs) => format!("{theirs} -o BatchMode=yes"),
        None => "ssh -o BatchMode=yes".into(),
    };
    env.insert("GIT_SSH_COMMAND".into(), ssh);
    env
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENVELOPE: &str = r#"{"error":"plane_id_in_use","code":2,"message":"there is already a plane called x","problems":[{"subject":null,"message":"pick another"}],"remedy":"Use another id."}"#;

    #[test]
    fn envelope_on_its_own_line() {
        let stderr = format!("waiting for lock…\nscript says hi\n{ENVELOPE}\n");
        let (log, envelope) = split_stderr(&stderr);
        assert_eq!(log, "waiting for lock…\nscript says hi\n");
        let envelope = envelope.unwrap();
        assert_eq!(envelope.error, "plane_id_in_use");
        assert_eq!(envelope.code, 2);
        assert_eq!(envelope.problems[0].subject, None);
        assert_eq!(envelope.remedy.as_deref(), Some("Use another id."));
    }

    #[test]
    fn envelope_glued_to_script_output() {
        let stderr = format!("line one\nno newline here{ENVELOPE}");
        let (log, envelope) = split_stderr(&stderr);
        assert_eq!(log, "line one\nno newline here");
        assert_eq!(envelope.unwrap().error, "plane_id_in_use");
    }

    #[test]
    fn script_output_mentioning_error_json_earlier_is_log() {
        let stderr = format!("{{\"error\": fake\n{ENVELOPE}\n");
        let (log, envelope) = split_stderr(&stderr);
        assert_eq!(log, "{\"error\": fake\n");
        assert!(envelope.is_some());
    }

    #[test]
    fn no_envelope_keeps_everything_as_log() {
        let (log, envelope) = split_stderr("just output\n");
        assert_eq!(log, "just output\n");
        assert!(envelope.is_none());
        assert!(split_stderr("").1.is_none());
    }

    #[test]
    fn parses_versions() {
        assert_eq!(parse_version("bp 0.1.0\n"), Some((0, 1, 0)));
        assert_eq!(parse_version("bp 1.12.3-beta.1"), Some((1, 12, 3)));
        assert_eq!(parse_version("bitplane 0.1.0"), None);
        assert_eq!(parse_version("bp 0.1"), None);
        assert!(parse_version("bp 0.0.9").unwrap() < MIN_VERSION);
    }

    #[test]
    fn findings_exit_still_parses_stdout() {
        let output = finish(
            Some(EXIT_FINDINGS),
            br#"{"action":"plane_list","planes":[]}"#,
            "",
        );
        assert!(expect_success(output, &[EXIT_OK, EXIT_FINDINGS]).is_ok());
    }

    #[test]
    fn git_exit_is_its_own_error() {
        let stderr = r#"{"error":"git_missing","code":4,"message":"git is missing","problems":[],"remedy":null}"#;
        let output = finish(Some(EXIT_GIT), b"", stderr);
        assert_eq!(
            expect_success(output, &[EXIT_OK]),
            Err(BpError::Git("git is missing".into()))
        );
    }

    #[test]
    fn wrong_action_is_incompatible() {
        let value = serde_json::json!({"action": "plane_show"});
        let result: Result<serde_json::Value, _> = parse_action(&value, "plane_list");
        assert!(matches!(result, Err(BpError::Incompatible(_))));
        let value = serde_json::json!({"action": "plane_list", "planes": 3});
        let result: Result<serde_json::Value, _> = parse_action(&value, "plane_list");
        assert!(result.is_ok(), "only the action tag is checked here");
    }

    #[test]
    fn git_env_disables_prompts_and_appends_batch_mode() {
        let env = git_env(&EnvMap::new());
        assert_eq!(env["GIT_TERMINAL_PROMPT"], "0");
        assert_eq!(env["GIT_SSH_COMMAND"], "ssh -o BatchMode=yes");

        let mut theirs = EnvMap::new();
        theirs.insert("GIT_SSH_COMMAND".into(), "ssh -i ~/.ssh/work".into());
        assert_eq!(
            git_env(&theirs)["GIT_SSH_COMMAND"],
            "ssh -i ~/.ssh/work -o BatchMode=yes"
        );
    }

    #[test]
    fn finds_executables_on_path() {
        let dir = tempfile::tempdir().unwrap();
        let bp = dir.path().join("bp");
        std::fs::write(&bp, "#!/bin/sh\n").unwrap();
        assert_eq!(
            find_on_path("bp", dir.path().to_str()),
            None,
            "not executable yet"
        );
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bp, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = std::env::join_paths(["/nonexistent".as_ref(), dir.path()]).unwrap();
        assert_eq!(find_on_path("bp", path.to_str()), Some(bp));
        assert_eq!(find_on_path("bp", None), None);
    }
}
