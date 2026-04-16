use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tracing::{debug, info, warn};

use super::database::DbOp;
use super::scanner::{get_mtime, is_network_mount, should_exclude};
use super::trigram::{FileEntry, TrigramIndex};

pub fn start_watcher(
    paths: Vec<PathBuf>,
    index: Arc<RwLock<TrigramIndex>>,
    db_tx: Sender<DbOp>,
) -> notify::Result<RecommendedWatcher> {
    let index_clone = index.clone();
    let db_tx_clone = db_tx.clone();

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            debug!("Watcher event received: {:?}", res);
            match res {
                Ok(event) => handle_fs_event(event, &index_clone, &db_tx_clone),
                Err(e) => warn!("Watch error: {:?}", e),
            }
        },
        Config::default().with_poll_interval(Duration::from_secs(2)),
    )?;

    for path in paths {
        if !is_network_mount(&path) {
            info!("Setting up inotify watch for: {}", path.display());
            match watcher.watch(&path, RecursiveMode::Recursive) {
                Ok(()) => info!("Successfully watching: {}", path.display()),
                Err(e) => {
                    // Don't fail entirely for permission errors - some subdirs may be inaccessible
                    // The watcher will still work for accessible directories
                    warn!("Partial watch failure for {} (continuing anyway): {:?}", path.display(), e);
                }
            }
        } else {
            info!("Skipping inotify for network mount: {}", path.display());
        }
    }

    Ok(watcher)
}

fn handle_fs_event(event: Event, index: &Arc<RwLock<TrigramIndex>>, db_tx: &Sender<DbOp>) {
    use notify::EventKind::*;

    match event.kind {
        Create(_) | Modify(_) => {
            for path in event.paths {
                if should_exclude(&path) {
                    continue;
                }
                if let Ok(meta) = path.metadata() {
                    let path_str = path.to_string_lossy().to_string();
                    let is_dir = meta.is_dir();
                    let mtime = get_mtime(&path);
                    let size = if is_dir { 0 } else { meta.len() };

                    let (id, trigrams) = {
                        let mut idx = index.write().unwrap();
                        idx.add(path_str.clone(), is_dir, mtime, size)
                    };

                    let entry = FileEntry {
                        id,
                        path: path_str,
                        is_dir,
                        mtime,
                        size,
                    };
                    let _ = db_tx.send(DbOp::SaveFile(entry, trigrams));
                    debug!("Indexed: {}", path.display());
                }
            }
        }
        Remove(_) => {
            for path in event.paths {
                let path_str = path.to_string_lossy().to_string();
                {
                    let mut idx = index.write().unwrap();
                    idx.remove(&path_str);
                }
                let _ = db_tx.send(DbOp::RemoveFile(path_str));
                debug!("Removed: {}", path.display());
            }
        }
        _ => {}
    }
}
