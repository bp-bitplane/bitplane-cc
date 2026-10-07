//! The two environments (spec §6.1).
//!
//! A GUI app launched from the Dock doesn't inherit the user's shell setup, so
//! bp and git run in an environment captured from a login + interactive shell.
//! Sessions get the app's **original** environment instead: their login shell
//! builds its own, and passing the captured one would source rc files twice.

use std::collections::BTreeMap;
use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

pub type EnvMap = BTreeMap<String, String>;

/// How long the shell may take to print its environment.
pub const CAPTURE_TIMEOUT: Duration = Duration::from_secs(10);

const MARKER: &str = "__BITPLANE_CC_ENV_7f3a__";

/// The environment bp and git calls run in.
#[derive(Debug, Clone)]
pub struct CapturedEnv {
    pub vars: EnvMap,
    /// Set when capturing failed and `vars` is just the app's own environment.
    pub warning: Option<String>,
}

/// The app's own environment, as it was at startup.
pub fn original() -> EnvMap {
    std::env::vars_os()
        .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
        .collect()
}

/// Capture the login + interactive shell environment, merged over `base`.
///
/// Never fails: on error or timeout it falls back to `base` with a warning.
pub async fn capture(base: &EnvMap, timeout: Duration) -> CapturedEnv {
    let shell = resolve_shell(base.get("SHELL").map(String::as_str));
    match run_capture(&shell, base, timeout).await {
        Ok(captured) => {
            let mut vars = base.clone();
            vars.extend(captured);
            CapturedEnv {
                vars,
                warning: None,
            }
        }
        Err(error) => CapturedEnv {
            vars: base.clone(),
            warning: Some(format!(
                "Couldn't read your shell environment ({error}); using the app's own"
            )),
        },
    }
}

/// `$SHELL`, then the account database's login shell, then `/bin/sh`.
pub fn resolve_shell(shell_var: Option<&str>) -> PathBuf {
    if let Some(shell) = shell_var.filter(|s| !s.is_empty()) {
        return PathBuf::from(shell);
    }
    login_shell_from_passwd().unwrap_or_else(|| PathBuf::from("/bin/sh"))
}

/// The basename of a shell path, used as the fallback tab title.
pub fn shell_name(shell: &Path) -> String {
    shell
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "shell".into())
}

/// Parse `env -0` output printed between two markers.
///
/// rc files may print banners before or after, so only the text between the
/// first two markers counts.
pub fn parse_env_output(output: &[u8]) -> Option<EnvMap> {
    let text = String::from_utf8_lossy(output);
    let start = text.find(MARKER)? + MARKER.len();
    let len = text[start..].find(MARKER)?;
    let body = &text[start..start + len];
    Some(
        body.split('\0')
            .filter_map(|entry| {
                let (key, value) = entry.split_once('=')?;
                (!key.is_empty() && !key.contains('\n')).then(|| (key.into(), value.into()))
            })
            .collect(),
    )
}

async fn run_capture(shell: &Path, base: &EnvMap, timeout: Duration) -> Result<EnvMap, String> {
    let script = format!("printf '%s' '{MARKER}'; env -0; printf '%s' '{MARKER}'");
    let mut command = Command::new(shell);
    command
        .args(["-l", "-i", "-c", &script])
        .env_clear()
        .envs(base)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    detach_from_terminal(&mut command);

    let output = tokio::time::timeout(timeout, command.output())
        .await
        .map_err(|_| format!("timed out after {}s", timeout.as_secs()))?
        .map_err(|e| e.to_string())?;
    parse_env_output(&output.stdout).ok_or_else(|| "no environment in shell output".into())
}

/// Start the child in a new session, so it has no controlling terminal and can
/// never prompt on one (spec §6.3).
pub fn detach_from_terminal(command: &mut Command) {
    // SAFETY: setsid is async-signal-safe and touches no parent state.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

fn login_shell_from_passwd() -> Option<PathBuf> {
    // SAFETY: getpwuid returns a pointer into static storage or null; the
    // shell string is copied out before anything else can call it.
    unsafe {
        let entry = libc::getpwuid(libc::getuid());
        if entry.is_null() || (*entry).pw_shell.is_null() {
            return None;
        }
        let shell = CStr::from_ptr((*entry).pw_shell).to_str().ok()?;
        (!shell.is_empty()).then(|| PathBuf::from(shell))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_env_between_markers_ignoring_banners() {
        let output = format!("motd\n{MARKER}A=1\0PATH=/bin:/usr/bin\0MULTI=x\ny\0{MARKER}bye");
        let env = parse_env_output(output.as_bytes()).unwrap();
        assert_eq!(env["A"], "1");
        assert_eq!(env["PATH"], "/bin:/usr/bin");
        assert_eq!(env["MULTI"], "x\ny");
    }

    #[test]
    fn keeps_equals_signs_in_values() {
        let output = format!("{MARKER}OPTS=a=b=c\0{MARKER}");
        assert_eq!(
            parse_env_output(output.as_bytes()).unwrap()["OPTS"],
            "a=b=c"
        );
    }

    #[test]
    fn missing_markers_is_none() {
        assert!(parse_env_output(b"A=1\0").is_none());
        assert!(parse_env_output(format!("{MARKER}A=1\0").as_bytes()).is_none());
    }

    #[test]
    fn shell_falls_back_when_unset() {
        assert_eq!(resolve_shell(Some("/bin/zsh")), PathBuf::from("/bin/zsh"));
        let fallback = resolve_shell(Some(""));
        assert!(fallback.is_absolute());
        assert_eq!(resolve_shell(None), fallback);
    }

    #[test]
    fn shell_name_is_basename() {
        assert_eq!(shell_name(Path::new("/opt/homebrew/bin/fish")), "fish");
    }

    #[tokio::test]
    async fn captures_from_a_real_shell() {
        let mut base = original();
        base.insert("SHELL".into(), "/bin/sh".into());
        base.insert("BITPLANE_CC_PROBE".into(), "yes".into());
        let captured = capture(&base, CAPTURE_TIMEOUT).await;
        assert_eq!(captured.warning, None);
        assert_eq!(captured.vars["BITPLANE_CC_PROBE"], "yes");
    }

    #[tokio::test]
    async fn falls_back_with_warning_when_shell_is_missing() {
        let mut base = EnvMap::new();
        base.insert("SHELL".into(), "/nonexistent/shell".into());
        let captured = capture(&base, CAPTURE_TIMEOUT).await;
        assert!(captured.warning.is_some());
        assert_eq!(captured.vars, base);
    }
}
