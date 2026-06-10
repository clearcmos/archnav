use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;
use tracing::info;

use super::database::DbOp;
use super::scanner::{path_under_root, scan_directory};
use super::trigram::TrigramIndex;

const INTEGRITY_CHECK_INTERVAL_SECS: u64 = 60;
const INTEGRITY_BATCH_SIZE: usize = 5000;
const NETWORK_SCAN_INTERVAL_SECS: u64 = 300;

pub fn start_integrity_checker(index: Arc<RwLock<TrigramIndex>>, db_tx: Sender<DbOp>) {
    thread::spawn(move || {
        let mut offset = 0;

        loop {
            thread::sleep(Duration::from_secs(INTEGRITY_CHECK_INTERVAL_SECS));

            // Bookmark roots that are currently unreachable (e.g. an unmounted
            // network share). Entries beneath them must not be purged: the
            // files still exist, the mount is just absent right now, and
            // nothing would re-add them until a restart or manual rescan.
            let (paths_to_check, unavailable_roots): (Vec<String>, Vec<String>) = {
                let idx = index.read().unwrap();

                if idx.files.is_empty() {
                    continue;
                }

                let unavailable: Vec<String> = idx
                    .bookmarks
                    .iter()
                    .filter(|b| !Path::new(&b.path).exists())
                    .map(|b| b.path.clone())
                    .collect();

                if offset >= idx.files.len() {
                    offset = 0;
                }

                // Clone only this cycle's batch, not every indexed path.
                // HashMap iteration order is not stable across mutations, so
                // this is a statistical sweep rather than a strict rotation.
                let batch: Vec<String> = idx
                    .files
                    .values()
                    .skip(offset)
                    .take(INTEGRITY_BATCH_SIZE)
                    .map(|f| f.path.clone())
                    .collect();
                offset += batch.len();
                (batch, unavailable)
            };

            let mut removed_count = 0;
            for path_str in paths_to_check {
                if unavailable_roots
                    .iter()
                    .any(|root| path_under_root(&path_str, root))
                {
                    continue;
                }
                let path = Path::new(&path_str);
                if !path.exists() {
                    {
                        let mut idx = index.write().unwrap();
                        idx.remove(&path_str);
                    }
                    let _ = db_tx.send(DbOp::RemoveFile(path_str));
                    removed_count += 1;
                }
            }

            if removed_count > 0 {
                info!("Integrity check: removed {} stale entries", removed_count);
            }
        }
    });
}

/// Periodically rescan network bookmark paths. inotify cannot see remote
/// changes, so polling is the only way these stay fresh. The caller decides
/// which paths are network (config flag or mount-table detection); no
/// re-filtering happens here.
pub fn start_network_scanner(
    network_paths: Vec<PathBuf>,
    index: Arc<RwLock<TrigramIndex>>,
    db_tx: Sender<DbOp>,
) {
    if network_paths.is_empty() {
        return;
    }

    thread::spawn(move || loop {
        // Sleep first: the startup scan/reconcile already covered these paths.
        thread::sleep(Duration::from_secs(NETWORK_SCAN_INTERVAL_SECS));
        for path in &network_paths {
            if !path.exists() {
                info!("Skipping network scan, mount absent: {}", path.display());
                continue;
            }
            info!("Periodic scan of network mount: {}", path.display());
            scan_directory(path, &index, &db_tx);
        }
    });
}
