//! The first launch on the daemon imports what cc restored itself before
//! (ADR 0005, superseded by 0028): the tabs in `sessions.json`, the agent
//! links its hooks wrote next to it, and those hooks in
//! `~/.claude/settings.json`, which the daemon's own hooks replace.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::persist::{self, Entry};
use crate::sessions::{OpenOptions, SessionError, Sessions};

/// What cc's hooks marked their entries with, in every version.
const HOOK_MARKER: &str = "# BITPLANE_CC";

/// Where the pre-daemon cc kept its state.
#[derive(Debug, Clone)]
pub struct Legacy {
    pub sessions: PathBuf,
    /// `<session key>.json`: the hook input that last named the tab's agent.
    pub agents: PathBuf,
    /// `~/.claude/settings.json`; `None` without a `HOME`.
    pub claude_settings: Option<PathBuf>,
}

/// Open a session per old tab, in creation order, resuming its agent, then
/// delete the old files. A daemon error stops the import and keeps the tabs
/// not yet opened for the next launch. cc's hooks are removed either way.
pub async fn import(legacy: &Legacy, sessions: &Sessions) {
    if let Some(settings) = &legacy.claude_settings
        && let Err(error) = remove_hooks(settings)
    {
        eprintln!("bitplane-cc: couldn't remove cc's agent hooks: {error}");
    }
    let entries = persist::load(&legacy.sessions);
    for (index, entry) in entries.iter().enumerate() {
        let resume = read_link(&legacy.agents, &entry.key);
        if let Err(error) = open(sessions, entry, resume).await {
            eprintln!("bitplane-cc: stopped importing cc's old tabs: {error}");
            if index > 0
                && let Err(error) = persist::store(&legacy.sessions, &entries[index..])
            {
                eprintln!("bitplane-cc: couldn't keep the tabs left to import: {error}");
            }
            return;
        }
    }
    let _ = std::fs::remove_file(&legacy.sessions);
    let _ = std::fs::remove_dir_all(&legacy.agents);
}

/// Drop cc's entries from a Claude Code settings file, keeping everything
/// else as it was. The file is written only when there was one.
pub fn remove_hooks(path: &Path) -> Result<(), String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("couldn't read {}: {e}", path.display())),
    };
    if !text.contains(HOOK_MARKER) {
        return Ok(());
    }
    let mut settings: Value = serde_json::from_str(&text)
        .map_err(|e| format!("{} isn't valid JSON: {e}", path.display()))?;
    let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    let mut changed = false;
    hooks.retain(|_, groups| {
        let Some(groups) = groups.as_array_mut() else {
            return true;
        };
        let before = groups.len();
        groups.retain_mut(|group| !drop_ours(group, &mut changed));
        !(groups.is_empty() && before > 0)
    });
    if !changed {
        return Ok(());
    }
    if hooks.is_empty()
        && let Some(root) = settings.as_object_mut()
    {
        root.remove("hooks");
    }
    let mut json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    json.push('\n');
    persist::write_atomic(path, json.as_bytes())
        .map_err(|e| format!("couldn't write {}: {e}", path.display()))
}

async fn open(
    sessions: &Sessions,
    entry: &Entry,
    resume: Option<String>,
) -> Result<(), SessionError> {
    let options = |resume| OpenOptions {
        name: entry.title.clone(),
        resume,
    };
    let opened = sessions
        .open(&entry.plane_id, &entry.plane_dir, options(resume))
        .await;
    match opened {
        Ok(_) | Err(SessionError::RootMissing(_)) => Ok(()),
        Err(SessionError::AgentLive) => sessions
            .open(&entry.plane_id, &entry.plane_dir, options(None))
            .await
            .map(drop),
        Err(error) => Err(error),
    }
}

/// Drops cc's hooks from one group; true when that leaves it empty.
fn drop_ours(group: &mut Value, changed: &mut bool) -> bool {
    let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
        return false;
    };
    let before = hooks.len();
    hooks.retain(|hook| {
        !hook
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|command| command.contains(HOOK_MARKER))
    });
    let dropped = hooks.len() < before;
    *changed |= dropped;
    dropped && hooks.is_empty()
}

/// The agent session a tab's hooks last reported, if it's safe to resume.
fn read_link(dir: &Path, key: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct HookInput {
        session_id: String,
    }
    if !is_id(key) {
        return None;
    }
    let text = std::fs::read_to_string(dir.join(format!("{key}.json"))).ok()?;
    let input: HookInput = serde_json::from_str(&text).ok()?;
    is_id(&input.session_id).then_some(input.session_id)
}

fn is_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const ID: &str = "0b7c1a6e-3f1d-4c55-9d6a-2b1f0e9c8a77";

    fn ours(version: &str) -> Value {
        json!({ "hooks": [{ "type": "command", "command": format!("cat > /dev/null {HOOK_MARKER} {version}") }] })
    }

    fn foreign() -> Value {
        json!({ "hooks": [{ "type": "command", "command": "notify-me" }] })
    }

    #[test]
    fn removes_only_ccs_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mixed = json!({ "hooks": [
            { "type": "command", "command": "notify-me" },
            { "type": "command", "command": format!("true {HOOK_MARKER} v2") },
        ] });
        let before = json!({
            "model": "opus",
            "hooks": {
                "SessionStart": [ours("v2")],
                "Stop": [foreign(), ours("v2")],
                "UserPromptSubmit": [mixed],
                "SessionEnd": [ours("v1")],
                "PreToolUse": [foreign()],
                "Notification": [],
            },
        });
        std::fs::write(&path, before.to_string()).unwrap();

        remove_hooks(&path).unwrap();

        let after: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            after,
            json!({
                "model": "opus",
                "hooks": {
                    "Stop": [foreign()],
                    "UserPromptSubmit": [foreign()],
                    "PreToolUse": [foreign()],
                    "Notification": [],
                },
            })
        );
    }

    #[test]
    fn drops_hooks_it_emptied_and_leaves_other_files_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            json!({ "hooks": { "Stop": [ours("v2")] } }).to_string(),
        )
        .unwrap();
        remove_hooks(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}\n");

        let untouched = r#"{"hooks":{"Stop":[]},  "model":"opus"}"#;
        std::fs::write(&path, untouched).unwrap();
        remove_hooks(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), untouched);

        remove_hooks(&dir.path().join("missing.json")).unwrap();
        assert!(!dir.path().join("missing.json").exists());

        std::fs::write(&path, format!("{{ torn {HOOK_MARKER}")).unwrap();
        assert!(remove_hooks(&path).is_err());
    }

    #[test]
    fn reads_only_safe_links() {
        let dir = tempfile::tempdir().unwrap();
        let link = |id: &str| json!({ "session_id": id, "cwd": "/p", "hook_event_name": "Stop" });
        std::fs::write(dir.path().join("k1.json"), link(ID).to_string()).unwrap();
        std::fs::write(dir.path().join("k2.json"), link("x; rm -rf ~").to_string()).unwrap();
        std::fs::write(dir.path().join("k3.json"), "not json").unwrap();

        assert_eq!(read_link(dir.path(), "k1").as_deref(), Some(ID));
        assert_eq!(read_link(dir.path(), "k2"), None);
        assert_eq!(read_link(dir.path(), "k3"), None);
        assert_eq!(read_link(dir.path(), "missing"), None);
        assert_eq!(read_link(dir.path(), "../k1"), None);
    }
}
