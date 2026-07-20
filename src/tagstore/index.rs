//! Index file model: schema validation, atomic load and save.
//!
//! The index is a single JSON document at <root>/.tagstore/index.json.
//! Writes go to a temporary file in the same directory followed by an
//! atomic rename (atomic on local filesystems and on the SMB server for
//! CIFS mounts). Output is deterministic - sorted keys via BTreeMap,
//! 2-space indent, trailing newline - matching the format the original
//! Python implementation wrote, so existing stores diff cleanly.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

pub const FORMAT_VERSION: u64 = 1;

// Field order is alphabetical so serde's struct serialization matches the
// sort_keys=True output of the Python implementation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub fp: String,
    pub mtime_ns: i64,
    pub size: u64,
    pub tags: Vec<String>,
}

#[derive(Serialize)]
struct IndexDoc<'a> {
    entries: &'a BTreeMap<String, Entry>,
    version: u64,
}

#[derive(Debug, Default)]
pub struct Index {
    pub entries: BTreeMap<String, Entry>,
}

impl Index {
    pub fn load(path: &Path) -> Result<Index, String> {
        let data =
            std::fs::read(path).map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
        let raw: serde_json::Value = serde_json::from_slice(&data)
            .map_err(|e| format!("{} is not valid JSON: {}", path.display(), e))?;

        // Version is checked before entry parsing so a future format fails
        // with a clear message instead of a shape mismatch.
        let version = raw
            .get("version")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| format!("{}: missing or invalid version", path.display()))?;
        if version > FORMAT_VERSION {
            return Err(format!(
                "{}: format version {} is newer than this archnav supports ({})",
                path.display(),
                version,
                FORMAT_VERSION
            ));
        }

        let raw_entries = raw
            .get("entries")
            .cloned()
            .ok_or_else(|| format!("{}: missing entries object", path.display()))?;
        let mut entries: BTreeMap<String, Entry> = serde_json::from_value(raw_entries)
            .map_err(|e| format!("{}: malformed entry: {}", path.display(), e))?;
        for entry in entries.values_mut() {
            entry.tags.sort();
        }
        Ok(Index { entries })
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let doc = IndexDoc {
            entries: &self.entries,
            version: FORMAT_VERSION,
        };
        let mut json = serde_json::to_string_pretty(&doc)
            .map_err(|e| format!("cannot serialize index: {}", e))?;
        json.push('\n');

        let tmp = path.with_file_name(format!(
            "{}.tmp.{}",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("index.json"),
            std::process::id()
        ));
        let write_result = (|| -> std::io::Result<()> {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(json.as_bytes())?;
            f.sync_all()?;
            std::fs::rename(&tmp, path)
        })();
        if write_result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        write_result.map_err(|e| format!("cannot write {}: {}", path.display(), e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Index written by the original Python implementation (committed
    // fixture): loading it proves cross-implementation compatibility,
    // including unicode paths, spaces, and tag ordering.
    const PYTHON_INDEX: &str = include_str!("testdata/python-index-v1.json");

    #[test]
    fn loads_python_written_index() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("index.json");
        std::fs::write(&p, PYTHON_INDEX).unwrap();
        let idx = Index::load(&p).unwrap();
        assert_eq!(idx.entries.len(), 2);
        let entry = &idx.entries["Top Picks/café menu.pdf"];
        assert_eq!(entry.tags, vec!["client:acme corp", "menu", "restaurant"]);
        assert_eq!(entry.size, 12);
        assert_eq!(entry.fp, "c374106b0e1eef9844fc247e5a222d47");
    }

    #[test]
    fn round_trip_preserves_python_fixture_bytes() {
        // Same content, same bytes: proves the Rust writer emits the exact
        // format the Python implementation produced.
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("index.json");
        std::fs::write(&p, PYTHON_INDEX).unwrap();
        let idx = Index::load(&p).unwrap();
        let out = tmp.path().join("out.json");
        idx.save(&out).unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), PYTHON_INDEX);
    }

    #[test]
    fn save_is_deterministic_and_leaves_no_temp_file() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("index.json");
        let mut idx = Index::default();
        idx.entries.insert(
            "b.txt".into(),
            Entry {
                fp: "aa".into(),
                mtime_ns: 2,
                size: 1,
                tags: vec!["x".into()],
            },
        );
        idx.save(&p).unwrap();
        let first = std::fs::read(&p).unwrap();
        idx.save(&p).unwrap();
        assert_eq!(first, std::fs::read(&p).unwrap());
        let leftovers: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name() != "index.json")
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn future_version_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("index.json");
        std::fs::write(&p, r#"{"version": 2, "entries": {}}"#).unwrap();
        let err = Index::load(&p).unwrap_err();
        assert!(err.contains("version 2"), "got: {}", err);
    }

    #[test]
    fn invalid_json_and_malformed_entries_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("index.json");
        std::fs::write(&p, "{nope").unwrap();
        assert!(Index::load(&p).unwrap_err().contains("not valid JSON"));
        std::fs::write(
            &p,
            r#"{"version": 1, "entries": {"a": {"tags": "oops", "size": 1, "mtime_ns": 2, "fp": "x"}}}"#,
        )
        .unwrap();
        assert!(Index::load(&p).unwrap_err().contains("malformed entry"));
    }

    #[test]
    fn tags_come_back_sorted() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("index.json");
        std::fs::write(
            &p,
            r#"{"version": 1, "entries": {"a": {"tags": ["b", "a"], "size": 1, "mtime_ns": 2, "fp": "x"}}}"#,
        )
        .unwrap();
        assert_eq!(Index::load(&p).unwrap().entries["a"].tags, vec!["a", "b"]);
    }
}
