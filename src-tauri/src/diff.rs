//! What a File tab shows, computed from two blobs (ADR 0009): a unified
//! patch, or why there is none. Pure: the blobs come from the daemon.

use serde::{Deserialize, Serialize};
use similar::TextDiff;
use specta::Type;

/// Past this on either side, a file is a placeholder instead of a diff.
pub const MAX_FILE_BYTES: u64 = 1_000_000;

/// Git's own heuristic: a NUL in the first 8000 bytes means binary.
const BINARY_SNIFF_BYTES: usize = 8000;

const CONTEXT_LINES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DiffMode {
    /// HEAD ↔ index.
    Index,
    /// Index ↔ worktree.
    Worktree,
    /// HEAD ↔ worktree.
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FileDiff {
    Diff {
        patch: String,
    },
    /// Nothing to compare: ignored, outside any repo, or unchanged in `file` mode.
    Plain,
    #[serde(rename_all = "camelCase")]
    Binary {
        old_size: u64,
        new_size: u64,
    },
    TooLarge {
        size: u64,
    },
    /// The change this `index` or `worktree` tab showed is gone.
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileContents {
    pub old: String,
    pub new: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FileText {
    Text { contents: String },
    Binary { size: u64 },
    TooLarge { size: u64 },
}

/// One side of a diff as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Side {
    /// Not in that revision, or not on disk.
    Missing,
    /// Read without its bytes, past the limit asked for.
    TooLarge {
        size: u64,
    },
    Bytes(Vec<u8>),
}

impl Side {
    pub fn size(&self) -> u64 {
        match self {
            Side::Missing => 0,
            Side::TooLarge { size } => *size,
            Side::Bytes(bytes) => bytes.len() as u64,
        }
    }

    /// Lossy text; empty when missing or not read.
    pub fn text(&self) -> String {
        match self {
            Side::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            Side::Missing | Side::TooLarge { .. } => String::new(),
        }
    }
}

/// `old` at `old_path` against `new` at `new_path`, both relative to their
/// repo; `unchanged` is what equal sides show.
pub fn diff(
    old_path: &str,
    new_path: &str,
    old: &Side,
    new: &Side,
    unchanged: FileDiff,
) -> FileDiff {
    if matches!(old, Side::TooLarge { .. }) || matches!(new, Side::TooLarge { .. }) {
        return FileDiff::TooLarge {
            size: old.size().max(new.size()),
        };
    }
    if old == new {
        return unchanged;
    }
    let bytes = |side: &Side| match side {
        Side::Bytes(bytes) => looks_binary(bytes),
        Side::Missing | Side::TooLarge { .. } => false,
    };
    if bytes(old) || bytes(new) {
        return FileDiff::Binary {
            old_size: old.size(),
            new_size: new.size(),
        };
    }
    let old_text = (*old != Side::Missing).then(|| old.text());
    let new_text = (*new != Side::Missing).then(|| new.text());
    FileDiff::Diff {
        patch: patch(old_path, new_path, old_text.as_deref(), new_text.as_deref()),
    }
}

/// A git-style unified patch; `None` is a side that doesn't exist.
pub fn patch(old_path: &str, new_path: &str, old: Option<&str>, new: Option<&str>) -> String {
    let mut out = format!("diff --git a/{old_path} b/{new_path}\n");
    match (old, new) {
        (None, Some(_)) => out.push_str("new file mode 100644\n"),
        (Some(_), None) => out.push_str("deleted file mode 100644\n"),
        _ if old_path != new_path => {
            out.push_str(&format!("rename from {old_path}\nrename to {new_path}\n"));
        }
        _ => {}
    }
    let (old_text, new_text) = (old.unwrap_or_default(), new.unwrap_or_default());
    if old_text == new_text {
        return out;
    }
    let old_name = old.map_or("/dev/null".into(), |_| format!("a/{old_path}"));
    let new_name = new.map_or("/dev/null".into(), |_| format!("b/{new_path}"));
    out.push_str(
        &TextDiff::from_lines(old_text, new_text)
            .unified_diff()
            .context_radius(CONTEXT_LINES)
            .header(&old_name, &new_name)
            .to_string(),
    );
    out
}

/// What a File tab with no diff shows of a file read with
/// [`MAX_FILE_BYTES`] as its limit.
pub fn file_text(side: &Side) -> FileText {
    match side {
        Side::TooLarge { size } => FileText::TooLarge { size: *size },
        Side::Bytes(bytes) if looks_binary(bytes) => FileText::Binary {
            size: bytes.len() as u64,
        },
        side => FileText::Text {
            contents: side.text(),
        },
    }
}

pub fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(BINARY_SNIFF_BYTES).any(|byte| *byte == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(text: &str) -> Side {
        Side::Bytes(text.as_bytes().to_vec())
    }

    fn patch_of(diff: FileDiff) -> String {
        match diff {
            FileDiff::Diff { patch } => patch,
            other => panic!("expected a diff, got {other:?}"),
        }
    }

    #[test]
    fn a_change_is_a_git_style_patch() {
        let diff = diff(
            "a.txt",
            "a.txt",
            &text("one\ntwo\n"),
            &text("one\nstaged\n"),
            FileDiff::None,
        );
        assert_eq!(
            patch_of(diff),
            "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n one\n-two\n+staged\n"
        );
    }

    #[test]
    fn equal_sides_show_what_the_mode_says() {
        let side = text("same\n");
        assert_eq!(
            diff("a", "a", &side, &side, FileDiff::Plain),
            FileDiff::Plain
        );
        assert_eq!(diff("a", "a", &side, &side, FileDiff::None), FileDiff::None);
        assert_eq!(
            diff("a", "a", &Side::Missing, &Side::Missing, FileDiff::None),
            FileDiff::None
        );
    }

    #[test]
    fn a_new_file_is_all_added_and_a_deleted_one_all_removed() {
        let added = patch_of(diff(
            "n.txt",
            "n.txt",
            &Side::Missing,
            &text("x\ny\n"),
            FileDiff::None,
        ));
        assert_eq!(
            added,
            "diff --git a/n.txt b/n.txt\nnew file mode 100644\n--- /dev/null\n+++ b/n.txt\n@@ -0,0 +1,2 @@\n+x\n+y\n"
        );
        let deleted = patch_of(diff(
            "n.txt",
            "n.txt",
            &text("x\n"),
            &Side::Missing,
            FileDiff::None,
        ));
        assert!(
            deleted.contains("deleted file mode 100644\n--- a/n.txt\n+++ /dev/null\n"),
            "{deleted}"
        );
        assert!(deleted.ends_with("@@ -1 +0,0 @@\n-x\n"), "{deleted}");
    }

    #[test]
    fn a_rename_diffs_against_its_old_path() {
        let patch = patch_of(diff(
            "old.txt",
            "new.txt",
            &text("a\nb\n"),
            &text("a\nc\n"),
            FileDiff::None,
        ));
        assert!(
            patch.starts_with("diff --git a/old.txt b/new.txt\nrename from old.txt\nrename to new.txt\n--- a/old.txt\n+++ b/new.txt\n"),
            "{patch}"
        );
    }

    #[test]
    fn a_missing_final_newline_is_marked() {
        let patch = patch_of(diff("a", "a", &text("a\n"), &text("a\nb"), FileDiff::None));
        assert!(
            patch.ends_with("+b\n\\ No newline at end of file\n"),
            "{patch}"
        );
    }

    #[test]
    fn binary_sides_report_their_sizes() {
        let binary = Side::Bytes(vec![0, 1, 2]);
        assert_eq!(
            diff("a", "a", &Side::Missing, &binary, FileDiff::None),
            FileDiff::Binary {
                old_size: 0,
                new_size: 3
            }
        );
        assert_eq!(file_text(&binary), FileText::Binary { size: 3 });
    }

    #[test]
    fn a_side_read_without_bytes_is_too_large() {
        assert_eq!(
            diff(
                "a",
                "a",
                &text("small"),
                &Side::TooLarge { size: 2_000_000 },
                FileDiff::None
            ),
            FileDiff::TooLarge { size: 2_000_000 }
        );
        assert_eq!(
            file_text(&Side::TooLarge { size: 5 }),
            FileText::TooLarge { size: 5 }
        );
    }

    #[test]
    fn plain_text_is_shown_as_is() {
        assert_eq!(
            file_text(&text("hello")),
            FileText::Text {
                contents: "hello".into()
            }
        );
    }
}
