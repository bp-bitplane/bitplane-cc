//! Files cc writes atomically, and the sessions cc restored itself before the
//! daemon owned them (ADR 0005, superseded by 0028): `<app data>/sessions.json`,
//! which the first launch on the daemon imports (`migrate`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub key: String,
    pub plane_id: String,
    /// Kept because restore runs before `bp list` has said where planes are.
    pub plane_dir: PathBuf,
    pub title: Option<String>,
    pub seq: u32,
}

#[derive(Serialize, Deserialize)]
struct File<E> {
    version: u32,
    sessions: E,
}

/// The entries in creation order; a missing, garbled or newer file is empty.
pub fn load(path: &Path) -> Vec<Entry> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    match serde_json::from_str::<File<Vec<Entry>>>(&text) {
        Ok(file) if file.version == VERSION => {
            let mut sessions = file.sessions;
            sessions.sort_by_key(|e| e.seq);
            sessions
        }
        _ => Vec::new(),
    }
}

/// What is left of an import that stopped part way.
pub fn store(path: &Path, sessions: &[Entry]) -> std::io::Result<()> {
    let file = File {
        version: VERSION,
        sessions,
    };
    write_atomic(path, &serde_json::to_vec(&file).expect("entries serialize"))
}

/// A crash mid-write leaves the previous file, never a torn one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, seq: u32) -> Entry {
        Entry {
            key: key.into(),
            plane_id: "alpha".into(),
            plane_dir: "/planes/alpha".into(),
            title: Some("claude".into()),
            seq,
        }
    }

    #[test]
    fn loads_in_creation_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data/sessions.json");
        store(&path, &[entry("b", 2), entry("a", 1)]).unwrap();
        assert_eq!(load(&path), [entry("a", 1), entry("b", 2)]);
        assert!(!dir.path().join("data/sessions.json.tmp").exists());
    }

    #[test]
    fn unreadable_files_load_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions.json");
        assert!(load(&path).is_empty());
        std::fs::write(&path, "{ torn").unwrap();
        assert!(load(&path).is_empty());
        std::fs::write(&path, r#"{"version":2,"sessions":[]}"#).unwrap();
        assert!(load(&path).is_empty());
    }
}
