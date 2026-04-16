use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use std::thread;
use notify::RecommendedWatcher;
use tracing::{info, warn, debug};

use super::database::{Database, DbOp, start_db_thread};
use super::integrity::{start_integrity_checker, start_network_scanner};
use super::query::{ParsedQuery, SortOrder};
use super::scanner::{scan_directory, reconcile_directory};
use super::trigram::{Bookmark, SearchAllResult, TrigramIndex};
use super::watcher::start_watcher;

/// Cache for incremental search refinement.
/// When user types "con" -> "conf" -> "config", we reuse previous results.
struct SearchCache {
    query: String,
    sort_order: SortOrder,
    results: Vec<SearchAllResult>,
    timestamp: Instant,
    /// Sequence number to prevent stale overwrites from slow searches
    seq: u64,
    /// Whether results were truncated (at MAX_RESULTS) - if so, can't safely filter
    was_truncated: bool,
}

impl SearchCache {
    /// Check if this cache can be used for a new query.
    /// Valid if: new query starts with cached query, same sort order, fresh enough,
    /// AND results weren't truncated (otherwise filtering might miss matches).
    fn is_valid_for(&self, new_query: &str, new_sort: SortOrder) -> bool {
        // Can't use truncated cache - refinement might miss results
        if self.was_truncated {
            return false;
        }

        // Don't cache fuzzy queries - the edit distance threshold changes with length
        // so cached results for "~abc" aren't valid for "~abcd"
        if new_query.starts_with('~') || self.query.starts_with('~') {
            return false;
        }

        // Don't use cache when wildcards are involved - parsing semantics change
        // e.g., "marois *" is a glob, but "marois *.pdf" is substring + extension filter
        if self.query.contains('*') || self.query.contains('?') {
            return false;
        }

        // Must be same sort order
        if self.sort_order != new_sort {
            return false;
        }

        // Cache must be fresh (within 5 seconds)
        if self.timestamp.elapsed() > Duration::from_secs(5) {
            return false;
        }

        // New query must start with cached query (refinement)
        let cached_lower = self.query.to_lowercase();
        let new_lower = new_query.to_lowercase();

        // Must be a strict extension (not same query, not shorter)
        new_lower.starts_with(&cached_lower) && new_lower.len() > cached_lower.len()
    }
}

/// Core search engine that owns the index, database, and background threads.
/// This is the non-Qt service layer used by the bridge QObject.
pub struct CoreEngine {
    pub index: Arc<RwLock<TrigramIndex>>,
    db_tx: Sender<DbOp>,
    _watcher: Arc<Mutex<Option<RecommendedWatcher>>>,
    /// Cache for incremental query refinement
    search_cache: RwLock<Option<SearchCache>>,
    /// Sequence counter to prevent stale cache updates from slow searches
    search_seq: std::sync::atomic::AtomicU64,
}

impl CoreEngine {
    /// Create a new engine, load from database, start background threads.
    pub fn new(bookmarks: Vec<Bookmark>) -> Self {
        let db = Database::open().expect("Failed to open database");
        let index = Arc::new(RwLock::new(TrigramIndex::new()));

        // Load existing index from database
        {
            let mut idx = index.write().unwrap();
            match db.load_into_index(&mut idx) {
                Ok(count) => info!("Loaded {} files from database", count),
                Err(e) => warn!("Failed to load index from database: {}", e),
            }

            // Merge config bookmarks with DB bookmarks:
            // - Config is the source of truth for which bookmarks should exist
            // - Add any config bookmarks missing from DB
            // - Remove any DB bookmarks not in config
            if idx.bookmarks.is_empty() {
                idx.bookmarks = bookmarks;
            } else {
                // Add new bookmarks from config that aren't in DB
                for bm in &bookmarks {
                    if !idx.bookmarks.iter().any(|b| b.name == bm.name) {
                        info!("Adding new bookmark from config: {} -> {}", bm.name, bm.path);
                        idx.bookmarks.push(bm.clone());
                    }
                }
                // Remove DB bookmarks that are no longer in config
                idx.bookmarks.retain(|b| {
                    let keep = bookmarks.iter().any(|cb| cb.name == b.name);
                    if !keep {
                        info!("Removing bookmark not in config: {}", b.name);
                    }
                    keep
                });
            }
        }

        // Start dedicated database writer thread
        let db_tx = start_db_thread(db);

        // Persist bookmarks to DB
        {
            let idx = index.read().unwrap();
            for bookmark in &idx.bookmarks {
                let _ = db_tx.send(DbOp::SaveBookmark(bookmark.clone()));
            }
        }

        // Report engine ready immediately after index load
        let file_count = index.read().unwrap().file_count();
        info!("Engine ready with {} indexed files", file_count);

        // Watcher will be set up asynchronously
        let watcher_holder: Arc<Mutex<Option<RecommendedWatcher>>> = Arc::new(Mutex::new(None));

        // Spawn background thread for watcher setup, integrity checker, network scanner, and initial scan
        {
            let index = index.clone();
            let db_tx = db_tx.clone();
            let watcher_holder = watcher_holder.clone();

            thread::spawn(move || {
                // Collect paths for watchers/scanners
                let all_paths: Vec<PathBuf> = index
                    .read()
                    .unwrap()
                    .bookmarks
                    .iter()
                    .map(|b| PathBuf::from(&b.path))
                    .collect();

                // Local-only paths for inotify (skip bookmarks marked as network)
                let local_paths: Vec<PathBuf> = index
                    .read()
                    .unwrap()
                    .bookmarks
                    .iter()
                    .filter(|b| !b.is_network)
                    .map(|b| PathBuf::from(&b.path))
                    .collect();

                // Start inotify watcher (local paths only)
                match start_watcher(local_paths, index.clone(), db_tx.clone()) {
                    Ok(w) => {
                        info!("File watcher setup complete");
                        *watcher_holder.lock().unwrap() = Some(w);
                    }
                    Err(e) => {
                        warn!("Failed to start file watcher: {}", e);
                    }
                };

                // Start background integrity checker + network scanner
                start_integrity_checker(index.clone(), db_tx.clone());
                start_network_scanner(all_paths.clone(), index.clone(), db_tx.clone());

                // Initial scan if database was empty, otherwise reconcile to find new files
                let needs_scan = index.read().unwrap().file_count() == 0;
                if needs_scan {
                    info!("Database empty, performing initial scan");
                    for path in &all_paths {
                        scan_directory(path, &index, &db_tx);
                    }
                } else {
                    // Reconcile: find files added while archnav wasn't running
                    info!("Reconciling index with filesystem...");
                    for path in &all_paths {
                        reconcile_directory(path, &index, &db_tx);
                    }
                }
            });
        }

        Self {
            index,
            db_tx,
            _watcher: watcher_holder,
            search_cache: RwLock::new(None),
            search_seq: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Search across all locations. Returns results and elapsed time.
    /// Uses incremental caching: if query refines previous query, filters cached results.
    pub fn search(
        &self,
        raw_query: &str,
        sort_index: i32,
    ) -> (Vec<SearchAllResult>, std::time::Duration) {
        use std::sync::atomic::Ordering;

        let start = Instant::now();
        let sort_order = SortOrder::from_index(sort_index);

        // Get a sequence number for this search
        let my_seq = self.search_seq.fetch_add(1, Ordering::SeqCst) + 1;

        debug!("[ENGINE] search start: query='{}', seq={}", raw_query, my_seq);

        // Check if we can use cached results
        // Extract cache data while holding read lock, then release before write
        let cache_hit_data: Option<(String, Vec<SearchAllResult>)> = {
            let cache_guard = self.search_cache.read().unwrap();
            if let Some(ref cache) = *cache_guard {
                if cache.is_valid_for(raw_query, sort_order) {
                    Some((cache.query.clone(), cache.results.clone()))
                } else {
                    None
                }
            } else {
                None
            }
        }; // Read lock released here

        if let Some((cached_query, cached_results)) = cache_hit_data {
            // Filter cached results instead of full search
            let query = ParsedQuery::parse(raw_query, sort_order);
            let results: Vec<SearchAllResult> = cached_results
                .iter()
                .filter(|r| query.matches_path(&r.path))
                .cloned()
                .collect();

            debug!(
                "[ENGINE] Cache HIT: '{}' -> '{}', seq={}, filtered {} -> {} results",
                cached_query,
                raw_query,
                my_seq,
                cached_results.len(),
                results.len()
            );

            // Update cache with refined results, but only if this is still the most recent search
            let was_truncated = results.len() >= super::trigram::MAX_RESULTS;
            {
                let mut cache_guard = self.search_cache.write().unwrap();
                let should_update = match cache_guard.as_ref() {
                    None => true,
                    Some(existing) => my_seq > existing.seq,
                };
                if should_update {
                    *cache_guard = Some(SearchCache {
                        query: raw_query.to_string(),
                        sort_order,
                        results: results.clone(),
                        timestamp: Instant::now(),
                        seq: my_seq,
                        was_truncated,
                    });
                } else {
                    debug!("[ENGINE] Cache HIT but not updating (seq {} <= existing)", my_seq);
                }
            }

            return (results, start.elapsed());
        }

        // Full search
        let idx = self.index.read().unwrap();
        let query = ParsedQuery::parse(raw_query, sort_order);
        debug!("[ENGINE] Cache MISS: '{}', seq={}, doing full search", raw_query, my_seq);
        let results = idx.search_all(&query, &[]);
        debug!("[ENGINE] Full search complete: '{}', seq={}, results={}", raw_query, my_seq, results.len());

        // Only update cache if this is still the most recent search
        // This prevents slow searches from overwriting newer results
        {
            let mut cache_guard = self.search_cache.write().unwrap();
            let should_update = match cache_guard.as_ref() {
                None => true,
                Some(existing) => my_seq > existing.seq,
            };

            if should_update {
                let was_truncated = results.len() >= super::trigram::MAX_RESULTS;
                *cache_guard = Some(SearchCache {
                    query: raw_query.to_string(),
                    sort_order,
                    results: results.clone(),
                    timestamp: Instant::now(),
                    seq: my_seq,
                    was_truncated,
                });
            } else {
                debug!("Discarding stale search results for '{}' (seq {} < cached)", raw_query, my_seq);
            }
        }

        (results, start.elapsed())
    }

    /// Get current bookmark list.
    pub fn bookmarks(&self) -> Vec<Bookmark> {
        self.index.read().unwrap().bookmarks.clone()
    }

    /// Add a new bookmark and scan its path.
    pub fn add_bookmark(&self, name: &str, path: &str, is_network: bool) {
        let bookmark = Bookmark {
            name: name.to_string(),
            path: path.to_string(),
            is_network,
        };

        {
            let mut idx = self.index.write().unwrap();
            idx.bookmarks.push(bookmark.clone());
        }

        let _ = self.db_tx.send(DbOp::SaveBookmark(bookmark));

        let path_buf = PathBuf::from(path);
        scan_directory(&path_buf, &self.index, &self.db_tx);
    }

    /// Remove a bookmark and all its indexed files.
    pub fn remove_bookmark(&self, name: &str) {
        let removed_path = {
            let mut idx = self.index.write().unwrap();
            if let Some(pos) = idx.bookmarks.iter().position(|b| b.name == name) {
                let bookmark = idx.bookmarks.remove(pos);
                Some(bookmark.path)
            } else {
                None
            }
        };

        if let Some(path) = removed_path {
            let _ = self.db_tx.send(DbOp::ClearFilesUnder(path.clone()));

            // Remove from in-memory index
            let to_remove: Vec<String> = {
                let idx = self.index.read().unwrap();
                idx.files
                    .values()
                    .filter(|f| f.path.starts_with(&path))
                    .map(|f| f.path.clone())
                    .collect()
            };
            let mut idx = self.index.write().unwrap();
            for p in to_remove {
                idx.remove(&p);
            }
        }
    }

    /// Rename a bookmark.
    pub fn rename_bookmark(&self, old_name: &str, new_name: &str) {
        let mut idx = self.index.write().unwrap();
        if let Some(bookmark) = idx.bookmarks.iter_mut().find(|b| b.name == old_name) {
            bookmark.name = new_name.to_string();
            let _ = self.db_tx.send(DbOp::SaveBookmark(bookmark.clone()));
        }
    }

    /// Rescan all bookmarks.
    pub fn rescan_all(&self) {
        let bookmarks = self.bookmarks();
        for bookmark in &bookmarks {
            let path = PathBuf::from(&bookmark.path);
            info!("Rescanning: {}", bookmark.path);
            scan_directory(&path, &self.index, &self.db_tx);
        }
    }

    /// Total number of indexed files.
    pub fn file_count(&self) -> usize {
        self.index.read().unwrap().file_count()
    }

    /// Record a file open event for frecency tracking.
    pub fn record_file_open(&self, path: &str) {
        let file_id = {
            let idx = self.index.read().unwrap();
            idx.path_to_id.get(path).copied()
        };

        if let Some(id) = file_id {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            // Update in-memory access data
            {
                let mut idx = self.index.write().unwrap();
                let access = idx.access_data.entry(id).or_default();
                access.open_count += 1;
                access.last_opened = now;
            }

            // Persist to database
            let _ = self.db_tx.send(super::database::DbOp::RecordFileOpen(id, now));
        }
    }

    /// Rescan a specific path (for IPC RESCAN command).
    pub fn rescan_path(&self, path: &str) -> usize {
        let path_buf = PathBuf::from(path);
        scan_directory(&path_buf, &self.index, &self.db_tx)
    }

    /// Test search from CLI - returns result count
    /// Clear the search cache (used by benchmarks to measure cold-search performance).
    pub fn clear_search_cache(&self) {
        *self.search_cache.write().unwrap() = None;
    }

    pub fn test_search(&self, query: &str) -> usize {
        let (results, elapsed) = self.search(query, 0);
        println!("Search '{}': {} results in {:?}", query, results.len(), elapsed);
        if !results.is_empty() {
            for r in results.iter().take(3) {
                println!("  - {}", r.path);
            }
            if results.len() > 3 {
                println!("  ... and {} more", results.len() - 3);
            }
        }
        results.len()
    }
}
