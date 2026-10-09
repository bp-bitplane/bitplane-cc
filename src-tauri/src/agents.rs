//! Agent links (ADR 0005): files the Claude Code hooks in `hooks.rs` write to
//! `<agents dir>/<session key>.json`, holding the hook's stdin, next to a
//! `.pid` naming the claude process that owns the link.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Present while the app quits, so `SessionEnd` hooks fired by the hang-up
/// keep their links.
pub const QUITTING_MARKER: &str = ".quitting";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLink {
    pub session_id: String,
    /// Where the agent session started; `claude --resume` only finds it there.
    pub cwd: PathBuf,
    /// A turn was in flight when the last hook ran.
    pub working: bool,
}

impl AgentLink {
    /// What restore types into the fresh shell.
    pub fn resume_command(&self) -> String {
        format!("claude --resume {}\r", self.session_id)
    }

    /// Restore resumes only from a directory that still exists inside the
    /// plane; quitting must agree, or it would skip asking for nothing.
    pub fn resumable_in(&self, plane_dir: &Path) -> bool {
        match (self.cwd.canonicalize(), plane_dir.canonicalize()) {
            (Ok(cwd), Ok(root)) => cwd.is_dir() && cwd.starts_with(root),
            _ => false,
        }
    }
}

pub fn read(dir: &Path, key: &str) -> Option<AgentLink> {
    let text = std::fs::read_to_string(link_path(dir, key)?).ok()?;
    parse(&text)
}

pub fn remove(dir: &Path, key: &str) {
    if let Some(path) = link_path(dir, key) {
        let _ = std::fs::remove_file(path.with_extension("pid"));
        let _ = std::fs::remove_file(path);
    }
}

/// Delete links of sessions that weren't restored, and a stale quit marker.
pub fn sweep(dir: &Path, keep: &HashSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let stale = match name
            .strip_suffix(".json")
            .or_else(|| name.strip_suffix(".pid"))
        {
            Some(key) => !keep.contains(key),
            None => name == QUITTING_MARKER || name.ends_with(".json.tmp"),
        };
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

pub fn mark_quitting(dir: &Path) {
    let _ = std::fs::write(dir.join(QUITTING_MARKER), b"");
}

#[derive(Deserialize)]
struct HookInput {
    session_id: String,
    cwd: PathBuf,
    hook_event_name: Option<String>,
}

fn parse(text: &str) -> Option<AgentLink> {
    let input: HookInput = serde_json::from_str(text).ok()?;
    // Typed into a shell, so nothing but an id may get through.
    let safe = !input.session_id.is_empty()
        && input
            .session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-');
    (safe && input.cwd.is_absolute()).then(|| AgentLink {
        session_id: input.session_id,
        cwd: input.cwd,
        working: input.hook_event_name.as_deref() == Some("UserPromptSubmit"),
    })
}

fn link_path(dir: &Path, key: &str) -> Option<PathBuf> {
    let safe = !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    safe.then(|| dir.join(format!("{key}.json")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0b7c1a6e-3f1d-4c55-9d6a-2b1f0e9c8a77";

    fn hook(event: &str, session_id: &str) -> String {
        serde_json::json!({
            "session_id": session_id,
            "transcript_path": "/tmp/t.jsonl",
            "cwd": "/planes/alpha/api",
            "hook_event_name": event,
            "source": "startup",
        })
        .to_string()
    }

    #[test]
    fn parses_hook_input() {
        assert_eq!(
            parse(&hook("Stop", ID)),
            Some(AgentLink {
                session_id: ID.into(),
                cwd: "/planes/alpha/api".into(),
                working: false,
            })
        );
        assert!(parse(&hook("UserPromptSubmit", ID)).unwrap().working);
        assert_eq!(
            parse(&hook("SessionStart", ID)).unwrap().resume_command(),
            format!("claude --resume {ID}\r")
        );
    }

    #[test]
    fn rejects_garbage_and_unsafe_ids() {
        assert_eq!(parse("not json"), None);
        assert_eq!(parse("{}"), None);
        assert_eq!(parse(&hook("Stop", "x; rm -rf ~")), None);
        assert_eq!(parse(&hook("Stop", "")), None);
    }

    #[test]
    fn reads_removes_and_sweeps() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        std::fs::write(dir.join("keep.json"), hook("Stop", ID)).unwrap();
        std::fs::write(dir.join("keep.pid"), "1").unwrap();
        std::fs::write(dir.join("drop.json"), hook("Stop", ID)).unwrap();
        std::fs::write(dir.join("drop.pid"), "1").unwrap();
        std::fs::write(dir.join("drop.json.tmp"), "").unwrap();
        mark_quitting(dir);

        assert!(read(dir, "keep").is_some());
        assert_eq!(read(dir, "../keep"), None);
        sweep(dir, &HashSet::from(["keep".to_string()]));
        let mut left: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, ["keep.json", "keep.pid"]);

        remove(dir, "keep");
        assert_eq!(std::fs::read_dir(dir).unwrap().count(), 0);
    }
}
