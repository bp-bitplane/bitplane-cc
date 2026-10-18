//! Running the git CLI (ADR 0006), the one way the backend reads git.

use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};

use tokio::process::Command;

use crate::bp;
use crate::env::EnvMap;

pub struct Git {
    program: PathBuf,
    env: EnvMap,
}

impl Git {
    /// `None` when git isn't on the captured `PATH`.
    pub fn find(env: &EnvMap) -> Option<Git> {
        let program = bp::find_on_path("git", env.get("PATH").map(String::as_str))?;
        Some(Git {
            program,
            env: bp::git_env(env),
        })
    }

    /// Only the captured environment, no stdin, output piped, killed when
    /// dropped so an abandoned read doesn't linger.
    pub fn command(&self, dir: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(args)
            .current_dir(dir)
            .env_clear()
            .envs(&self.env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        command
    }

    /// Stdout of a run that exits 0 or with one of `also_ok`.
    pub async fn run(&self, dir: &Path, args: &[&str], also_ok: &[i32]) -> Result<Vec<u8>, String> {
        let output = self
            .command(dir, args)
            .output()
            .await
            .map_err(|e| format!("couldn't run git: {e}"))?;
        let ok = output.status.success()
            || output
                .status
                .code()
                .is_some_and(|code| also_ok.contains(&code));
        if ok {
            Ok(output.stdout)
        } else {
            Err(failure(&output))
        }
    }

    /// Whether a run exits 0; any failure to run counts as no.
    pub async fn succeeds(&self, dir: &Path, args: &[&str]) -> bool {
        self.command(dir, args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .is_ok_and(|status| status.success())
    }
}

/// Git's own stderr, or the exit status when it said nothing.
pub fn failure(output: &Output) -> String {
    match String::from_utf8_lossy(&output.stderr).trim() {
        "" => format!("git exited with {}", output.status),
        message => message.to_string(),
    }
}
