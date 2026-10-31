use notify::RecommendedWatcher;
use rayon::prelude::*;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use super::database::{start_db_thread, Database, DbOp};
use super::integrity::{start_integrity_checker, start_network_scanner};
use super::query::{ParsedQuery, SortOrder};
use super::scanner::{is_network_mount, path_under_root, reconcile_directory, scan_directory};
use super::trigram::{Bookmark, SearchResult, TrigramIndex};
use super::watcher::start_watcher;

/// Search results, shared between the cache and whoever displays them.
pub type Results = Arc<Vec<SearchResult>>;

/// Free `value` on a pool thread. Dropping the last handle on a million
/// results releases a million shared paths, which takes longer than the
/// search that replaced them.
pub fn drop_in_background<T: Send + 'static>(value: T) {
    rayon::spawn(move || drop(value));
}

/// Cache for incremental search refinement.
/// When user types "con" -> "conf" -> "config", we reuse previous results.
struct SearchCache {
    query: String,
    sort_order: SortOrder,
    results: Results,
    timestamp: Instant,
    /// Sequence number to prevent stale overwrites from slow searches
    seq: u64,
    /// Whether results were truncated at max_results - if so, can't safely filter
    was_truncated: bool,
}

impl SearchCache {
    /// Check if this cache can be used for a new query.
    /// Valid only if every result of the new query must be present in the cached
    /// result set. We compare the *parsed* form of both queries, not raw strings,
    /// because prefixes like `folder:` change parse semantics rather than extending
    /// the substring (e.g. `folder` -> `folder:mar` is NOT a refinement: the new
    /// substring is "mar", not "folder:mar", and the file-type filter changed).
    fn is_valid_for(&self, new_query: &str, new_sort: SortOrder) -> bool {
        // Can't use truncated cache - refinement might miss matches
        if self.was_truncated {
            return false;
        }
        if self.sort_order != new_sort {
            return false;
        }
        if self.timestamp.elapsed() > Duration::from_secs(5) {
            return false;
        }

        let old = ParsedQuery::parse(&self.query, self.sort_order);
        let new = ParsedQuery::parse(new_query, new_sort);

        // Only plain word searches refine (fuzzy thresholds shift with length;
        // glob/regex don't string-extend), and only when every filter is the
        // same and the new words and `!words` can only remove results.
        new.narrows(&old)
    }
}

/// A count with thousands separators: 132402 -> "132,402".
pub fn group_digits(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The status bar text after a search. Says how many paths matched in all
/// when a result cap cut the list short.
pub fn results_status(shown: usize, total: usize, millis: u128) -> String {
    if total > shown {
        format!(
            "{} of {} results in {}ms",
            group_digits(shown),
            group_digits(total),
            millis
        )
    } else {
        format!("{} results in {}ms", group_digits(shown), millis)
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
    /// Per-search result cap from config; 0 returns every match
    max_results: usize,
}

impl CoreEngine {
    /// Create a new engine, load from database, start background threads.
    /// `exclude_paths` are recursive blacklist locations from config (already
    /// normalized via `AppConfig::expanded_exclude_paths`). `max_results` is
    /// the per-search result cap from config; 0 returns every match.
    pub fn new(bookmarks: Vec<Bookmark>, exclude_paths: Vec<String>, max_results: usize) -> Self {
        // Install the user's recursive exclude paths before anything scans, so
        // the initial scan, reconcile, network rescans, and the watcher all
        // skip them via the shared should_exclude chokepoint.
        super::scanner::set_exclude_paths(exclude_paths);

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
                        info!(
                            "Adding new bookmark from config: {} -> {}",
                            bm.name, bm.path
                        );
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

        // Drop any files indexed before their location was added to the exclude
        // list. Going forward the scanner skips excluded paths, but entries
        // indexed earlier still exist on disk, so the integrity checker won't
        // clear them - purge them here so a new exclude takes effect on restart.
        if super::scanner::has_user_excludes() {
            let to_purge: Vec<String> = {
                let idx = index.read().unwrap();
                idx.files
                    .values()
                    .filter(|f| super::scanner::is_user_excluded(&f.path))
                    .map(|f| f.path.to_string())
                    .collect()
            };
            if !to_purge.is_empty() {
                info!(
                    "Purging {} indexed files now covered by exclude_paths",
                    to_purge.len()
                );
                let mut idx = index.write().unwrap();
                for path in &to_purge {
                    idx.remove(path);
                    let _ = db_tx.send(DbOp::RemoveFile(path.clone()));
                }
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
                // Split bookmark paths into local (inotify) and network
                // (periodic rescan). The config is_network flag is
                // authoritative; mount-table detection catches network mounts
                // the user didn't flag.
                let mut local_paths: Vec<PathBuf> = Vec::new();
                let mut network_paths: Vec<PathBuf> = Vec::new();
                {
                    let idx = index.read().unwrap();
                    for b in &idx.bookmarks {
                        let path = PathBuf::from(&b.path);
                        if b.is_network || is_network_mount(&path) {
                            network_paths.push(path);
                        } else {
                            local_paths.push(path);
                        }
                    }
                }
                let all_paths: Vec<PathBuf> = local_paths
                    .iter()
                    .chain(network_paths.iter())
                    .cloned()
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
                start_network_scanner(network_paths, index.clone(), db_tx.clone());

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
            max_results,
        }
    }

    /// Run a search across all bookmarks: the results (capped at
    /// max_results), how many paths matched in total, and how long it took.
    /// When the query refines the previous one, filters its cached results.
    pub fn search(&self, raw_query: &str, sort_index: i32) -> (Results, usize, Duration) {
        use std::sync::atomic::Ordering;

        let start = Instant::now();
        let sort_order = SortOrder::from_index(sort_index);

        // Get a sequence number for this search
        let my_seq = self.search_seq.fetch_add(1, Ordering::SeqCst) + 1;

        debug!(
            "[ENGINE] search start: query='{}', seq={}",
            raw_query, my_seq
        );

        // Check if we can use cached results
        // Extract cache data while holding read lock, then release before write
        let cache_hit_data: Option<(String, Results)> = {
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
            let dirs_only = query.dirs_only();
            let results: Vec<SearchResult> = cached_results
                .par_iter()
                .filter(|r| {
                    if dirs_only && !r.is_dir {
                        return false;
                    }
                    query.matches_result(&r.path, r.is_dir, r.size, r.mtime)
                })
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

            // The cache held every match of a broader query, so the filtered
            // set is every match of this one: complete, never truncated.
            let total = results.len();
            let results = Arc::new(results);
            {
                let mut cache_guard = self.search_cache.write().unwrap();
                let should_update = match cache_guard.as_ref() {
                    None => true,
                    Some(existing) => my_seq > existing.seq,
                };
                if should_update {
                    drop_in_background(cache_guard.replace(SearchCache {
                        query: raw_query.to_string(),
                        sort_order,
                        results: Arc::clone(&results),
                        timestamp: Instant::now(),
                        seq: my_seq,
                        was_truncated: false,
                    }));
                } else {
                    debug!(
                        "[ENGINE] Cache HIT but not updating (seq {} <= existing)",
                        my_seq
                    );
                }
            }

            drop_in_background(cached_results);
            return (results, total, start.elapsed());
        }

        // Full search
        let idx = self.index.read().unwrap();
        let query = ParsedQuery::parse(raw_query, sort_order);
        debug!(
            "[ENGINE] Cache MISS: '{}', seq={}, doing full search",
            raw_query, my_seq
        );
        let (results, total) = idx.search_all_counted(&query, &[], self.max_results);
        drop(idx);
        let results = Arc::new(results);
        debug!(
            "[ENGINE] Full search complete: '{}', seq={}, results={}",
            raw_query,
            my_seq,
            results.len()
        );

        // Only update cache if this is still the most recent search
        // This prevents slow searches from overwriting newer results
        {
            let mut cache_guard = self.search_cache.write().unwrap();
            let should_update = match cache_guard.as_ref() {
                None => true,
                Some(existing) => my_seq > existing.seq,
            };

            if should_update {
                let was_truncated = total > results.len();
                drop_in_background(cache_guard.replace(SearchCache {
                    query: raw_query.to_string(),
                    sort_order,
                    results: Arc::clone(&results),
                    timestamp: Instant::now(),
                    seq: my_seq,
                    was_truncated,
                }));
            } else {
                debug!(
                    "Discarding stale search results for '{}' (seq {} < cached)",
                    raw_query, my_seq
                );
            }
        }

        (results, total, start.elapsed())
    }

    /// Get current bookmark list.
    pub fn bookmarks(&self) -> Vec<Bookmark> {
        self.index.read().unwrap().bookmarks.clone()
    }

    /// Mirror the current bookmark list into config.json. The config is the
    /// source of truth at startup (bookmarks missing from it are removed from
    /// the DB), so runtime bookmark changes must be written back or they would
    /// silently vanish on the next launch.
    fn persist_bookmarks_to_config(&self) {
        let bookmarks = self.bookmarks();
        let mut config = crate::config::AppConfig::load();
        config.bookmarks = bookmarks
            .iter()
            .map(|b| crate::config::BookmarkConfig {
                name: b.name.clone(),
                path: b.path.clone(),
                is_network: b.is_network,
            })
            .collect();
        config.save();
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
        self.persist_bookmarks_to_config();

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
            self.persist_bookmarks_to_config();

            // Remove from in-memory index (boundary-aware: removing
            // "/mnt/data" must not purge "/mnt/database")
            let to_remove: Vec<String> = {
                let idx = self.index.read().unwrap();
                idx.files
                    .values()
                    .filter(|f| path_under_root(&f.path, &path))
                    .map(|f| f.path.to_string())
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
        let renamed = {
            let mut idx = self.index.write().unwrap();
            if let Some(bookmark) = idx.bookmarks.iter_mut().find(|b| b.name == old_name) {
                bookmark.name = new_name.to_string();
                let _ = self.db_tx.send(DbOp::SaveBookmark(bookmark.clone()));
                true
            } else {
                false
            }
        };
        if renamed {
            self.persist_bookmarks_to_config();
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
            let _ = self
                .db_tx
                .send(super::database::DbOp::RecordFileOpen(id, now));
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
        let (results, total, elapsed) = self.search(query, 0);
        println!(
            "Search '{}': {} of {} matches in {:?}",
            query,
            results.len(),
            total,
            elapsed
        );
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

#[cfg(test)]
mod cache_tests {
    use super::*;

    fn cache(query: &str) -> SearchCache {
        SearchCache {
            query: query.to_string(),
            sort_order: SortOrder::MtimeDesc,
            results: Arc::default(),
            timestamp: Instant::now(),
            seq: 0,
            was_truncated: false,
        }
    }

    #[test]
    fn exclusions_refine_only_when_they_remove_results() {
        assert!(cache("movies").is_valid_for("movies !trailer", SortOrder::MtimeDesc));
        assert!(!cache("movies !t").is_valid_for("movies !tr", SortOrder::MtimeDesc));
        assert!(cache("left").is_valid_for("left all", SortOrder::MtimeDesc));
    }

    #[test]
    fn status_line_reports_the_true_match_count() {
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(999), "999");
        assert_eq!(group_digits(1000), "1,000");
        assert_eq!(group_digits(1441511), "1,441,511");
        assert_eq!(results_status(3, 3, 0), "3 results in 0ms");
        assert_eq!(
            results_status(500, 132402, 12),
            "500 of 132,402 results in 12ms"
        );
    }

    #[test]
    fn refine_substring_extension_is_valid() {
        let c = cache("mar");
        assert!(c.is_valid_for("marilyn", SortOrder::MtimeDesc));
    }

    #[test]
    fn folder_prefix_after_substring_is_invalid() {
        // Bug repro: typing "folder" then "folder:mar" must NOT reuse cache,
        // because the new query's substring is "mar" (not "folder:mar") and
        // the file-type filter changed.
        let c = cache("folder");
        assert!(!c.is_valid_for("folder:mar", SortOrder::MtimeDesc));
    }

    #[test]
    fn refine_within_folder_prefix_is_valid() {
        let c = cache("folder:");
        assert!(c.is_valid_for("folder:mar", SortOrder::MtimeDesc));
    }

    #[test]
    fn extending_within_folder_prefix_is_valid() {
        let c = cache("folder:mar");
        assert!(c.is_valid_for("folder:marilyn", SortOrder::MtimeDesc));
    }

    #[test]
    fn shrinking_query_is_invalid() {
        let c = cache("folder:marilyn");
        assert!(!c.is_valid_for("folder:maril", SortOrder::MtimeDesc));
    }

    #[test]
    fn dropping_folder_prefix_is_invalid() {
        let c = cache("folder:mar");
        assert!(!c.is_valid_for("mar", SortOrder::MtimeDesc));
    }

    #[test]
    fn truncated_cache_is_invalid() {
        let mut c = cache("mar");
        c.was_truncated = true;
        assert!(!c.is_valid_for("marilyn", SortOrder::MtimeDesc));
    }

    #[test]
    fn different_sort_is_invalid() {
        let c = cache("mar");
        assert!(!c.is_valid_for("marilyn", SortOrder::NameAsc));
    }
}
