use rusqlite::{params, Connection};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::time::Instant;
use std::{fs, thread};
use tracing::{debug, info, warn};

use super::trigram::{AccessInfo, Bookmark, FileEntry, TrigramIndex};

const DB_PATH: &str = ".local/share/archnav/index.db";

/// Schema version for trigram cache. Bump to force rebuild.
const TRIGRAM_SCHEMA_VERSION: &str = "2";

pub enum DbOp {
    SaveFile(FileEntry, Vec<[u8; 3]>),
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

            CREATE TABLE IF NOT EXISTS file_trigrams (
                file_id INTEGER PRIMARY KEY,
                trigrams BLOB NOT NULL
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

            CREATE INDEX IF NOT EXISTS idx_files_path ON files(path);
            CREATE INDEX IF NOT EXISTS idx_files_mtime ON files(mtime);
            CREATE INDEX IF NOT EXISTS idx_file_access_last ON file_access(last_opened);
        "#,
        )?;

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
            conn.execute("DELETE FROM file_trigrams", [])?;
            conn.execute("DELETE FROM posting_lists", [])?;
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
        // Try fast path: load pre-built posting lists
        if let Some(count) = self.try_load_fast(index)? {
            // Load access data for frecency
            self.load_access_into_index(index);
            return Ok(count);
        }

        // Slow path: load files with per-file trigrams, rebuild posting lists
        let count = self.load_slow(index)?;

        // Load access data for frecency
        self.load_access_into_index(index);

        // Cache posting lists for next startup
        self.save_posting_lists(index)?;

        Ok(count)
    }

    /// Load file access data into the index for frecency scoring.
    fn load_access_into_index(&self, index: &mut TrigramIndex) {
        let data = self.load_access_data();
        for (file_id, (open_count, last_opened)) in data {
            index.access_data.insert(file_id, AccessInfo {
                open_count,
                last_opened,
            });
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
                path: row.get(1)?,
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
                let ids = unpack_u32_ids(&id_blob);
                index.trigrams.insert(trigram, ids);
                pl_count += 1;
            }
        }

        // Backfill trigrams for files added after this cache was built. Such files
        // (id >= cached_next_id) live in `files`/`file_trigrams` but are missing
        // from the cached posting lists, so without this they show up in all-file
        // listings (e.g. `*.mkv`) yet are invisible to substring/trigram search,
        // and a rescan can't repair it because TrigramIndex::add() skips known
        // paths. file_trigrams is kept in sync with files, so it is authoritative.
        let mut backfilled = 0usize;
        {
            let mut stmt = self
                .conn
                .prepare("SELECT file_id, trigrams FROM file_trigrams WHERE file_id >= ?1")?;
            let rows = stmt.query_map(params![cached_next_id], |row| {
                Ok((row.get::<_, u32>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?;
            for row in rows {
                let (file_id, blob) = row?;
                if !index.files.contains_key(&file_id) {
                    continue; // dangling trigram row for an already-removed file
                }
                for chunk in blob.chunks_exact(3) {
                    index
                        .trigrams
                        .entry([chunk[0], chunk[1], chunk[2]])
                        .or_default()
                        .insert(file_id);
                }
                backfilled += 1;
            }
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

    /// Slow path: load files with per-file trigrams, rebuild posting lists in memory.
    fn load_slow(&self, index: &mut TrigramIndex) -> rusqlite::Result<usize> {
        let start = Instant::now();

        let mut stmt = self.conn.prepare(
            "SELECT f.id, f.path, f.is_dir, f.mtime, f.size, ft.trigrams
             FROM files f
             LEFT JOIN file_trigrams ft ON f.id = ft.file_id",
        )?;

        let mut count = 0;
        let mut recomputed = 0;

        let rows = stmt.query_map([], |row| {
            Ok((
                FileEntry {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    is_dir: row.get::<_, i32>(2)? != 0,
                    mtime: row.get(3)?,
                    size: row.get(4)?,
                },
                row.get::<_, Option<Vec<u8>>>(5)?,
            ))
        })?;

        let mut needs_backfill: Vec<(u32, Vec<[u8; 3]>)> = Vec::new();

        for row in rows {
            let (entry, trigram_blob) = row?;
            let id = entry.id;

            let trigrams: Vec<[u8; 3]> = if let Some(blob) = trigram_blob {
                blob.chunks_exact(3)
                    .map(|chunk| [chunk[0], chunk[1], chunk[2]])
                    .collect()
            } else {
                recomputed += 1;
                let computed = TrigramIndex::compute_trigrams(&entry.path);
                needs_backfill.push((id, computed.clone()));
                computed
            };

            index.add_with_trigrams(entry, &trigrams);
            count += 1;
        }

        // Load bookmarks
        self.load_bookmarks(index)?;

        // Backfill missing per-file trigrams
        if !needs_backfill.is_empty() {
            info!(
                "Backfilling trigram cache for {} files (recomputed {} of {})",
                needs_backfill.len(),
                recomputed,
                count
            );
            let tx = self.conn.unchecked_transaction()?;
            {
                let mut insert = tx.prepare(
                    "INSERT OR REPLACE INTO file_trigrams (file_id, trigrams) VALUES (?1, ?2)",
                )?;
                for (file_id, trigrams) in &needs_backfill {
                    let blob = pack_trigrams(trigrams);
                    insert.execute(params![file_id, blob])?;
                }
            }
            tx.commit()?;
        }

        info!(
            "Slow loaded {} files ({} recomputed) in {:?}",
            count,
            recomputed,
            start.elapsed()
        );

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
            let mut stmt = tx.prepare(
                "INSERT INTO posting_lists (trigram, file_ids) VALUES (?1, ?2)",
            )?;

            for (trigram, ids) in &index.trigrams {
                let trigram_blob = trigram.as_slice();
                let id_blob = pack_u32_ids(ids);
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

    fn save_file(&self, entry: &FileEntry, trigrams: &[[u8; 3]]) {
        if let Err(e) = self.conn.execute(
            "INSERT OR REPLACE INTO files (id, path, is_dir, mtime, size) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![entry.id, entry.path, entry.is_dir as i32, entry.mtime, entry.size],
        ) {
            warn!("Failed to save file: {}", e);
            return;
        }

        // Persist per-file trigrams
        if !trigrams.is_empty() {
            let blob = pack_trigrams(trigrams);
            if let Err(e) = self.conn.execute(
                "INSERT OR REPLACE INTO file_trigrams (file_id, trigrams) VALUES (?1, ?2)",
                params![entry.id, blob],
            ) {
                warn!("Failed to save trigrams: {}", e);
            }
        }

    }

    fn remove_file(&self, path: &str) {
        let file_id: Option<u32> = self
            .conn
            .query_row(
                "SELECT id FROM files WHERE path = ?1",
                params![path],
                |row| row.get(0),
            )
            .ok();

        if let Err(e) = self
            .conn
            .execute("DELETE FROM files WHERE path = ?1", params![path])
        {
            warn!("Failed to remove file: {}", e);
        }

        if let Some(id) = file_id {
            let _ = self
                .conn
                .execute("DELETE FROM file_trigrams WHERE file_id = ?1", params![id]);
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
        if let Err(e) = self.conn.execute(
            "DELETE FROM file_trigrams WHERE file_id IN (SELECT id FROM files WHERE path LIKE ?1)",
            params![format!("{}%", path)],
        ) {
            warn!("Failed to clear trigrams: {}", e);
        }

        if let Err(e) = self.conn.execute(
            "DELETE FROM files WHERE path LIKE ?1",
            params![format!("{}%", path)],
        ) {
            warn!("Failed to clear files: {}", e);
        }
    }

    fn process_op(&self, op: DbOp) {
        match op {
            DbOp::SaveFile(entry, trigrams) => self.save_file(&entry, &trigrams),
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

        let mut stmt = match self.conn.prepare(
            "SELECT file_id, open_count, last_opened FROM file_access"
        ) {
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

/// Pack trigrams into a BLOB (3 bytes per trigram, concatenated).
fn pack_trigrams(trigrams: &[[u8; 3]]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(trigrams.len() * 3);
    for t in trigrams {
        blob.extend_from_slice(t);
    }
    blob
}

/// Pack a set of u32 IDs into a BLOB (4 bytes each, little-endian).
fn pack_u32_ids(ids: &HashSet<u32>) -> Vec<u8> {
    let mut blob = Vec::with_capacity(ids.len() * 4);
    for &id in ids {
        blob.extend_from_slice(&id.to_le_bytes());
    }
    blob
}

/// Unpack a BLOB into a HashSet of u32 IDs.
fn unpack_u32_ids(blob: &[u8]) -> HashSet<u32> {
    let mut ids = HashSet::with_capacity(blob.len() / 4);
    for chunk in blob.chunks_exact(4) {
        ids.insert(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    ids
}

/// Start a dedicated database writer thread. Returns the sender for queueing ops.
pub fn start_db_thread(db: Database) -> Sender<DbOp> {
    let (tx, rx) = channel::<DbOp>();

    thread::spawn(move || {
        for op in rx {
            db.process_op(op);
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

            let (id, tris) = idx.add("/data/alpha.txt".into(), false, 1, 1);
            db.save_file(idx.files.get(&id).unwrap(), &tris);
            db.save_posting_lists(&idx).unwrap();
        }

        // Simulate an incremental add (e.g. inotify/network scan): writes files +
        // file_trigrams for a higher id, but intentionally does NOT rewrite the
        // posting-list cache. This is exactly the real-world drift that hid the file.
        {
            let db = Database::open_at(path.clone()).unwrap();
            let mut idx = TrigramIndex::new();
            db.load_into_index(&mut idx).unwrap();
            let (id, tris) = idx.add("/data/A Man Called Otto.mkv".into(), false, 2, 2);
            db.save_file(idx.files.get(&id).unwrap(), &tris);
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
            assert_eq!(results.len(), 1, "`otto *.mkv` must match exactly the backfilled file");
        }

        cleanup(&path);
    }
}
