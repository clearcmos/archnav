use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;
use tracing::info;

use super::database::DbOp;
use super::scanner::{is_network_mount, scan_directory};
use super::trigram::TrigramIndex;

const INTEGRITY_CHECK_INTERVAL_SECS: u64 = 60;
const INTEGRITY_BATCH_SIZE: usize = 5000;
const NETWORK_SCAN_INTERVAL_SECS: u64 = 300;

pub fn start_integrity_checker(index: Arc<RwLock<TrigramIndex>>, db_tx: Sender<DbOp>) {
    thread::spawn(move || {
        let mut offset = 0;

        loop {
            thread::sleep(Duration::from_secs(INTEGRITY_CHECK_INTERVAL_SECS));

            let paths_to_check: Vec<String> = {
                let idx = index.read().unwrap();
                let all_paths: Vec<_> = idx.files.values().map(|f| f.path.clone()).collect();

                if all_paths.is_empty() {
                    continue;
                }

                if offset >= all_paths.len() {
                    offset = 0;
                }

                let end = (offset + INTEGRITY_BATCH_SIZE).min(all_paths.len());
                let batch = all_paths[offset..end].to_vec();
                offset = end;
                batch
            };

            let mut removed_count = 0;
            for path_str in paths_to_check {
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

pub fn start_network_scanner(
    paths: Vec<PathBuf>,
    index: Arc<RwLock<TrigramIndex>>,
    db_tx: Sender<DbOp>,
) {
    let network_paths: Vec<PathBuf> = paths.into_iter().filter(|p| is_network_mount(p)).collect();

    if network_paths.is_empty() {
        return;
    }

    thread::spawn(move || loop {
        for path in &network_paths {
            info!("Periodic scan of network mount: {}", path.display());
            scan_directory(path, &index, &db_tx);
        }
        thread::sleep(Duration::from_secs(NETWORK_SCAN_INTERVAL_SECS));
    });
}
