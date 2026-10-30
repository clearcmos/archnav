use std::path::Path;
use std::sync::mpsc::Sender;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::UNIX_EPOCH;
use std::{fs, time};
use tracing::info;
use walkdir::WalkDir;

use super::database::DbOp;
use super::trigram::{FileEntry, TrigramIndex, Upsert, EXCLUDE_PATTERNS};

/// User-configured recursive exclude paths (absolute, already normalized:
/// `~` expanded and trailing slashes trimmed). Installed once at startup via
/// [`set_exclude_paths`] so every scan path - the initial scan, reconcile,
/// network rescans, and the live inotify watcher - shares one source of truth.
static USER_EXCLUDE_PATHS: OnceLock<Vec<String>> = OnceLock::new();

/// Install the user's recursive exclude paths. Call once, before any scanning.
pub fn set_exclude_paths(paths: Vec<String>) {
    let _ = USER_EXCLUDE_PATHS.set(paths);
}

/// Whether the user configured any exclude paths.
pub fn has_user_excludes() -> bool {
    USER_EXCLUDE_PATHS.get().is_some_and(|v| !v.is_empty())
}

/// True if `path` equals, or is nested recursively under, any exclude path.
pub fn is_user_excluded(path: &str) -> bool {
    USER_EXCLUDE_PATHS
        .get()
        .is_some_and(|excludes| path_under_any(path, excludes))
}

/// Pure matcher: is `path` one of `excludes`, or beneath one?
fn path_under_any(path: &str, excludes: &[String]) -> bool {
    excludes.iter().any(|b| path_under_root(path, b))
}

/// True if `path` equals `root` or sits beneath it with a `/` boundary, so
/// `/a/b` is under `/a` but `/a/bc` is not (sibling sharing a prefix).
/// Trailing slashes on `root` are tolerated; an empty root matches nothing.
pub fn path_under_root(path: &str, root: &str) -> bool {
    let root = if root.len() > 1 {
        root.trim_end_matches('/')
    } else {
        root
    };
    if root.is_empty() {
        return false;
    }
    if root == "/" {
        return path.starts_with('/');
    }
    path == root
        || (path.len() > root.len()
            && path.as_bytes()[root.len()] == b'/'
            && path.starts_with(root))
}

const NETWORK_FS_TYPES: &[&str] = &["nfs", "nfs4", "cifs", "smb", "smbfs", "fuse.sshfs"];

pub fn is_network_mount(path: &Path) -> bool {
    match fs::read_to_string("/proc/mounts") {
        Ok(mounts) => is_network_mount_in(&path.to_string_lossy(), &mounts),
        Err(_) => false,
    }
}

/// Pure matcher: resolve `path` to its most specific (longest) mount point in
/// `mounts` (/proc/mounts format) and report whether that filesystem is a
/// network type. First-match-wins is wrong here: the root `/` line prefixes
/// every path and usually appears first, so the most specific mount must win
/// (e.g. `/mnt/syno` over `/`). Later lines win ties, matching mount order.
fn is_network_mount_in(path: &str, mounts: &str) -> bool {
    let mut best_len = 0usize;
    let mut best_fs = "";
    for line in mounts.lines() {
        let mut parts = line.split_whitespace();
        let (Some(_dev), Some(mount_point), Some(fs_type)) =
            (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if path_under_root(path, mount_point) && mount_point.len() >= best_len {
            best_len = mount_point.len();
            best_fs = fs_type;
        }
    }
    NETWORK_FS_TYPES.contains(&best_fs)
}

pub fn should_exclude(path: &Path, is_dir: bool) -> bool {
    // Name patterns are directory conventions (target, build, node_modules...);
    // only apply them to directories so a plain file that happens to share the
    // name (e.g. a script called "build") stays searchable.
    if is_dir {
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if EXCLUDE_PATTERNS
                .iter()
                .any(|p| name == *p || (p == &".Trash" && name.starts_with(".Trash")))
            {
                return true;
            }
        }
    }

    // Fast check: see if path string contains any excluded pattern
    // This catches files deep inside .steam, .cache, etc.
    let path_str = path.to_string_lossy();
    for pattern in EXCLUDE_PATTERNS {
        // Check for /pattern/ in the path (directory component)
        let search = format!("/{}/", pattern);
        if path_str.contains(&search) {
            return true;
        }
    }

    // User-configured recursive exclude paths from config.json.
    if is_user_excluded(&path_str) {
        return true;
    }

    false
}

/// Modification time in Unix seconds, or 0 if the platform does not report one.
pub fn mtime_of(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .map(|t| t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64)
        .unwrap_or(0)
}

/// The (mtime, size) the index records for `path`, from a single stat that
/// follows symlinks. Directories record size 0; a failed stat records zeros.
pub fn stat_entry(path: &Path, is_dir: bool) -> (i64, u64) {
    match path.metadata() {
        Ok(meta) => (mtime_of(&meta), if is_dir { 0 } else { meta.len() }),
        Err(_) => (0, 0),
    }
}

/// Index one entry and queue only the database write it needs. Most entries
/// on a rescan are unchanged; the read-locked check lets those skip the write
/// lock and the database entirely.
pub fn index_path(
    index: &Arc<RwLock<TrigramIndex>>,
    db_tx: &Sender<DbOp>,
    path: String,
    is_dir: bool,
    mtime: i64,
    size: u64,
) -> Upsert {
    let unchanged = {
        let idx = index.read().unwrap();
        idx.path_to_id
            .get(&path)
            .copied()
            .filter(|_| idx.is_current(&path, is_dir, mtime, size))
    };
    if let Some(id) = unchanged {
        return Upsert::Unchanged(id);
    }

    let outcome = {
        let mut idx = index.write().unwrap();
        idx.upsert(path.clone(), is_dir, mtime, size)
    };
    let entry = |id| FileEntry {
        id,
        path,
        is_dir,
        mtime,
        size,
    };
    match &outcome {
        Upsert::Unchanged(_) => {}
        Upsert::Updated(id) => {
            let _ = db_tx.send(DbOp::UpdateFile(entry(*id)));
        }
        Upsert::Added(id, _) => {
            let _ = db_tx.send(DbOp::SaveFile(entry(*id)));
        }
    }
    outcome
}

pub fn scan_directory(
    root: &Path,
    index: &Arc<RwLock<TrigramIndex>>,
    db_tx: &Sender<DbOp>,
) -> usize {
    let mut count = 0;
    let (mut added, mut updated) = (0usize, 0usize);
    let start = time::Instant::now();

    info!("Scanning directory: {}", root.display());

    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !should_exclude(e.path(), e.file_type().is_dir()));

    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();
        let is_dir = entry.file_type().is_dir();
        let (mtime, size) = stat_entry(path, is_dir);
        let path_str = path.to_string_lossy().to_string();
        match index_path(index, db_tx, path_str, is_dir, mtime, size) {
            Upsert::Added(..) => added += 1,
            Upsert::Updated(_) => updated += 1,
            Upsert::Unchanged(_) => {}
        }
        count += 1;
    }

    let elapsed = start.elapsed();
    info!(
        "Scanned {} files ({} new, {} updated) in {:?}",
        count, added, updated, elapsed
    );

    count
}

/// Reconcile directory: find and index files that aren't in the database yet.
/// This is faster than a full scan because it skips already-indexed files.
pub fn reconcile_directory(
    root: &Path,
    index: &Arc<RwLock<TrigramIndex>>,
    db_tx: &Sender<DbOp>,
) -> usize {
    let mut added = 0;
    let start = time::Instant::now();

    info!("Reconciling directory: {}", root.display());

    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !should_exclude(e.path(), e.file_type().is_dir()));

    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();
        let path_str = path.to_string_lossy().to_string();

        // Check if already indexed
        let already_indexed = {
            let idx = index.read().unwrap();
            idx.path_to_id.contains_key(&path_str)
        };

        if already_indexed {
            continue;
        }

        // New file - add it
        let is_dir = entry.file_type().is_dir();
        let (mtime, size) = stat_entry(path, is_dir);
        index_path(index, db_tx, path_str, is_dir, mtime, size);
        added += 1;
    }

    let elapsed = start.elapsed();
    if added > 0 {
        info!("Reconciliation added {} new files in {:?}", added, elapsed);
    } else {
        info!(
            "Reconciliation complete, no new files found ({:?})",
            elapsed
        );
    }

    added
}

#[cfg(test)]
mod tests {
    use super::{
        is_network_mount_in, path_under_any, path_under_root, reconcile_directory, scan_directory,
    };
    use crate::search::database::DbOp;
    use crate::search::trigram::TrigramIndex;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::mpsc::{channel, Receiver};
    use std::sync::{Arc, RwLock};

    fn excludes(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|s| s.to_string()).collect()
    }

    static TREE_COUNTER: AtomicU32 = AtomicU32::new(0);

    /// A small real directory tree: two files and a subdirectory holding one.
    fn temp_tree() -> PathBuf {
        let n = TREE_COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!("archnav_scan_{}_{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("a.txt"), "aaaa").unwrap();
        std::fs::write(root.join("b.txt"), "bb").unwrap();
        std::fs::write(root.join("sub").join("c.txt"), "c").unwrap();
        root
    }

    fn queued(rx: &Receiver<DbOp>) -> Vec<DbOp> {
        rx.try_iter().collect()
    }

    /// Regression: every periodic network rescan re-saved every entry with
    /// INSERT OR REPLACE, so an unchanged 127k-entry share cost 127k row
    /// rewrites (about 500 MiB of disk writes) every 5 minutes.
    #[test]
    fn rescan_of_unchanged_tree_queues_no_db_writes() {
        let root = temp_tree();
        let index = Arc::new(RwLock::new(TrigramIndex::new()));
        let (tx, rx) = channel();

        let first = scan_directory(&root, &index, &tx);
        assert_eq!(first, 5, "root, sub, and three files");
        assert_eq!(queued(&rx).len(), 5, "first scan saves every entry");

        scan_directory(&root, &index, &tx);
        assert_eq!(
            queued(&rx).len(),
            0,
            "a rescan with nothing changed must not touch the database"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Startup reconcile adds what the index lacks and nothing else; changes
    /// to files it already knows are left to rescans and the watcher.
    #[test]
    fn reconcile_indexes_only_paths_the_index_lacks() {
        let root = temp_tree();
        let index = Arc::new(RwLock::new(TrigramIndex::new()));
        let (tx, rx) = channel();
        scan_directory(&root, &index, &tx);
        queued(&rx);

        std::fs::write(root.join("sub").join("d.txt"), "d").unwrap();
        std::fs::write(root.join("b.txt"), "a different size").unwrap();
        assert_eq!(reconcile_directory(&root, &index, &tx), 1);

        let ops = queued(&rx);
        assert_eq!(ops.len(), 1);
        assert!(matches!(&ops[0], DbOp::SaveFile(e) if e.path.ends_with("/sub/d.txt")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rescan_queues_one_metadata_update_for_a_changed_file() {
        let root = temp_tree();
        let index = Arc::new(RwLock::new(TrigramIndex::new()));
        let (tx, rx) = channel();
        scan_directory(&root, &index, &tx);
        queued(&rx);

        // Rewriting an existing file changes its size but not its directory.
        std::fs::write(root.join("b.txt"), "longer").unwrap();
        scan_directory(&root, &index, &tx);

        let ops = queued(&rx);
        assert_eq!(ops.len(), 1, "only the changed file is written");
        assert!(
            matches!(&ops[0], DbOp::UpdateFile(e) if e.path.ends_with("/b.txt") && e.size == 6),
            "the change is an in-place metadata update, not a re-save"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn excludes_exact_path_and_descendants() {
        let ex = excludes(&["/home/u/Downloads"]);
        assert!(path_under_any("/home/u/Downloads", &ex)); // the folder itself
        assert!(path_under_any("/home/u/Downloads/a.zip", &ex)); // direct child
        assert!(path_under_any("/home/u/Downloads/deep/nested/x", &ex)); // recursive
    }

    #[test]
    fn does_not_exclude_siblings_sharing_a_prefix() {
        let ex = excludes(&["/home/u/Down"]);
        // "/home/u/Downloads" must NOT be excluded by the prefix "/home/u/Down".
        assert!(!path_under_any("/home/u/Downloads", &ex));
        assert!(!path_under_any("/home/u/Downloads/a", &ex));
        // The exact "/home/u/Down" and its real children still match.
        assert!(path_under_any("/home/u/Down", &ex));
        assert!(path_under_any("/home/u/Down/file", &ex));
    }

    #[test]
    fn does_not_exclude_unrelated_paths() {
        let ex = excludes(&["/home/u/Downloads", "/mnt/scratch"]);
        assert!(!path_under_any("/home/u/Documents/a", &ex));
        assert!(!path_under_any("/mnt/data/x", &ex));
    }

    #[test]
    fn empty_exclude_list_matches_nothing() {
        assert!(!path_under_any("/anything/at/all", &[]));
    }

    #[test]
    fn path_under_root_boundaries() {
        assert!(path_under_root("/mnt/data", "/mnt/data"));
        assert!(path_under_root("/mnt/data/x/y", "/mnt/data"));
        assert!(path_under_root("/mnt/data/x", "/mnt/data/")); // trailing slash tolerated
        assert!(!path_under_root("/mnt/database", "/mnt/data")); // sibling prefix
        assert!(!path_under_root("/mnt", "/mnt/data")); // parent of root
        assert!(path_under_root("/anything", "/"));
        assert!(!path_under_root("/anything", ""));
    }

    #[test]
    fn network_mount_uses_most_specific_mount_point() {
        // Regression: first-prefix-match always hit the root "/" line (which
        // prefixes every path), so cifs/nfs mounts were never detected and the
        // periodic network rescan never started.
        let mounts = "\
proc /proc proc rw 0 0
/dev/nvme1n1p2 / ext4 rw 0 0
/dev/sda1 /mnt/data ext4 rw 0 0
//192.168.1.4/syno /mnt/syno cifs rw 0 0
";
        assert!(is_network_mount_in("/mnt/syno", mounts));
        assert!(is_network_mount_in("/mnt/syno/videos/x.mkv", mounts));
        assert!(!is_network_mount_in("/mnt/data", mounts));
        assert!(!is_network_mount_in("/mnt/data/file", mounts));
        assert!(!is_network_mount_in("/mnt/synology", mounts)); // boundary, falls to "/"
        assert!(!is_network_mount_in("/home/u/file", mounts));
    }
}
