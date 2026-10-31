use roaring::RoaringBitmap;
use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::time::Instant;
use std::{fs, thread};
use tracing::{debug, info, warn};

use super::trigram::{AccessInfo, Bookmark, FileEntry, TrigramIndex};

const DB_PATH: &str = ".local/share/archnav/index.db";

/// Schema version for trigram cache. Bump to force rebuild.
const TRIGRAM_SCHEMA_VERSION: &str = "3";

pub enum DbOp {
    /// Insert a newly indexed file.
    SaveFile(FileEntry),
    /// Rewrite the metadata of a file already in the table (same id and path).
    UpdateFile(FileEntry),
    RemoveFile(String),
    SaveBookmark(Bookmark),
    ClearFilesUnder(String),
    RecordFileOpen(u32, i64), // file_id, timestamp
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open() -> rusqlite::Result<Self> {
        let home = dirs::home_dir().expect("No home directory");
        Self::open_at(home.join(DB_PATH))
    }

    /// Open (or create) the database at an explicit path. Split out from `open`
    /// so tests can run against a temporary database instead of the real one.
    fn open_at(db_path: PathBuf) -> rusqlite::Result<Self> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent).ok();
        }

        let conn = Connection::open(&db_path)?;

        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA cache_size = -64000;
            PRAGMA temp_store = MEMORY;
            -- Truncate the WAL to 64 MiB after a checkpoint. Without a limit it
            -- keeps the size of its largest transaction (a posting-list rebuild)
            -- for good.
            PRAGMA journal_size_limit = 67108864;

            CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY,
                path TEXT UNIQUE NOT NULL,
                is_dir INTEGER NOT NULL,
                mtime INTEGER NOT NULL,
                size INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS bookmarks (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT UNIQUE NOT NULL,
                is_network INTEGER NOT NULL,
                last_scan INTEGER
            );

            CREATE TABLE IF NOT EXISTS posting_lists (
                trigram BLOB NOT NULL PRIMARY KEY,
                file_ids BLOB NOT NULL
            );

            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS file_access (
                file_id INTEGER PRIMARY KEY,
                open_count INTEGER NOT NULL DEFAULT 0,
                last_opened INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_file_access_last ON file_access(last_opened);
        "#,
        )?;

        // Older databases also stored every file's trigrams, which are a pure
        // function of its path, plus two indexes on `files` that nothing
        // queries: one on path duplicating the index SQLite already keeps for
        // its UNIQUE constraint, one on mtime. They held a large share of the
        // file and added writes to every row change. Drop them once, then
        // VACUUM so the space goes back to the filesystem.
        let legacy: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE name IN ('file_trigrams', 'idx_files_path', 'idx_files_mtime')",
            [],
            |row| row.get(0),
        )?;
        if legacy > 0 {
            info!("Dropping stored trigrams and unused indexes, then compacting the database");
            let start = Instant::now();
            conn.execute_batch(
                "DROP TABLE IF EXISTS file_trigrams;
                 DROP INDEX IF EXISTS idx_files_path;
                 DROP INDEX IF EXISTS idx_files_mtime;
                 VACUUM;",
            )?;
            info!("Database compacted in {:?}", start.elapsed());
        }

        // Check schema version; if outdated, clear all caches
        let current_version: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'trigram_schema_version'",
                [],
                |row| row.get(0),
            )
            .ok();

        if current_version.as_deref() != Some(TRIGRAM_SCHEMA_VERSION) {
            info!("Trigram schema version changed, rebuilding cache");
            conn.execute("DELETE FROM posting_lists", [])?;
            // Without its count the emptied cache cannot pass for current
            conn.execute(
                "DELETE FROM meta WHERE key IN ('posting_lists_count', 'posting_lists_next_id')",
                [],
            )?;
            // Give the old cache's space back to the filesystem; the rebuilt
            // one is written into a compact file
            conn.execute_batch("VACUUM")?;
            conn.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('trigram_schema_version', ?1)",
                params![TRIGRAM_SCHEMA_VERSION],
            )?;
        }

        Ok(Self { conn })
    }

    /// Load index from database. Tries fast path (posting lists) first,
    /// falls back to rebuilding from per-file trigrams if cache is stale.
    pub fn load_into_index(&self, index: &mut TrigramIndex) -> rusqlite::Result<usize> {
        let count = {
            // Try fast path: load pre-built posting lists
            if let Some(count) = self.try_load_fast(index)? {
                // Load access data for frecency
                self.load_access_into_index(index);
                count
            } else {
                // Slow path: load files with per-file trigrams, rebuild posting lists
                let count = self.load_slow(index)?;

                // Load access data for frecency
                self.load_access_into_index(index);

                // Cache posting lists for next startup
                self.save_posting_lists(index)?;

                count
            }
        };

        // Continue id allocation from the persisted high-water mark so ids of
        // deleted files are never reused (see save_file for why that matters).
        let max_id: Option<u32> = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'max_file_id'",
                [],
                |row| row.get::<_, String>(0),
            )
            .ok()
            .and_then(|s| s.parse().ok());
        if let Some(max_id) = max_id {
            if max_id >= index.next_id {
                index.next_id = max_id + 1;
            }
        }

        Ok(count)
    }

    /// Load file access data into the index for frecency scoring.
    fn load_access_into_index(&self, index: &mut TrigramIndex) {
        let data = self.load_access_data();
        for (file_id, (open_count, last_opened)) in data {
            index.access_data.insert(
                file_id,
                AccessInfo {
                    open_count,
                    last_opened,
                },
            );
        }
        if !index.access_data.is_empty() {
            info!("Loaded {} file access records", index.access_data.len());
        }
    }

    /// Fast path: load files + pre-built posting lists directly.
    /// Returns None if the cache is stale or missing.
    fn try_load_fast(&self, index: &mut TrigramIndex) -> rusqlite::Result<Option<usize>> {
        // Check if posting lists exist and are current
        let cached_count: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'posting_lists_count'",
                [],
                |row| row.get(0),
            )
            .ok();

        let actual_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;

        let cached_count_num = cached_count
            .as_deref()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(-1);

        // Allow small tolerance in file count (1%) to avoid rebuilding for minor changes
        // The integrity checker will clean up stale entries in the background
        let count_diff = (cached_count_num - actual_count).unsigned_abs() as i64;
        let tolerance = (actual_count / 100).max(100); // 1% or at least 100 files

        if actual_count == 0 {
            return Ok(None);
        }

        if cached_count_num < 0 || count_diff > tolerance {
            info!(
                "Posting list cache stale (cached={}, actual={}, diff={}), rebuilding",
                cached_count_num, actual_count, count_diff
            );
            return Ok(None);
        }

        // The posting-list cache only covers files with id < this high-water mark
        // (it is only rewritten on a full rebuild). We need it to know which files
        // were added since the cache was built so we can backfill their trigrams
        // below. If it's absent (a cache from before this was tracked), fall back
        // to a full rebuild, which repopulates it.
        let cached_next_id: u32 = match self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'posting_lists_next_id'",
                [],
                |row| row.get::<_, String>(0),
            )
            .ok()
            .and_then(|s| s.parse().ok())
        {
            Some(n) => n,
            None => {
                info!("Posting list cache missing next_id high-water mark, rebuilding");
                return Ok(None);
            }
        };

        if count_diff > 0 {
            debug!(
                "Posting list cache slightly stale (diff={}), using anyway",
                count_diff
            );
        }

        let start = Instant::now();

        // Load files (metadata only, no trigram JOIN needed)
        let mut stmt = self
            .conn
            .prepare("SELECT id, path, is_dir, mtime, size FROM files")?;

        let mut file_count = 0;
        let rows = stmt.query_map([], |row| {
            Ok(FileEntry {
                id: row.get(0)?,
                path: row.get::<_, String>(1)?.into(),
                is_dir: row.get::<_, i32>(2)? != 0,
                mtime: row.get(3)?,
                size: row.get(4)?,
            })
        })?;

        for row in rows {
            let entry = row?;
            let id = entry.id;
            if id >= index.next_id {
                index.next_id = id + 1;
            }
            index.path_to_id.insert(entry.path.clone(), id);
            index.files.insert(id, entry);
            file_count += 1;
        }

        let files_elapsed = start.elapsed();

        // Load posting lists directly into trigrams map
        let mut stmt = self
            .conn
            .prepare("SELECT trigram, file_ids FROM posting_lists")?;

        let mut pl_count = 0;
        let rows = stmt.query_map([], |row| {
            let trigram_blob: Vec<u8> = row.get(0)?;
            let id_blob: Vec<u8> = row.get(1)?;
            Ok((trigram_blob, id_blob))
        })?;

        for row in rows {
            let (trigram_blob, id_blob) = row?;
            if trigram_blob.len() == 3 {
                let trigram = [trigram_blob[0], trigram_blob[1], trigram_blob[2]];
                match RoaringBitmap::deserialize_from(&id_blob[..]) {
                    Ok(ids) => {
                        index.trigrams.insert(trigram, ids);
                        pl_count += 1;
                    }
                    Err(e) => {
                        warn!("Corrupt posting list cache ({}), rebuilding", e);
                        index.files.clear();
                        index.path_to_id.clear();
                        index.trigrams.clear();
                        return Ok(None);
                    }
                }
            }
        }

        // Backfill trigrams for files added after this cache was built. Such files
        // (id >= cached_next_id) live in `files` but are missing from the cached
        // posting lists, so without this they show up in all-file listings (e.g.
        // `*.mkv`) yet are invisible to substring/trigram search, and a rescan
        // can't repair it because TrigramIndex::add() skips known paths.
        // Trigrams are a pure function of the path, so they are recomputed.
        let mut backfilled = 0usize;
        for entry in index.files.values().filter(|e| e.id >= cached_next_id) {
            for trigram in TrigramIndex::compute_trigrams(&entry.path) {
                index.trigrams.entry(trigram).or_default().insert(entry.id);
            }
            backfilled += 1;
        }
        if backfilled > 0 {
            info!(
                "Backfilled trigrams for {} file(s) added since cache (id >= {})",
                backfilled, cached_next_id
            );
        }

        // Load bookmarks
        self.load_bookmarks(index)?;

        info!(
            "Fast loaded {} files + {} posting lists in {:?} (files: {:?})",
            file_count,
            pl_count,
            start.elapsed(),
            files_elapsed
        );

        Ok(Some(file_count))
    }

    /// Slow path: load files and rebuild posting lists in memory from their paths.
    fn load_slow(&self, index: &mut TrigramIndex) -> rusqlite::Result<usize> {
        let start = Instant::now();

        let mut stmt = self
            .conn
            .prepare("SELECT id, path, is_dir, mtime, size FROM files")?;

        let mut count = 0;

        let rows = stmt.query_map([], |row| {
            Ok(FileEntry {
                id: row.get(0)?,
                path: row.get::<_, String>(1)?.into(),
                is_dir: row.get::<_, i32>(2)? != 0,
                mtime: row.get(3)?,
                size: row.get(4)?,
            })
        })?;

        for row in rows {
            let entry = row?;
            let trigrams = TrigramIndex::compute_trigrams(&entry.path);
            index.add_with_trigrams(entry, &trigrams);
            count += 1;
        }
        // Path-component trigrams cover long runs of consecutive ids (a
        // folder's files are scanned together), which run containers hold in
        // a few bytes each
        for ids in index.trigrams.values_mut() {
            ids.optimize();
        }

        // Load bookmarks
        self.load_bookmarks(index)?;

        info!("Slow loaded {} files in {:?}", count, start.elapsed());

        Ok(count)
    }

    /// Load bookmarks into the index.
    fn load_bookmarks(&self, index: &mut TrigramIndex) -> rusqlite::Result<()> {
        let mut stmt = self
            .conn
            .prepare("SELECT name, path, is_network FROM bookmarks")?;
        let bookmarks = stmt.query_map([], |row| {
            Ok(Bookmark {
                name: row.get(0)?,
                path: row.get(1)?,
                is_network: row.get::<_, i32>(2)? != 0,
            })
        })?;

        for bookmark in bookmarks {
            index.bookmarks.push(bookmark?);
        }
        Ok(())
    }

    /// Save the in-memory posting lists to the database for fast loading next time.
    fn save_posting_lists(&self, index: &TrigramIndex) -> rusqlite::Result<()> {
        let start = Instant::now();

        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM posting_lists", [])?;

        {
            let mut stmt =
                tx.prepare("INSERT INTO posting_lists (trigram, file_ids) VALUES (?1, ?2)")?;

            for (trigram, ids) in &index.trigrams {
                let trigram_blob = trigram.as_slice();
                let mut id_blob = Vec::with_capacity(ids.serialized_size());
                ids.serialize_into(&mut id_blob)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                stmt.execute(params![trigram_blob, id_blob])?;
            }
        }

        // Store file count for cache validation
        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('posting_lists_count', ?1)",
            params![index.file_count().to_string()],
        )?;

        // Store the next_id high-water mark. Every file currently indexed has
        // id < next_id and is represented in these posting lists, so on the next
        // fast load any file with id >= this value was added afterward and must
        // have its trigrams backfilled (see try_load_fast).
        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('posting_lists_next_id', ?1)",
            params![index.next_id.to_string()],
        )?;

        tx.commit()?;

        info!(
            "Saved {} posting lists in {:?}",
            index.trigrams.len(),
            start.elapsed()
        );

        Ok(())
    }

    fn save_file(&self, entry: &FileEntry) {
        if let Err(e) = self.conn.execute(
            "INSERT OR REPLACE INTO files (id, path, is_dir, mtime, size) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![entry.id, &*entry.path, entry.is_dir as i32, entry.mtime, entry.size],
        ) {
            warn!("Failed to save file: {}", e);
            return;
        }

        // Track the high-water mark of allocated ids. next_id is otherwise
        // recomputed as max(loaded id)+1, so deleting the highest-id file lets
        // a later file reuse that id; the reused id sits below the posting-list
        // cache's next_id mark, gets skipped by the fast-load backfill, and the
        // file stays invisible to trigram search until a full cache rebuild.
        if let Err(e) = self.conn.execute(
            "INSERT INTO meta (key, value) VALUES ('max_file_id', ?1)
             ON CONFLICT(key) DO UPDATE SET
                value = MAX(CAST(value AS INTEGER), CAST(excluded.value AS INTEGER))",
            params![entry.id.to_string()],
        ) {
            warn!("Failed to update max_file_id: {}", e);
        }
    }

    /// Rewrite only the metadata columns, so an mtime or size change on an
    /// existing file does not delete and reinsert its row. A row missing from
    /// the table (an earlier insert failed) is inserted instead.
    fn update_file(&self, entry: &FileEntry) {
        match self.conn.execute(
            "UPDATE files SET is_dir = ?2, mtime = ?3, size = ?4 WHERE id = ?1",
            params![entry.id, entry.is_dir as i32, entry.mtime, entry.size],
        ) {
            Ok(0) => self.save_file(entry),
            Ok(_) => {}
            Err(e) => warn!("Failed to update file: {}", e),
        }
    }

    fn remove_file(&self, path: &str) {
        if let Err(e) = self
            .conn
            .execute("DELETE FROM files WHERE path = ?1", params![path])
        {
            warn!("Failed to remove file: {}", e);
        }
    }

    pub fn save_bookmark(&self, bookmark: &Bookmark) {
        if let Err(e) = self.conn.execute(
            "INSERT OR REPLACE INTO bookmarks (name, path, is_network) VALUES (?1, ?2, ?3)",
            params![bookmark.name, bookmark.path, bookmark.is_network as i32],
        ) {
            warn!("Failed to save bookmark: {}", e);
        }
    }

    fn clear_files_under(&self, path: &str) {
        // Match the path itself or anything below it with a '/' boundary, so
        // clearing "/mnt/data" cannot take "/mnt/database" with it. LIKE
        // wildcards are escaped so '_' and '%' in real paths stay literal
        // (an unescaped '_' matches any single character).
        let root = path.trim_end_matches('/');
        let prefix = format!("{}/%", escape_like(root));

        if let Err(e) = self.conn.execute(
            "DELETE FROM files WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\'",
            params![root, prefix],
        ) {
            warn!("Failed to clear files: {}", e);
        }
    }

    fn process_op(&self, op: DbOp) {
        match op {
            DbOp::SaveFile(entry) => self.save_file(&entry),
            DbOp::UpdateFile(entry) => self.update_file(&entry),
            DbOp::RemoveFile(path) => self.remove_file(&path),
            DbOp::SaveBookmark(bookmark) => self.save_bookmark(&bookmark),
            DbOp::ClearFilesUnder(path) => self.clear_files_under(&path),
            DbOp::RecordFileOpen(file_id, timestamp) => self.record_file_open(file_id, timestamp),
        }
    }

    /// Record a file open event for frecency tracking.
    fn record_file_open(&self, file_id: u32, timestamp: i64) {
        if let Err(e) = self.conn.execute(
            "INSERT INTO file_access (file_id, open_count, last_opened)
             VALUES (?1, 1, ?2)
             ON CONFLICT(file_id) DO UPDATE SET
                open_count = open_count + 1,
                last_opened = ?2",
            params![file_id, timestamp],
        ) {
            warn!("Failed to record file open: {}", e);
        }
    }

    /// Load access data (open counts and last opened times) into a HashMap.
    pub fn load_access_data(&self) -> std::collections::HashMap<u32, (u32, i64)> {
        let mut data = std::collections::HashMap::new();

        let mut stmt = match self
            .conn
            .prepare("SELECT file_id, open_count, last_opened FROM file_access")
        {
            Ok(s) => s,
            Err(_) => return data,
        };

        let rows = match stmt.query_map([], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, i64>(2)?,
            ))
        }) {
            Ok(r) => r,
            Err(_) => return data,
        };

        for row in rows.flatten() {
            data.insert(row.0, (row.1, row.2));
        }

        data
    }
}

/// Escape SQLite LIKE wildcards so a literal path can be used as a prefix
/// pattern (paired with ESCAPE '\' in the query).
fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Start a dedicated database writer thread. Returns the sender for queueing ops.
pub fn start_db_thread(db: Database) -> Sender<DbOp> {
    let (tx, rx) = channel::<DbOp>();

    thread::spawn(move || {
        // Drain bursts into a single transaction: the initial scan queues
        // hundreds of thousands of ops, and one autocommit per op makes that
        // far slower than it needs to be.
        const MAX_BATCH: usize = 1000;
        while let Ok(first) = rx.recv() {
            let mut ops = vec![first];
            while ops.len() < MAX_BATCH {
                match rx.try_recv() {
                    Ok(op) => ops.push(op),
                    Err(_) => break,
                }
            }

            if ops.len() == 1 {
                db.process_op(ops.pop().unwrap());
                continue;
            }

            let count = ops.len();
            match db.conn.unchecked_transaction() {
                Ok(batch_tx) => {
                    for op in ops {
                        db.process_op(op);
                    }
                    if let Err(e) = batch_tx.commit() {
                        warn!("DB batch commit failed ({} ops): {}", count, e);
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to open DB transaction, falling back to autocommit: {}",
                        e
                    );
                    for op in ops {
                        db.process_op(op);
                    }
                }
            }
        }
        info!("Database thread exiting");
    });

    tx
}

#[cfg(test)]
mod fast_load_tests {
    use super::*;
    use crate::search::query::{ParsedQuery, SortOrder};
    use crate::search::trigram::{Bookmark, TrigramIndex};
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_db_path() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("archnav_fastload_{}_{}.db", std::process::id(), n))
    }

    fn cleanup(path: &PathBuf) {
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(format!("{}-wal", path.display()));
        let _ = fs::remove_file(format!("{}-shm", path.display()));
    }

    /// Regression test: a file added after the posting-list cache was built (so it
    /// is in `files`/`file_trigrams` but not in the cached posting lists) must still
    /// be found by substring search after a fast load.
    #[test]
    fn fast_load_backfills_files_added_after_cache() {
        let path = temp_db_path();
        cleanup(&path);

        // Build an index with one file; persist files + per-file trigrams + cache.
        {
            let db = Database::open_at(path.clone()).unwrap();
            let mut idx = TrigramIndex::new();
            idx.bookmarks.push(Bookmark {
                name: "t".into(),
                path: "/data".into(),
                is_network: false,
            });
            db.save_bookmark(&idx.bookmarks[0]);

            let (id, _) = idx.add("/data/alpha.txt".into(), false, 1, 1);
            db.save_file(idx.files.get(&id).unwrap());
            db.save_posting_lists(&idx).unwrap();
        }

        // Simulate an incremental add (e.g. inotify/network scan): writes files +
        // file_trigrams for a higher id, but intentionally does NOT rewrite the
        // posting-list cache. This is exactly the real-world drift that hid the file.
        {
            let db = Database::open_at(path.clone()).unwrap();
            let mut idx = TrigramIndex::new();
            db.load_into_index(&mut idx).unwrap();
            let (id, _) = idx.add("/data/A Man Called Otto.mkv".into(), false, 2, 2);
            db.save_file(idx.files.get(&id).unwrap());
        }

        // A fresh fast load must still surface the file added after the cache.
        {
            let db = Database::open_at(path.clone()).unwrap();
            let mut idx = TrigramIndex::new();
            db.load_into_index(&mut idx).unwrap();

            let q = ParsedQuery::parse("otto", SortOrder::MtimeDesc);
            let results = idx.search_all(&q, &[]);
            assert!(
                results.iter().any(|r| r.path.ends_with("Otto.mkv")),
                "substring search must find a file added after the posting-list cache was built"
            );

            // And the extension-filtered form the user actually typed.
            let q = ParsedQuery::parse("otto *.mkv", SortOrder::MtimeDesc);
            let results = idx.search_all(&q, &[]);
            assert_eq!(
                results.len(),
                1,
                "`otto *.mkv` must match exactly the backfilled file"
            );
        }

        cleanup(&path);
    }

    /// A schema bump drops the cached posting lists; the next load must
    /// rebuild them from the paths instead of trusting the stale cache count.
    #[test]
    fn schema_bump_rebuilds_the_posting_lists() {
        let path = temp_db_path();
        cleanup(&path);

        {
            let db = Database::open_at(path.clone()).unwrap();
            let mut idx = TrigramIndex::new();
            idx.bookmarks.push(Bookmark {
                name: "t".into(),
                path: "/data".into(),
                is_network: false,
            });
            db.save_bookmark(&idx.bookmarks[0]);
            let (id, _) = idx.add("/data/alpha.txt".into(), false, 1, 1);
            db.save_file(idx.files.get(&id).unwrap());
            db.save_posting_lists(&idx).unwrap();
            db.conn
                .execute(
                    "UPDATE meta SET value = 'old' WHERE key = 'trigram_schema_version'",
                    [],
                )
                .unwrap();
        }

        {
            let db = Database::open_at(path.clone()).unwrap();
            let mut idx = TrigramIndex::new();
            db.load_into_index(&mut idx).unwrap();
            let q = ParsedQuery::parse("alpha", SortOrder::MtimeDesc);
            assert_eq!(idx.search_all(&q, &[]).len(), 1);
        }

        cleanup(&path);
    }

    /// Removing a bookmark must not delete rows for sibling paths sharing a
    /// string prefix, and LIKE wildcards in real paths must stay literal.
    #[test]
    fn clear_files_under_respects_boundary_and_like_wildcards() {
        let path = temp_db_path();
        cleanup(&path);

        {
            let db = Database::open_at(path.clone()).unwrap();
            let mut idx = TrigramIndex::new();
            for p in [
                "/mnt/data/a.txt",
                "/mnt/database/b.txt",
                "/mnt/my_dir/c.txt",
                "/mnt/myxdir/d.txt",
            ] {
                let (id, _) = idx.add(p.to_string(), false, 1, 1);
                db.save_file(idx.files.get(&id).unwrap());
            }

            db.clear_files_under("/mnt/data");
            db.clear_files_under("/mnt/my_dir");

            let mut stmt = db
                .conn
                .prepare("SELECT path FROM files ORDER BY path")
                .unwrap();
            let remaining: Vec<String> = stmt
                .query_map([], |r| r.get(0))
                .unwrap()
                .map(|r| r.unwrap())
                .collect();
            assert_eq!(
                remaining,
                vec![
                    "/mnt/database/b.txt".to_string(),
                    "/mnt/myxdir/d.txt".to_string()
                ]
            );
        }

        cleanup(&path);
    }

    fn file_row(db: &Database, id: u32) -> Option<(String, i64, i64)> {
        db.conn
            .query_row(
                "SELECT path, mtime, size FROM files WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok()
    }

    #[test]
    fn update_file_rewrites_metadata_and_inserts_a_missing_row() {
        let path = temp_db_path();
        cleanup(&path);

        {
            let db = Database::open_at(path.clone()).unwrap();
            let mut idx = TrigramIndex::new();
            let (id, _) = idx.add("/d/notes.txt".to_string(), false, 1, 10);
            db.save_file(idx.files.get(&id).unwrap());

            idx.upsert("/d/notes.txt".to_string(), false, 2, 20);
            db.update_file(idx.files.get(&id).unwrap());
            assert_eq!(file_row(&db, id), Some(("/d/notes.txt".into(), 2, 20)));

            // A row the table never got is written by the update rather than lost.
            let (lost, _) = idx.add("/d/lost.txt".to_string(), false, 3, 30);
            db.update_file(idx.files.get(&lost).unwrap());
            assert_eq!(file_row(&db, lost), Some(("/d/lost.txt".into(), 3, 30)));
        }

        cleanup(&path);
    }

    /// Opening a database in the old layout drops the stored per-file
    /// trigrams and the two unused `files` indexes, and keeps every file.
    #[test]
    fn legacy_trigram_table_and_indexes_are_dropped_on_open() {
        let path = temp_db_path();
        cleanup(&path);

        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE files (id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL,
                     is_dir INTEGER NOT NULL, mtime INTEGER NOT NULL, size INTEGER NOT NULL);
                 CREATE TABLE file_trigrams (file_id INTEGER PRIMARY KEY, trigrams BLOB NOT NULL);
                 CREATE INDEX idx_files_path ON files(path);
                 CREATE INDEX idx_files_mtime ON files(mtime);
                 INSERT INTO files VALUES (1, '/d/kept.txt', 0, 5, 50);
                 INSERT INTO file_trigrams VALUES (1, x'6b6570');",
            )
            .unwrap();
        }

        {
            let db = Database::open_at(path.clone()).unwrap();
            let legacy: i64 = db
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE name IN ('file_trigrams', 'idx_files_path', 'idx_files_mtime')",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(legacy, 0);
            assert_eq!(file_row(&db, 1), Some(("/d/kept.txt".into(), 5, 50)));

            db.save_bookmark(&Bookmark {
                name: "d".into(),
                path: "/d".into(),
                is_network: false,
            });
            let mut idx = TrigramIndex::new();
            db.load_into_index(&mut idx).unwrap();
            let q = ParsedQuery::parse("kept", SortOrder::MtimeDesc);
            assert_eq!(
                idx.search_all(&q, &[]).len(),
                1,
                "trigrams come from the path"
            );
        }

        cleanup(&path);
    }

    /// Deleting the highest-id file must not let a later file reuse its id
    /// across a restart; a reused id is skipped by the posting-list backfill
    /// and stays invisible to trigram search.
    #[test]
    fn deleted_high_id_is_not_reused_after_reload() {
        let path = temp_db_path();
        cleanup(&path);

        let removed_id;
        {
            let db = Database::open_at(path.clone()).unwrap();
            let mut idx = TrigramIndex::new();
            let (id_a, _) = idx.add("/d/aaaa.txt".to_string(), false, 1, 1);
            db.save_file(idx.files.get(&id_a).unwrap());
            let (id_b, _) = idx.add("/d/bbbb.txt".to_string(), false, 2, 2);
            db.save_file(idx.files.get(&id_b).unwrap());
            removed_id = id_b;
            db.remove_file("/d/bbbb.txt");
        }

        {
            let db = Database::open_at(path.clone()).unwrap();
            let mut idx = TrigramIndex::new();
            db.load_into_index(&mut idx).unwrap();
            let (new_id, _) = idx.add("/d/cccc.txt".to_string(), false, 3, 3);
            assert!(
                new_id > removed_id,
                "deleted ids must not be reused (got {} after removing {})",
                new_id,
                removed_id
            );
        }

        cleanup(&path);
    }
}
