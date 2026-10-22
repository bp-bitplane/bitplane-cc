//! The Claude Code hooks that write agent links (ADR 0005), merged into
//! `~/.claude/settings.json` next to whatever hooks are already there.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use specta::Type;

use crate::persist::write_atomic;

/// Identifies our entries, so reinstalling replaces them and leaves others.
const MARKER: &str = "# BITPLANE_CC v2";
const MARKER_PREFIX: &str = "# BITPLANE_CC";

// `$PPID` is the claude process. Every claude started in the tab inherits the
// tab's id, so the first live one owns the link and a nested `claude -p` (say,
// from the agent's own Bash tool) neither replaces nor deletes it.
const WRITE_LINK: &str = r#"if [ -n "$BITPLANE_SESSION_ID" ] && [ -n "$BITPLANE_CC_AGENTS" ]; then f="$BITPLANE_CC_AGENTS/$BITPLANE_SESSION_ID"; o=$(cat "$f.pid" 2>/dev/null); if [ -n "$o" ] && [ "$o" != "$PPID" ] && kill -0 "$o" 2>/dev/null; then cat > /dev/null; else cat > "$f.json.tmp" && mv "$f.json.tmp" "$f.json" && echo "$PPID" > "$f.pid"; fi; else cat > /dev/null; fi"#;
// A claude ends either because the user exited it or because its tab died
// with the app; only the first drops the link. An app quitting on its own
// terms marks `.quitting` before it hangs up, any other app death leaves
// `$BITPLANE_CC_PID` gone or a zombie by the time the PTY closes.
const REMOVE_LINK: &str = r#"cat > /dev/null; if [ -n "$BITPLANE_SESSION_ID" ] && [ -n "$BITPLANE_CC_AGENTS" ] && [ -n "$BITPLANE_CC_PID" ] && [ ! -e "$BITPLANE_CC_AGENTS/.quitting" ]; then case "$(ps -o stat= -p "$BITPLANE_CC_PID" 2>/dev/null)" in ""|*Z*) ;; *) f="$BITPLANE_CC_AGENTS/$BITPLANE_SESSION_ID"; o=$(cat "$f.pid" 2>/dev/null); if [ -z "$o" ] || [ "$o" = "$PPID" ]; then rm -f "$f.json" "$f.pid"; fi ;; esac; fi"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum HookStatus {
    Installed,
    Missing,
    /// Installed by an older version of the app.
    Outdated,
}

/// What the "Enable agent resume" dialog shows before installing.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentHooks {
    pub path: String,
    pub status: HookStatus,
    /// The `hooks` entries that get merged in, as JSON.
    pub entries: String,
}

pub fn settings_path(home: &Path) -> PathBuf {
    home.join(".claude").join("settings.json")
}

pub fn describe(path: &Path) -> Result<AgentHooks, String> {
    let settings = read(path)?;
    Ok(AgentHooks {
        path: path.to_string_lossy().into_owned(),
        status: status(&settings),
        entries: serde_json::to_string_pretty(&Value::Object(ours())).expect("valid JSON"),
    })
}

/// Merge our hooks in, replacing earlier versions of them. Idempotent.
pub fn install(path: &Path) -> Result<(), String> {
    let mut settings = read(path)?;
    let root = settings
        .as_object_mut()
        .ok_or("the settings file isn't a JSON object")?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or("`hooks` in the settings file isn't an object")?;
    for (event, entry) in ours() {
        let groups = hooks
            .entry(event.clone())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| format!("`hooks.{event}` in the settings file isn't a list"))?;
        remove_ours(groups);
        groups.extend(entry.as_array().into_iter().flatten().cloned());
    }
    let mut json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    json.push('\n');
    write_atomic(path, json.as_bytes())
        .map_err(|e| format!("couldn't write {}: {e}", path.display()))
}

fn read(path: &Path) -> Result<Value, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| format!("{} isn't valid JSON: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(format!("couldn't read {}: {e}", path.display())),
    }
}

fn status(settings: &Value) -> HookStatus {
    let commands = |event: &str| -> Vec<String> {
        settings
            .pointer(&format!("/hooks/{event}"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(group_commands)
            .filter(|c| c.contains(MARKER_PREFIX))
            .collect()
    };
    let wanted = ours();
    let found: Vec<(Vec<String>, String)> = wanted
        .iter()
        .map(|(event, entry)| {
            let expected = entry
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(group_commands)
                .collect::<String>();
            (commands(event), expected)
        })
        .collect();
    if found
        .iter()
        .all(|(c, expected)| c.len() == 1 && &c[0] == expected)
    {
        HookStatus::Installed
    } else if found.iter().all(|(c, _)| c.is_empty()) {
        HookStatus::Missing
    } else {
        HookStatus::Outdated
    }
}

fn ours() -> Map<String, Value> {
    let entry = |command: &str| {
        json!([{
            "hooks": [{ "type": "command", "command": format!("{command} {MARKER}") }]
        }])
    };
    let mut map = Map::new();
    for event in ["SessionStart", "UserPromptSubmit", "Stop"] {
        map.insert(event.into(), entry(WRITE_LINK));
    }
    map.insert("SessionEnd".into(), entry(REMOVE_LINK));
    map
}

/// Drop our commands, then any group they leave empty.
fn remove_ours(groups: &mut Vec<Value>) {
    for group in groups.iter_mut() {
        if let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) {
            hooks.retain(|hook| !is_ours(hook));
        }
    }
    groups.retain(|group| {
        group
            .get("hooks")
            .and_then(Value::as_array)
            .is_none_or(|hooks| !hooks.is_empty())
    });
}

fn group_commands(group: &Value) -> Vec<String> {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|hook| hook.get("command")?.as_str().map(String::from))
        .collect()
}

fn is_ours(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| c.contains(MARKER_PREFIX))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn foreign() -> Value {
        json!({ "hooks": [{ "type": "command", "command": "notify-me" }] })
    }

    #[test]
    fn installs_into_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(dir.path());
        assert_eq!(describe(&path).unwrap().status, HookStatus::Missing);
        install(&path).unwrap();
        assert_eq!(describe(&path).unwrap().status, HookStatus::Installed);
    }

    #[test]
    fn keeps_foreign_settings_and_hooks_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let before = json!({
            "model": "opus",
            "hooks": { "Stop": [foreign()], "PreToolUse": [foreign()] },
        });
        std::fs::write(&path, before.to_string()).unwrap();

        install(&path).unwrap();
        let once = std::fs::read_to_string(&path).unwrap();
        install(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), once);

        let after: Value = serde_json::from_str(&once).unwrap();
        assert_eq!(after["model"], "opus");
        assert_eq!(after["hooks"]["PreToolUse"], json!([foreign()]));
        assert_eq!(after["hooks"]["Stop"][0], foreign());
        assert_eq!(after["hooks"]["Stop"].as_array().unwrap().len(), 2);
        assert_eq!(status(&after), HookStatus::Installed);
        assert_eq!(
            after.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["model", "hooks"]
        );
    }

    #[test]
    fn replaces_an_older_version() {
        let old = json!({ "hooks": { "Stop": [{
            "hooks": [{ "type": "command", "command": "true # BITPLANE_CC v0" }]
        }] } });
        assert_eq!(status(&old), HookStatus::Outdated);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, old.to_string()).unwrap();
        install(&path).unwrap();
        let after: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(status(&after), HookStatus::Installed);
        assert_eq!(after["hooks"]["Stop"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn refuses_settings_it_cannot_merge_into() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert!(install(&path).is_err());
        std::fs::write(&path, r#"{"hooks":[]}"#).unwrap();
        assert!(install(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"hooks":[]}"#);
    }

    #[test]
    fn hook_commands_write_and_remove_links() {
        let dir = tempfile::tempdir().unwrap();
        let agents = dir.path().to_str().unwrap();
        let app = std::process::id().to_string();
        let env = [
            ("BITPLANE_SESSION_ID", "k1"),
            ("BITPLANE_CC_AGENTS", agents),
            ("BITPLANE_CC_PID", app.as_str()),
        ];
        let link = dir.path().join("k1.json");

        run(WRITE_LINK, "{\"a\":1}", &[]);
        assert!(!link.exists());
        run(WRITE_LINK, "{\"a\":1}", &env);
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "{\"a\":1}");

        let owner = dir.path().join("k1.pid");
        assert_eq!(
            std::fs::read_to_string(&owner).unwrap().trim(),
            std::process::id().to_string()
        );

        crate::agents::mark_quitting(dir.path());
        run(REMOVE_LINK, "{}", &env);
        assert!(link.exists());
        std::fs::remove_file(dir.path().join(crate::agents::QUITTING_MARKER)).unwrap();

        let mut nested_owner = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        std::fs::write(&owner, nested_owner.id().to_string()).unwrap();
        run(WRITE_LINK, "{\"b\":2}", &env);
        run(REMOVE_LINK, "{}", &env);
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "{\"a\":1}");
        nested_owner.kill().unwrap();
        nested_owner.wait().unwrap();

        run(WRITE_LINK, "{\"b\":2}", &env);
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "{\"b\":2}");
        run(REMOVE_LINK, "{}", &env);
        assert!(!link.exists());
        assert!(!owner.exists());
    }

    #[test]
    fn remove_link_keeps_the_link_once_the_app_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let agents = dir.path().to_str().unwrap();
        let link = dir.path().join("k1.json");
        let remove_with_app = |app: &str| {
            std::fs::write(&link, "{}").unwrap();
            std::fs::write(dir.path().join("k1.pid"), std::process::id().to_string()).unwrap();
            run(
                REMOVE_LINK,
                "{}",
                &[
                    ("BITPLANE_SESSION_ID", "k1"),
                    ("BITPLANE_CC_AGENTS", agents),
                    ("BITPLANE_CC_PID", app),
                ],
            );
            link.exists()
        };

        let mut exited = std::process::Command::new("true").spawn().unwrap();
        let zombie = exited.id().to_string();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &zombie])
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains('Z'))
        {
            assert!(
                std::time::Instant::now() < deadline,
                "no zombie to test with"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(remove_with_app(&zombie), "the app exiting hangs up the tab");
        exited.wait().unwrap();
        assert!(remove_with_app(&zombie), "the app is gone");
        assert!(remove_with_app(""), "a shell the app didn't start");
        assert!(!remove_with_app(&std::process::id().to_string()));
    }

    fn run(command: &str, input: &str, env: &[(&str, &str)]) {
        use std::io::Write;
        let mut child = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("{command} {MARKER}"))
            .env_remove("BITPLANE_SESSION_ID")
            .env_remove("BITPLANE_CC_AGENTS")
            .env_remove("BITPLANE_CC_PID")
            .envs(env.iter().copied())
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success());
    }
}
