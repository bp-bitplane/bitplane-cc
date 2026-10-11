//! The plane list: resolving the planes directory, scanning for drafts,
//! merging everything into sidebar rows, and the watcher (spec §5.1, §5.2, §6.2).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{Debouncer, new_debouncer};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::env::EnvMap;

pub const PLANE_FILE: &str = "plane.toml";
pub const BITPLANE_DIR: &str = ".bitplane";
pub const DEBOUNCE: Duration = Duration::from_millis(250);

/// One sidebar row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlaneRow {
    pub id: String,
    pub directory: String,
    pub kind: PlaneKind,
    /// Finding tags from `bp list`; empty for drafts and planes being created.
    pub findings: Vec<String>,
    /// Member specs from `bp list`; empty unless `kind` is `Plane`.
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PlaneKind {
    Plane,
    Draft,
    Creating,
    Gone,
}

/// Where a row came from, before merging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub id: String,
    pub directory: PathBuf,
    pub findings: Vec<String>,
    pub members: Vec<String>,
}

impl Source {
    pub fn new(id: impl Into<String>, directory: impl Into<PathBuf>) -> Self {
        Source {
            id: id.into(),
            directory: directory.into(),
            findings: Vec::new(),
            members: Vec::new(),
        }
    }
}

/// bp's plane id rules: `[a-z0-9][a-z0-9._-]*`, at most 64 characters, and
/// not starting with the reserved `bp-`.
pub fn validate_plane_id(id: &str) -> Result<(), String> {
    let mut chars = id.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let rest_ok =
        chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'));
    if id.is_empty() {
        Err("Enter an id".into())
    } else if !first_ok || !rest_ok {
        Err(
            "Use lowercase letters, digits, '.', '_' and '-', starting with a letter or digit"
                .into(),
        )
    } else if id.len() > 64 {
        Err("At most 64 characters".into())
    } else if id.starts_with("bp-") {
        Err("Ids starting with 'bp-' are reserved".into())
    } else {
        Ok(())
    }
}

/// Resolve the planes directory the way bp does: `$BITPLANE_PLANES_DIR`, then
/// `planes_dir` in bp's config file, then `~/planes`.
pub fn resolve_planes_dir(env: &EnvMap) -> PathBuf {
    let home = env.get("HOME").map(PathBuf::from).unwrap_or_default();
    if let Some(dir) = env.get("BITPLANE_PLANES_DIR").filter(|d| !d.is_empty()) {
        return expand_tilde(dir, &home);
    }
    if let Some(dir) = config_planes_dir(&config_path(env, &home)) {
        return expand_tilde(&dir, &home);
    }
    home.join("planes")
}

/// Every subfolder without a plane file, skipping dot-folders. A missing
/// planes directory has no drafts.
pub fn scan_drafts(planes_dir: &Path) -> Vec<Source> {
    let Ok(entries) = std::fs::read_dir(planes_dir) else {
        return Vec::new();
    };
    let mut drafts: Vec<Source> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|entry| {
            let id = entry.file_name().into_string().ok()?;
            let path = entry.path();
            (!id.starts_with('.') && !path.join(PLANE_FILE).exists()).then(|| Source::new(id, path))
        })
        .collect();
    drafts.sort_by(|a, b| a.id.cmp(&b.id));
    drafts
}

/// Merge every source into alphabetically sorted rows.
///
/// A plane being created wins over what `bp list` says about it (its findings
/// are noise until the create finishes). A plane that still has sessions but
/// appears nowhere else is gone.
pub fn merge_rows(
    listed: &[Source],
    drafts: &[Source],
    creating: &[Source],
    with_sessions: &[Source],
) -> Vec<PlaneRow> {
    let mut rows: BTreeMap<String, PlaneRow> = BTreeMap::new();
    let mut put = |source: &Source, kind: PlaneKind| {
        let listed = kind == PlaneKind::Plane;
        rows.entry(source.id.clone()).or_insert_with(|| PlaneRow {
            id: source.id.clone(),
            directory: source.directory.to_string_lossy().into_owned(),
            kind,
            findings: if listed {
                source.findings.clone()
            } else {
                Vec::new()
            },
            members: if listed {
                source.members.clone()
            } else {
                Vec::new()
            },
        });
    };
    for source in creating {
        put(source, PlaneKind::Creating);
    }
    for source in listed {
        put(source, PlaneKind::Plane);
    }
    for source in drafts {
        put(source, PlaneKind::Draft);
    }
    for source in with_sessions {
        put(source, PlaneKind::Gone);
    }
    rows.into_values().collect()
}

/// Paths the watcher observes, none recursively: the planes directory (or its
/// parent while it doesn't exist), and each plane's root and `.bitplane/`.
pub fn watch_paths(planes_dir: &Path, plane_dirs: &[PathBuf]) -> BTreeSet<PathBuf> {
    let mut paths = BTreeSet::new();
    if planes_dir.is_dir() {
        paths.insert(planes_dir.to_path_buf());
    } else if let Some(parent) = planes_dir.parent().filter(|p| p.is_dir()) {
        paths.insert(parent.to_path_buf());
    }
    for dir in plane_dirs {
        for path in [dir.clone(), dir.join(BITPLANE_DIR)] {
            if path.is_dir() {
                paths.insert(path);
            }
        }
    }
    paths
}

/// A debounced, non-recursive watcher over a changing set of paths.
pub struct Watcher {
    debouncer: Debouncer<RecommendedWatcher>,
    watched: BTreeSet<PathBuf>,
}

impl Watcher {
    /// `on_change` runs on the debouncer's thread after each debounced batch.
    pub fn new(on_change: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        let debouncer = new_debouncer(
            DEBOUNCE,
            move |result: notify_debouncer_mini::DebounceEventResult| {
                if result.is_ok_and(|events| !events.is_empty()) {
                    on_change();
                }
            },
        )
        .map_err(|e| e.to_string())?;
        Ok(Watcher {
            debouncer,
            watched: BTreeSet::new(),
        })
    }

    /// Watch exactly `paths`, adding and removing the difference.
    pub fn set_paths(&mut self, paths: BTreeSet<PathBuf>) {
        let watcher = self.debouncer.watcher();
        for gone in self.watched.difference(&paths) {
            let _ = watcher.unwatch(gone);
        }
        let mut watched = BTreeSet::new();
        for path in paths {
            let ok = self.watched.contains(&path)
                || watcher.watch(&path, RecursiveMode::NonRecursive).is_ok();
            if ok {
                watched.insert(path);
            }
        }
        self.watched = watched;
    }
}

fn config_path(env: &EnvMap, home: &Path) -> PathBuf {
    if let Some(path) = env.get("BITPLANE_CONFIG").filter(|p| !p.is_empty()) {
        return expand_tilde(path, home);
    }
    let config_home = env
        .get("XDG_CONFIG_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    config_home.join("bitplane").join("config.toml")
}

fn config_planes_dir(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let table: toml::Table = text.parse().ok()?;
    table.get("planes_dir")?.as_str().map(str::to_owned)
}

fn expand_tilde(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix('~') {
        Some("") => home.to_path_buf(),
        Some(rest) if rest.starts_with(std::path::MAIN_SEPARATOR) => {
            home.join(&rest[std::path::MAIN_SEPARATOR.len_utf8()..])
        }
        _ => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> EnvMap {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn validates_plane_ids() {
        for ok in ["a", "0", "feat-x", "v1.2_rc", &"a".repeat(64)] {
            assert_eq!(validate_plane_id(ok), Ok(()), "{ok}");
        }
        for bad in ["", "-a", ".a", "A", "a/b", "a b", "bp-x", &"a".repeat(65)] {
            assert!(validate_plane_id(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn env_var_wins() {
        let env = env(&[
            ("HOME", "/home/u"),
            ("BITPLANE_PLANES_DIR", "~/work/planes"),
        ]);
        assert_eq!(
            resolve_planes_dir(&env),
            PathBuf::from("/home/u/work/planes")
        );
    }

    #[test]
    fn empty_env_var_is_ignored() {
        let env = env(&[("HOME", "/nonexistent-home"), ("BITPLANE_PLANES_DIR", "")]);
        assert_eq!(
            resolve_planes_dir(&env),
            PathBuf::from("/nonexistent-home/planes")
        );
    }

    #[test]
    fn config_file_from_bitplane_config() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("custom.toml");
        std::fs::write(&config, "version = 1\nplanes_dir = \"~/elsewhere\"\n").unwrap();
        let env = env(&[
            ("HOME", "/home/u"),
            ("BITPLANE_CONFIG", config.to_str().unwrap()),
        ]);
        assert_eq!(resolve_planes_dir(&env), PathBuf::from("/home/u/elsewhere"));
    }

    #[test]
    fn config_file_under_xdg_config_home() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("bitplane")).unwrap();
        std::fs::write(
            dir.path().join("bitplane/config.toml"),
            "planes_dir = \"/abs/planes\"\n",
        )
        .unwrap();
        let env = env(&[
            ("HOME", "/home/u"),
            ("XDG_CONFIG_HOME", dir.path().to_str().unwrap()),
        ]);
        assert_eq!(resolve_planes_dir(&env), PathBuf::from("/abs/planes"));
    }

    #[test]
    fn config_under_default_config_home() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".config/bitplane")).unwrap();
        std::fs::write(
            home.path().join(".config/bitplane/config.toml"),
            "planes_dir = \"~\"\n",
        )
        .unwrap();
        let env = env(&[("HOME", home.path().to_str().unwrap())]);
        assert_eq!(resolve_planes_dir(&env), home.path());
    }

    #[test]
    fn defaults_to_home_planes() {
        let home = tempfile::tempdir().unwrap();
        let env = env(&[("HOME", home.path().to_str().unwrap())]);
        assert_eq!(resolve_planes_dir(&env), home.path().join("planes"));
    }

    #[test]
    fn drafts_are_folders_without_a_plane_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for name in ["real", "draft", ".hidden", "stray"] {
            std::fs::create_dir(root.join(name)).unwrap();
        }
        std::fs::write(root.join("real").join(PLANE_FILE), "").unwrap();
        std::fs::write(root.join("file.txt"), "").unwrap();
        let ids: Vec<_> = scan_drafts(root).into_iter().map(|d| d.id).collect();
        assert_eq!(ids, ["draft", "stray"]);
        assert!(scan_drafts(&root.join("missing")).is_empty());
    }

    #[test]
    fn merges_sorted_with_precedence() {
        let mut listed = Source::new("beta", "/p/beta");
        listed.findings = vec!["member_worktree_missing".into()];
        listed.members = vec!["@api".into()];
        let mut creating_listed = Source::new("new", "/p/new");
        creating_listed.findings = vec!["create_never_completed".into()];
        creating_listed.members = vec!["@api".into()];
        let rows = merge_rows(
            &[listed, creating_listed],
            &[Source::new("alpha", "/p/alpha")],
            &[Source::new("new", "/p/new")],
            &[Source::new("beta", "/p/beta"), Source::new("old", "/p/old")],
        );
        let summary: Vec<_> = rows
            .iter()
            .map(|r| (r.id.as_str(), r.kind, r.findings.len(), r.members.len()))
            .collect();
        assert_eq!(
            summary,
            [
                ("alpha", PlaneKind::Draft, 0, 0),
                ("beta", PlaneKind::Plane, 1, 1),
                ("new", PlaneKind::Creating, 0, 0),
                ("old", PlaneKind::Gone, 0, 0),
            ]
        );
    }

    #[test]
    fn watches_parent_while_planes_dir_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let planes = dir.path().join("planes");
        assert_eq!(
            watch_paths(&planes, &[]),
            BTreeSet::from([dir.path().to_path_buf()])
        );

        std::fs::create_dir_all(planes.join("a/.bitplane")).unwrap();
        std::fs::create_dir_all(planes.join("b")).unwrap();
        let paths = watch_paths(
            &planes,
            &[planes.join("a"), planes.join("b"), planes.join("c")],
        );
        assert_eq!(
            paths,
            BTreeSet::from([
                planes.clone(),
                planes.join("a"),
                planes.join("a/.bitplane"),
                planes.join("b"),
            ])
        );
    }
}
