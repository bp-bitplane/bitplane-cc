//! What restore reads on launch (ADR 0005): the open sessions in
//! `<app data>/sessions.json`, rewritten shortly after every session change.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

const VERSION: u32 = 1;
pub const DEBOUNCE: Duration = Duration::from_millis(500);

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
struct File {
    version: u32,
    sessions: Vec<Entry>,
}

/// The entries in creation order; a missing, garbled or newer file is empty.
pub fn load(path: &Path) -> Vec<Entry> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    match serde_json::from_str::<File>(&text) {
        Ok(file) if file.version == VERSION => {
            let mut sessions = file.sessions;
            sessions.sort_by_key(|e| e.seq);
            sessions
        }
        _ => Vec::new(),
    }
}

pub fn save(path: &Path, sessions: Vec<Entry>) -> std::io::Result<()> {
    let file = File {
        version: VERSION,
        sessions,
    };
    let json = serde_json::to_vec_pretty(&file).map_err(std::io::Error::other)?;
    write_atomic(path, &json)
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

pub type Collect = Arc<dyn Fn() -> Vec<Entry> + Send + Sync>;

/// Saves `collect()` on a background thread, at most once per `DEBOUNCE`.
pub struct Persister {
    path: PathBuf,
    collect: Collect,
    state: Arc<(Mutex<PersistState>, Condvar)>,
}

#[derive(Default)]
struct PersistState {
    dirty: bool,
    /// Set by `flush` at quit: the hang-up that follows must not save an
    /// empty list over the sessions restore needs.
    frozen: bool,
}

impl Persister {
    pub fn new(path: PathBuf, collect: Collect) -> Arc<Self> {
        let persister = Arc::new(Persister {
            path,
            collect,
            state: Arc::default(),
        });
        let weak = Arc::downgrade(&persister);
        let state = Arc::clone(&persister.state);
        std::thread::Builder::new()
            .name("persist".into())
            .spawn(move || {
                loop {
                    {
                        let (lock, changed) = &*state;
                        let mut s = lock.lock().unwrap();
                        while !s.dirty {
                            s = changed.wait(s).unwrap();
                        }
                    }
                    std::thread::sleep(DEBOUNCE);
                    let Some(persister) = weak.upgrade() else {
                        return;
                    };
                    persister.write();
                }
            })
            .expect("spawning a thread");
        persister
    }

    pub fn mark(&self) {
        let (lock, changed) = &*self.state;
        lock.lock().unwrap().dirty = true;
        changed.notify_all();
    }

    /// Save now and stop saving.
    pub fn flush(&self) {
        self.write();
        self.state.0.lock().unwrap().frozen = true;
    }

    fn write(&self) {
        let mut s = self.state.0.lock().unwrap();
        if s.frozen {
            return;
        }
        s.dirty = false;
        // Held while writing, so a flush can't race a background save.
        if let Err(error) = save(&self.path, (self.collect)()) {
            eprintln!(
                "bitplane-cc: couldn't save {}: {error}",
                self.path.display()
            );
        }
    }
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
    fn round_trips_in_creation_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data/sessions.json");
        save(&path, vec![entry("b", 2), entry("a", 1)]).unwrap();
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

    #[test]
    fn saves_after_a_change_and_never_after_a_flush() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions.json");
        let entries = Arc::new(Mutex::new(vec![entry("a", 1)]));
        let source = Arc::clone(&entries);
        let persister = Persister::new(
            path.clone(),
            Arc::new(move || source.lock().unwrap().clone()),
        );

        persister.mark();
        std::thread::sleep(DEBOUNCE * 3);
        assert_eq!(load(&path), [entry("a", 1)]);

        entries.lock().unwrap().push(entry("b", 2));
        persister.flush();
        entries.lock().unwrap().clear();
        persister.mark();
        std::thread::sleep(DEBOUNCE * 3);
        assert_eq!(load(&path), [entry("a", 1), entry("b", 2)]);
    }
}
