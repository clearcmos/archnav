// Read-only view of tagdex tag stores, plus mutations via the tagdex CLI.
//
// Single-writer principle: archnav parses .tagstore/index.json directly for
// display (cheap, cached by index mtime+size), but every mutation shells out
// to `tagdex` so the store lock, atomic index writes, content fingerprints,
// and xattr mirroring stay in one implementation. If archnav wrote the JSON
// itself there would be two write paths that could drift and corrupt.
//
// Index format: tagdex index.json version 1 (see ~/git/tagdex CLAUDE.md).

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

const STORE_DIR: &str = ".tagstore";
const INDEX_FILE: &str = "index.json";
const SUPPORTED_VERSION: u64 = 1;

#[derive(Deserialize)]
struct IndexDoc {
    version: u64,
    entries: HashMap<String, EntryDoc>,
}

#[derive(Deserialize)]
struct EntryDoc {
    #[serde(default)]
    tags: Vec<String>,
}

/// Result of looking up a file's tags.
#[derive(Debug)]
pub enum TagLookup {
    /// No .tagstore directory exists above the file.
    NoStore,
    /// The file is inside a store; an empty vec means untagged.
    Tags(Vec<String>),
}

struct CachedIndex {
    mtime: SystemTime,
    size: u64,
    entries: Arc<HashMap<String, Vec<String>>>,
}

fn cache() -> &'static Mutex<HashMap<PathBuf, CachedIndex>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedIndex>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Store-root discovery cache, keyed by the file's parent directory. The
/// results list decorates every row with tags on each keystroke, and the
/// walk-up stats behind discovery are expensive on CIFS mounts. The short
/// TTL also bounds how stale a freshly-created (or deleted) .tagstore can
/// look inside a running archnav.
const DIR_ROOT_TTL: Duration = Duration::from_secs(5);

type DirRootCache = HashMap<PathBuf, (Instant, Option<PathBuf>)>;

fn dir_root_cache() -> &'static Mutex<DirRootCache> {
    static CACHE: OnceLock<Mutex<DirRootCache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Walk upward from the file's directory looking for a .tagstore dir.
pub fn find_store_root(file: &Path) -> Option<PathBuf> {
    let dir = file.parent()?;
    if let Some((at, root)) = dir_root_cache().lock().unwrap().get(dir) {
        if at.elapsed() < DIR_ROOT_TTL {
            return root.clone();
        }
    }
    let root = discover_root(dir);
    dir_root_cache()
        .lock()
        .unwrap()
        .insert(dir.to_path_buf(), (Instant::now(), root.clone()));
    root
}

fn discover_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start;
    loop {
        if cur.join(STORE_DIR).is_dir() {
            return Some(cur.to_path_buf());
        }
        cur = cur.parent()?;
    }
}

/// Public read of a store root's tag entries (rel path -> tags), used by the
/// search engine to answer t: queries from the stores themselves.
pub fn entries_for_root(root: &Path) -> Result<Arc<HashMap<String, Vec<String>>>, String> {
    load_index(root)
}

/// Load (or reuse from cache) the tag entries of the store rooted at `root`.
fn load_index(root: &Path) -> Result<Arc<HashMap<String, Vec<String>>>, String> {
    let index_path = root.join(STORE_DIR).join(INDEX_FILE);
    let meta = std::fs::metadata(&index_path)
        .map_err(|e| format!("cannot stat {}: {}", index_path.display(), e))?;
    let mtime = meta.modified().map_err(|e| e.to_string())?;
    let size = meta.len();

    if let Some(cached) = cache().lock().unwrap().get(root) {
        if cached.mtime == mtime && cached.size == size {
            return Ok(cached.entries.clone());
        }
    }

    let data = std::fs::read(&index_path)
        .map_err(|e| format!("cannot read {}: {}", index_path.display(), e))?;
    let doc: IndexDoc = serde_json::from_slice(&data).map_err(|e| {
        format!(
            "{} is not a valid tagdex index: {}",
            index_path.display(),
            e
        )
    })?;
    if doc.version > SUPPORTED_VERSION {
        return Err(format!(
            "tagdex index version {} is newer than archnav supports ({})",
            doc.version, SUPPORTED_VERSION
        ));
    }

    let entries: Arc<HashMap<String, Vec<String>>> =
        Arc::new(doc.entries.into_iter().map(|(k, v)| (k, v.tags)).collect());
    cache().lock().unwrap().insert(
        root.to_path_buf(),
        CachedIndex {
            mtime,
            size,
            entries: entries.clone(),
        },
    );
    Ok(entries)
}

/// Look up the tags of an absolute file path.
pub fn read_tags(file: &Path) -> Result<TagLookup, String> {
    let Some(root) = find_store_root(file) else {
        return Ok(TagLookup::NoStore);
    };
    let entries = load_index(&root)?;
    let rel = file
        .strip_prefix(&root)
        .map_err(|_| format!("{} is not under {}", file.display(), root.display()))?
        .to_string_lossy()
        .into_owned();
    Ok(TagLookup::Tags(
        entries.get(rel.as_str()).cloned().unwrap_or_default(),
    ))
}

/// Locate the tagdex binary. KDE autostart sessions may not have
/// ~/.local/bin on PATH, so prefer the explicit uv tool location.
fn tagdex_bin() -> PathBuf {
    if let Ok(p) = std::env::var("TAGDEX_BIN") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    if let Some(home) = dirs::home_dir() {
        let local = home.join(".local/bin/tagdex");
        if local.is_file() {
            return local;
        }
    }
    PathBuf::from("tagdex")
}

/// Parse the tag dialog's comma-separated input into clean tags.
/// Commas are invalid inside tagdex tags, so they are a safe separator.
pub fn parse_tag_input(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Replace the file's tags via the tagdex CLI (empty slice clears them).
pub fn write_tags(file: &Path, tags: &[String]) -> Result<(), String> {
    let bin = tagdex_bin();
    let mut cmd = std::process::Command::new(&bin);
    if tags.is_empty() {
        cmd.arg("rm").arg("--all").arg(file);
    } else {
        cmd.arg("set");
        for tag in tags {
            cmd.arg("-t").arg(tag);
        }
        cmd.arg(file);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("failed to run {}: {}", bin.display(), e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let msg = stderr.trim().trim_start_matches("tagdex: ").to_string();
        return Err(if msg.is_empty() {
            format!("tagdex exited with {}", out.status)
        } else {
            msg
        });
    }
    // Drop the cached index so the next read reflects the write immediately.
    if let Some(root) = find_store_root(file) {
        cache().lock().unwrap().remove(&root);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store(root: &Path, entries_json: &str) {
        std::fs::create_dir_all(root.join(STORE_DIR)).unwrap();
        std::fs::write(
            root.join(STORE_DIR).join(INDEX_FILE),
            format!(r#"{{"version": 1, "entries": {}}}"#, entries_json),
        )
        .unwrap();
    }

    #[test]
    fn parse_tag_input_splits_and_trims() {
        assert_eq!(parse_tag_input(" a, b tag ,, c "), vec!["a", "b tag", "c"]);
        assert_eq!(parse_tag_input("  "), Vec::<String>::new());
    }

    #[test]
    fn find_store_root_walks_up() {
        let tmp = tempfile::tempdir().unwrap();
        make_store(tmp.path(), "{}");
        let nested = tmp.path().join("a/b");
        std::fs::create_dir_all(&nested).unwrap();
        let file = nested.join("f.txt");
        std::fs::write(&file, "x").unwrap();
        assert_eq!(find_store_root(&file).unwrap(), tmp.path());
    }

    #[test]
    fn read_tags_no_store() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("f.txt");
        std::fs::write(&file, "x").unwrap();
        assert!(matches!(read_tags(&file).unwrap(), TagLookup::NoStore));
    }

    #[test]
    fn read_tags_tagged_and_untagged() {
        let tmp = tempfile::tempdir().unwrap();
        make_store(
            tmp.path(),
            r#"{"sub/doc.txt": {"tags": ["b", "a"], "size": 1, "mtime_ns": 2, "fp": "x"}}"#,
        );
        let tagged = tmp.path().join("sub/doc.txt");
        match read_tags(&tagged).unwrap() {
            TagLookup::Tags(t) => assert_eq!(t, vec!["b", "a"]),
            TagLookup::NoStore => panic!("expected tags"),
        }
        let untagged = tmp.path().join("other.txt");
        match read_tags(&untagged).unwrap() {
            TagLookup::Tags(t) => assert!(t.is_empty()),
            TagLookup::NoStore => panic!("expected empty tags, not NoStore"),
        }
    }

    #[test]
    fn newer_index_version_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(STORE_DIR)).unwrap();
        std::fs::write(
            tmp.path().join(STORE_DIR).join(INDEX_FILE),
            r#"{"version": 99, "entries": {}}"#,
        )
        .unwrap();
        let file = tmp.path().join("f.txt");
        let err = read_tags(&file).unwrap_err();
        assert!(err.contains("version 99"));
    }

    #[test]
    fn cache_invalidated_when_index_changes() {
        let tmp = tempfile::tempdir().unwrap();
        make_store(tmp.path(), "{}");
        let file = tmp.path().join("doc.txt");
        match read_tags(&file).unwrap() {
            TagLookup::Tags(t) => assert!(t.is_empty()),
            TagLookup::NoStore => panic!(),
        }
        // Rewrite the index with different content (size changes, so the
        // mtime+size cache key misses even with coarse timestamps).
        make_store(
            tmp.path(),
            r#"{"doc.txt": {"tags": ["fresh"], "size": 1, "mtime_ns": 2, "fp": "x"}}"#,
        );
        match read_tags(&file).unwrap() {
            TagLookup::Tags(t) => assert_eq!(t, vec!["fresh"]),
            TagLookup::NoStore => panic!(),
        }
    }
}
