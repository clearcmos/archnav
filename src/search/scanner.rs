use std::path::Path;
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};
use std::time::UNIX_EPOCH;
use std::{fs, time};
use tracing::info;
use walkdir::WalkDir;

use super::database::DbOp;
use super::trigram::{FileEntry, TrigramIndex, EXCLUDE_PATTERNS};

pub fn is_network_mount(path: &Path) -> bool {
    let path_str = path.to_string_lossy();

    if path_str.starts_with("/mnt/")
        || path_str.starts_with("/media/")
        || path_str.starts_with("/net/")
    {
        if let Ok(mounts) = fs::read_to_string("/proc/mounts") {
            for line in mounts.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    let mount_point = parts[1];
                    let fs_type = parts[2];

                    if path_str.starts_with(mount_point) {
                        return matches!(
                            fs_type,
                            "nfs" | "nfs4" | "cifs" | "smb" | "smbfs" | "fuse.sshfs"
                        );
                    }
                }
            }
        }
    }
    false
}

pub fn should_exclude(path: &Path) -> bool {
    // Check if the file/dir name itself matches
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if EXCLUDE_PATTERNS
            .iter()
            .any(|p| name == *p || (p == &".Trash" && name.starts_with(".Trash")))
        {
            return true;
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

    false
}

pub fn get_mtime(path: &Path) -> i64 {
    path.metadata()
        .and_then(|m| m.modified())
        .map(|t| t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64)
        .unwrap_or(0)
}

pub fn get_size(path: &Path) -> u64 {
    path.metadata().map(|m| m.len()).unwrap_or(0)
}

pub fn scan_directory(
    root: &Path,
    index: &Arc<RwLock<TrigramIndex>>,
    db_tx: &Sender<DbOp>,
) -> usize {
    let mut count = 0;
    let start = time::Instant::now();

    info!("Scanning directory: {}", root.display());

    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !should_exclude(e.path()));

    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();
        let path_str = path.to_string_lossy().to_string();
        let is_dir = entry.file_type().is_dir();
        let mtime = get_mtime(path);
        let size = if is_dir { 0 } else { get_size(path) };

        let (id, trigrams) = {
            let mut idx = index.write().unwrap();
            idx.add(path_str.clone(), is_dir, mtime, size)
        };

        let file_entry = FileEntry {
            id,
            path: path_str,
            is_dir,
            mtime,
            size,
        };
        let _ = db_tx.send(DbOp::SaveFile(file_entry, trigrams));
        count += 1;
    }

    let elapsed = start.elapsed();
    info!("Scanned {} files in {:?}", count, elapsed);

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
        .filter_entry(|e| !should_exclude(e.path()));

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
        let mtime = get_mtime(path);
        let size = if is_dir { 0 } else { get_size(path) };

        let (id, trigrams) = {
            let mut idx = index.write().unwrap();
            idx.add(path_str.clone(), is_dir, mtime, size)
        };

        let file_entry = FileEntry {
            id,
            path: path_str,
            is_dir,
            mtime,
            size,
        };
        let _ = db_tx.send(DbOp::SaveFile(file_entry, trigrams));
        added += 1;
    }

    let elapsed = start.elapsed();
    if added > 0 {
        info!("Reconciliation added {} new files in {:?}", added, elapsed);
    } else {
        info!("Reconciliation complete, no new files found ({:?})", elapsed);
    }

    added
}
