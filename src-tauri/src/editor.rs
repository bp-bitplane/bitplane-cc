//! Opening a plane in an external editor.

use std::path::Path;

use tokio::process::Command;

use crate::bp;
use crate::env::EnvMap;

/// Run `code <dir>`, found on the captured `PATH`.
pub async fn open_in_vscode(dir: &Path, env: &EnvMap) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("{} no longer exists", dir.display()));
    }
    let program = bp::find_on_path("code", env.get("PATH").map(String::as_str)).ok_or(
        "VS Code's `code` command isn't on your PATH. In VS Code, run \"Shell Command: Install 'code' command in PATH\".",
    )?;
    let output = Command::new(&program)
        .arg(dir)
        .env_clear()
        .envs(env)
        .current_dir(dir)
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| format!("couldn't run {}: {e}", program.display()))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(match stderr.trim() {
        "" => format!("`code` exited with {}", output.status),
        message => message.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn fake_code(bin: &Path, script: &str) {
        let path = bin.join("code");
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn env_with_path(bin: &Path) -> EnvMap {
        EnvMap::from([("PATH".into(), format!("{}:/bin:/usr/bin", bin.display()))])
    }

    #[tokio::test]
    async fn runs_code_with_the_plane_root() {
        let bin = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let record = bin.path().join("args");
        fake_code(bin.path(), &format!("echo \"$1\" > {}", record.display()));
        open_in_vscode(root.path(), &env_with_path(bin.path()))
            .await
            .unwrap();
        let args = std::fs::read_to_string(record).unwrap();
        assert_eq!(args.trim(), root.path().display().to_string());
    }

    #[tokio::test]
    async fn reports_a_missing_code_command() {
        let bin = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let error = open_in_vscode(root.path(), &env_with_path(bin.path()))
            .await
            .unwrap_err();
        assert!(error.contains("isn't on your PATH"), "{error}");
    }

    #[tokio::test]
    async fn reports_code_failing() {
        let bin = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        fake_code(bin.path(), "echo boom >&2; exit 3");
        let error = open_in_vscode(root.path(), &env_with_path(bin.path()))
            .await
            .unwrap_err();
        assert_eq!(error, "boom");
    }

    #[tokio::test]
    async fn refuses_a_missing_root() {
        let bin = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let gone = root.path().join("gone");
        let error = open_in_vscode(&gone, &env_with_path(bin.path()))
            .await
            .unwrap_err();
        assert!(error.contains("no longer exists"), "{error}");
    }
}
