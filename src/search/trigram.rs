use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;

use super::query::{FileTypeMode, ParsedQuery, QueryMode, SortOrder};
use super::scanner::path_under_root;

/// Access information for frecency scoring.
#[derive(Debug, Clone, Default)]
pub struct AccessInfo {
    pub open_count: u32,
    pub last_opened: i64,
}

impl AccessInfo {
    /// Calculate frecency score. Higher = more relevant.
    /// Combines frequency (open count) with recency (time since last access).
    pub fn frecency_score(&self) -> f64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.frecency_score_at(now)
    }

    /// `frecency_score` against a fixed `now`, so sorting many results reads
    /// the clock once.
    pub fn frecency_score_at(&self, now: i64) -> f64 {
        if self.open_count == 0 {
            return 0.0;
        }

        let hours_ago = (now - self.last_opened) as f64 / 3600.0;
        let recency_weight = if hours_ago < 1.0 {
            4.0 // Last hour: 4x
        } else if hours_ago < 24.0 {
            2.0 // Last day: 2x
        } else if hours_ago < 168.0 {
            1.5 // Last week: 1.5x
        } else if hours_ago < 720.0 {
            1.0 // Last month: 1x
        } else {
            0.5 // Older: 0.5x
        };

        // Log scale for frequency to prevent runaway counts
        (self.open_count as f64).ln_1p() * recency_weight
    }
}

pub const MAX_RESULTS: usize = 2000;

const BINARY_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "ico", "webp", "svg", "tiff", "raw", "mp3", "mp4", "wav",
    "avi", "mkv", "mov", "flac", "ogg", "m4a", "aac", "pdf", "doc", "docx", "xls", "xlsx", "ppt",
    "pptx", "zip", "tar", "gz", "bz2", "xz", "7z", "rar", "zst", "exe", "dll", "so", "dylib", "a",
    "o", "obj", "bin", "dat", "db", "sqlite", "sqlite3", "ttf", "otf", "woff", "woff2", "eot",
    "class", "jar", "war", "pyc", "pyo", "whl", "min.js", "min.css",
];

pub const EXCLUDE_PATTERNS: &[&str] = &[
    ".git",
    "node_modules",
    "__pycache__",
    ".cache",
    ".npm",
    ".cargo",
    "target",
    "build",
    "dist",
    ".next",
    ".nuxt",
    ".Trash",
    "Trash",
    ".steam",
    "dosdevices", // Wine/Proton paths with z: symlink to root
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub id: u32,
    pub path: String,
    pub is_dir: bool,
    pub mtime: i64,
    pub size: u64,
}

/// Outcome of [`TrigramIndex::upsert`].
#[derive(Debug, PartialEq)]
pub enum Upsert {
    /// Already indexed with identical metadata; nothing to persist.
    Unchanged(u32),
    /// Already indexed; its metadata was updated in place.
    Updated(u32),
    /// Newly indexed under these trigrams.
    Added(u32, Vec<[u8; 3]>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub name: String,
    pub path: String,
    pub is_network: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub path: String,
    pub is_dir: bool,
    pub mtime: i64,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchAllResult {
    pub path: String,
    pub is_dir: bool,
    pub mtime: i64,
    pub size: u64,
    pub bookmark: String,
}

/// Files a query could match: every indexed file, or the ids its trigrams allow.
enum Candidates {
    All,
    Ids(Vec<u32>),
}

pub struct TrigramIndex {
    pub trigrams: HashMap<[u8; 3], HashSet<u32>>,
    pub files: HashMap<u32, FileEntry>,
    pub path_to_id: HashMap<String, u32>,
    pub next_id: u32,
    pub bookmarks: Vec<Bookmark>,
    /// Access data for frecency scoring (file_id -> AccessInfo)
    pub access_data: HashMap<u32, AccessInfo>,
    /// Result cap applied after sorting. Set from config max_results at
    /// startup; hard-capped at MAX_RESULTS.
    pub max_results: usize,
}

impl Default for TrigramIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl TrigramIndex {
    pub fn new() -> Self {
        Self {
            trigrams: HashMap::new(),
            files: HashMap::new(),
            path_to_id: HashMap::new(),
            next_id: 1,
            bookmarks: Vec::new(),
            access_data: HashMap::new(),
            max_results: MAX_RESULTS,
        }
    }

    /// Extract trigrams from a string (lowercase for case-insensitive search)
    pub fn extract_trigrams(s: &str) -> Vec<[u8; 3]> {
        let lower = s.to_lowercase();
        let bytes = lower.as_bytes();
        if bytes.len() < 3 {
            return Vec::new();
        }
        bytes.windows(3).map(|w| [w[0], w[1], w[2]]).collect()
    }

    /// Compute all trigrams for a file path (filename + path components, deduplicated).
    pub fn compute_trigrams(path: &str) -> Vec<[u8; 3]> {
        let mut all = std::collections::HashSet::new();

        let filename = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path);

        for t in Self::extract_trigrams(filename) {
            all.insert(t);
        }
        for component in path.split('/').filter(|s| !s.is_empty()) {
            for t in Self::extract_trigrams(component) {
                all.insert(t);
            }
        }

        all.into_iter().collect()
    }

    /// Add a file to the index. Returns (file_id, trigrams) for persistence.
    pub fn add(
        &mut self,
        path: String,
        is_dir: bool,
        mtime: i64,
        size: u64,
    ) -> (u32, Vec<[u8; 3]>) {
        match self.upsert(path, is_dir, mtime, size) {
            Upsert::Unchanged(id) | Upsert::Updated(id) => (id, Vec::new()),
            Upsert::Added(id, trigrams) => (id, trigrams),
        }
    }

    /// True if `path` is indexed with exactly this metadata, so a rescan
    /// has nothing to record for it.
    pub fn is_current(&self, path: &str, is_dir: bool, mtime: i64, size: u64) -> bool {
        self.path_to_id
            .get(path)
            .and_then(|id| self.files.get(id))
            .is_some_and(|e| e.is_dir == is_dir && e.mtime == mtime && e.size == size)
    }

    /// Index `path`, or refresh its metadata if already indexed, and report
    /// which of the two happened so the caller persists only what changed.
    pub fn upsert(&mut self, path: String, is_dir: bool, mtime: i64, size: u64) -> Upsert {
        if let Some(&existing_id) = self.path_to_id.get(&path) {
            if let Some(entry) = self.files.get_mut(&existing_id) {
                if entry.is_dir == is_dir && entry.mtime == mtime && entry.size == size {
                    return Upsert::Unchanged(existing_id);
                }
                entry.is_dir = is_dir;
                entry.mtime = mtime;
                entry.size = size;
            }
            return Upsert::Updated(existing_id);
        }

        let id = self.next_id;
        self.next_id += 1;

        let file_trigrams = Self::compute_trigrams(&path);

        for &trigram in &file_trigrams {
            self.trigrams.entry(trigram).or_default().insert(id);
        }

        let entry = FileEntry {
            id,
            path: path.clone(),
            is_dir,
            mtime,
            size,
        };
        self.files.insert(id, entry);
        self.path_to_id.insert(path, id);

        Upsert::Added(id, file_trigrams)
    }

    /// Add a file with pre-computed trigrams (used when loading from DB).
    pub fn add_with_trigrams(&mut self, entry: FileEntry, trigrams: &[[u8; 3]]) {
        let id = entry.id;
        if id >= self.next_id {
            self.next_id = id + 1;
        }

        for &trigram in trigrams {
            self.trigrams.entry(trigram).or_default().insert(id);
        }

        self.path_to_id.insert(entry.path.clone(), id);
        self.files.insert(id, entry);
    }

    /// Remove a file from the index
    pub fn remove(&mut self, path: &str) {
        if let Some(id) = self.path_to_id.remove(path) {
            if let Some(entry) = self.files.remove(&id) {
                let filename = Path::new(&entry.path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&entry.path);

                for trigram in Self::extract_trigrams(filename) {
                    if let Some(set) = self.trigrams.get_mut(&trigram) {
                        set.remove(&id);
                    }
                }
                for component in entry.path.split('/').filter(|s| !s.is_empty()) {
                    for trigram in Self::extract_trigrams(component) {
                        if let Some(set) = self.trigrams.get_mut(&trigram) {
                            set.remove(&id);
                        }
                    }
                }
            }
        }
    }

    /// Get candidate file IDs from trigram intersection
    fn get_candidates(&self, query: &ParsedQuery) -> Candidates {
        match &query.mode {
            QueryMode::Substring(_) => {
                // Every word must appear, so all their trigrams intersect
                let mut trigrams: Vec<[u8; 3]> = query
                    .search_words()
                    .flat_map(Self::extract_trigrams)
                    .collect();
                trigrams.sort_unstable();
                trigrams.dedup();
                self.intersect_trigrams(&trigrams)
            }
            QueryMode::Regex(re) => {
                // Try to extract literal fragments from regex for trigram narrowing
                let pattern = re.as_str();
                let literals = extract_regex_literals(pattern);
                if literals.is_empty() {
                    // No literals - scan all files
                    Candidates::All
                } else {
                    // Use longest literal for trigram narrowing
                    let best = literals.iter().max_by_key(|s| s.len()).unwrap();
                    let trigrams = Self::extract_trigrams(&best.to_lowercase());
                    self.intersect_trigrams(&trigrams)
                }
            }
            QueryMode::Glob(_) => {
                // Glob patterns are hard to extract trigrams from; scan all
                Candidates::All
            }
            QueryMode::Fuzzy {
                query,
                max_distance,
            } => {
                // For fuzzy matching, trigram-based narrowing doesn't work well because
                // typos change the trigrams. For short queries or high edit distance,
                // scan all files. For longer queries, use lenient trigram matching.
                let trigrams = Self::extract_trigrams(&query.to_lowercase());

                // If query is short or has few trigrams, scan all files
                // (Levenshtein will filter properly)
                if query.len() < 5 || trigrams.len() < 2 || *max_distance >= 2 {
                    return Candidates::All;
                }

                // For longer queries with low edit distance, use partial trigram matching
                // Require at least 1 matching trigram (very lenient)
                let mut candidates: HashSet<u32> = HashSet::new();

                for trigram in &trigrams {
                    if let Some(ids) = self.trigrams.get(trigram) {
                        candidates.extend(ids.iter().copied());
                    }
                }

                if candidates.is_empty() {
                    // Fallback to scanning all if no trigram matches
                    Candidates::All
                } else {
                    Candidates::Ids(candidates.into_iter().collect())
                }
            }
        }
    }

    /// Intersect posting lists for given trigrams. Walks the shortest list and
    /// keeps the ids every other list contains, so a common trigram costs a
    /// lookup per candidate rather than a copy of its whole list.
    fn intersect_trigrams(&self, trigrams: &[[u8; 3]]) -> Candidates {
        if trigrams.is_empty() {
            return Candidates::All;
        }
        let mut lists: Vec<&HashSet<u32>> = Vec::with_capacity(trigrams.len());
        for trigram in trigrams {
            match self.trigrams.get(trigram) {
                Some(list) => lists.push(list),
                None => return Candidates::Ids(Vec::new()),
            }
        }
        lists.sort_by_key(|list| list.len());
        let (shortest, rest) = lists.split_first().unwrap();
        Candidates::Ids(
            shortest
                .iter()
                .copied()
                .filter(|id| rest.iter().all(|list| list.contains(id)))
                .collect(),
        )
    }

    /// Check if a path matches the query: its mode, then its `!words`
    fn matches_query(&self, path: &str, query: &ParsedQuery) -> bool {
        self.mode_matches(path, query) && !query.is_excluded(path)
    }

    fn mode_matches(&self, path: &str, query: &ParsedQuery) -> bool {
        // Check path segments first (for path-aware search like "src/config")
        if !query.matches_path_segments(path) {
            return false;
        }

        match &query.mode {
            QueryMode::Substring(_) => query.substring_matches(path),
            QueryMode::Regex(re) => re.is_match(path),
            QueryMode::Glob(matcher) => {
                // Match against filename
                let filename = Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path);
                matcher.is_match(filename) || matcher.is_match(path)
            }
            QueryMode::Fuzzy {
                query: q,
                max_distance,
            } => {
                if q.is_empty() {
                    return true;
                }
                let filename = Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path);
                let filename_lower = filename.to_lowercase();
                let q_lower = q.to_lowercase();

                // First try exact substring match
                if filename_lower.contains(&q_lower) {
                    return true;
                }

                // For fuzzy matching, try to find a substring of similar length that matches
                // within edit distance. Use a sliding window approach with char indices.
                let name_stem = Path::new(filename)
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or(filename)
                    .to_lowercase();

                let q_chars: Vec<char> = q_lower.chars().collect();
                let stem_chars: Vec<char> = name_stem.chars().collect();
                let q_len = q_chars.len();
                let stem_len = stem_chars.len();

                // If query is longer than filename, compare directly
                if q_len >= stem_len {
                    return super::query::levenshtein_bounded(&q_lower, &name_stem, *max_distance)
                        .is_some();
                }

                // Try sliding window of length q_len +/- max_distance
                let min_window = q_len.saturating_sub(*max_distance);
                let max_window = (q_len + *max_distance).min(stem_len);

                for window_size in min_window..=max_window {
                    for start in 0..=stem_len.saturating_sub(window_size) {
                        let window: String =
                            stem_chars[start..start + window_size].iter().collect();
                        if super::query::levenshtein_bounded(&q_lower, &window, *max_distance)
                            .is_some()
                        {
                            return true;
                        }
                    }
                }

                false
            }
        }
    }

    /// Search for files matching the query within a single bookmark
    pub fn search(&self, query: &ParsedQuery, bookmark_path: &str) -> Vec<SearchResult> {
        // Must be under the bookmark path, with a path-boundary check so
        // "/mnt/data" cannot leak in "/mnt/database" entries. An empty
        // bookmark path means no restriction.
        let mut matches = self.matching_entries(query, |entry| {
            bookmark_path.is_empty() || path_under_root(&entry.path, bookmark_path)
        });
        self.keep_top(&mut matches, query.sort_order);
        matches
            .into_iter()
            .map(|entry| SearchResult {
                path: entry.path.clone(),
                is_dir: entry.is_dir,
                mtime: entry.mtime,
                size: entry.size,
            })
            .collect()
    }

    /// Search all bookmarks at once (faster than multiple searches)
    pub fn search_all(
        &self,
        query: &ParsedQuery,
        bookmark_paths: &[String],
    ) -> Vec<SearchAllResult> {
        self.search_all_counted(query, bookmark_paths).0
    }

    /// `search_all`, plus how many paths matched before the result cap.
    pub fn search_all_counted(
        &self,
        query: &ParsedQuery,
        bookmark_paths: &[String],
    ) -> (Vec<SearchAllResult>, usize) {
        let bookmark_map: HashMap<&str, &str> = self
            .bookmarks
            .iter()
            .map(|b| (b.path.as_str(), b.name.as_str()))
            .collect();

        let search_paths: Vec<&str> = if bookmark_paths.is_empty() {
            self.bookmarks.iter().map(|b| b.path.as_str()).collect()
        } else {
            bookmark_paths.iter().map(|s| s.as_str()).collect()
        };

        // Find which bookmark a file belongs to (boundary-aware, so
        // "/mnt/database" is not attributed to bookmark "/mnt/data")
        let bookmark_of = |path: &str| {
            search_paths
                .iter()
                .find(|&&bp| path_under_root(path, bp))
                .and_then(|bp| bookmark_map.get(bp).copied())
        };

        let mut matches = self.matching_entries(query, |entry| bookmark_of(&entry.path).is_some());
        let total = matches.len();
        self.keep_top(&mut matches, query.sort_order);
        let results = matches
            .into_iter()
            .map(|entry| SearchAllResult {
                path: entry.path.clone(),
                is_dir: entry.is_dir,
                mtime: entry.mtime,
                size: entry.size,
                bookmark: bookmark_of(&entry.path).unwrap_or_default().to_string(),
            })
            .collect();
        (results, total)
    }

    /// Every indexed entry the query matches and `keep` accepts. The checks
    /// run in parallel across all cores, which matters when there are no
    /// trigrams to narrow with (`ab`, `*.rs` alone) and every path is checked.
    fn matching_entries<'a>(
        &'a self,
        query: &ParsedQuery,
        keep: impl Fn(&FileEntry) -> bool + Sync,
    ) -> Vec<&'a FileEntry> {
        let accept = |entry: &&FileEntry| keep(entry) && self.entry_matches(entry, query);
        match self.get_candidates(query) {
            Candidates::All => self
                .files
                .par_iter()
                .map(|(_, e)| e)
                .filter(accept)
                .collect(),
            Candidates::Ids(ids) => {
                debug!("search: {} candidates from trigrams", ids.len());
                ids.par_iter()
                    .filter_map(|id| self.files.get(id))
                    .filter(accept)
                    .collect()
            }
        }
    }

    /// File type, extension, size, and date filters, then the text match.
    fn entry_matches(&self, entry: &FileEntry, query: &ParsedQuery) -> bool {
        match query.file_type_mode {
            FileTypeMode::All => {}
            FileTypeMode::GotoDir => {
                if !entry.is_dir {
                    return false;
                }
            }
            FileTypeMode::GotoFile => {
                if entry.is_dir {
                    return false;
                }
            }
            FileTypeMode::Edit => {
                if entry.is_dir {
                    return false;
                }
                if let Some(ext) = Path::new(&entry.path).extension().and_then(|e| e.to_str()) {
                    if BINARY_EXTENSIONS.contains(&ext.to_lowercase().as_str()) {
                        return false;
                    }
                }
            }
        }

        if let Some(ref ext_filter) = query.extension_filter {
            match Path::new(&entry.path).extension().and_then(|e| e.to_str()) {
                Some(ext) if ext.eq_ignore_ascii_case(ext_filter) => {}
                _ => return false,
            }
        }

        query.meta_matches(entry.is_dir, entry.size, entry.mtime)
            && self.matches_query(&entry.path, query)
    }

    /// Sort `entries` by `order` and keep the first `max_results`. A linear
    /// selection picks those first, so a query matching a million paths does
    /// not pay for sorting all of them. Ties fall back to the path, which keeps
    /// the order the same from one search to the next.
    fn keep_top(&self, entries: &mut Vec<&FileEntry>, order: SortOrder) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let score = |e: &FileEntry| {
            self.access_data
                .get(&e.id)
                .map(|a| a.frecency_score_at(now))
                .unwrap_or(0.0)
        };
        let cmp = |a: &&FileEntry, b: &&FileEntry| compare_entries(a, b, order, &score);
        if self.max_results > 0 && entries.len() > self.max_results {
            entries.select_nth_unstable_by(self.max_results - 1, cmp);
            entries.truncate(self.max_results);
        }
        entries.par_sort_unstable_by(cmp);
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn trigram_count(&self) -> usize {
        self.trigrams.len()
    }
}

/// Order two entries by `order`, then by path.
fn compare_entries(
    a: &FileEntry,
    b: &FileEntry,
    order: SortOrder,
    score: &impl Fn(&FileEntry) -> f64,
) -> Ordering {
    let name = |e: &FileEntry| Path::new(&e.path).file_name().map(|n| n.to_os_string());
    let primary = match order {
        SortOrder::MtimeDesc => b.mtime.cmp(&a.mtime),
        SortOrder::MtimeAsc => a.mtime.cmp(&b.mtime),
        SortOrder::NameAsc => name(a).cmp(&name(b)),
        SortOrder::NameDesc => name(b).cmp(&name(a)),
        SortOrder::SizeDesc => b.size.cmp(&a.size),
        SortOrder::SizeAsc => a.size.cmp(&b.size),
        SortOrder::PathAsc => Ordering::Equal,
        SortOrder::Frecency => score(b).partial_cmp(&score(a)).unwrap_or(Ordering::Equal),
    };
    primary.then_with(|| a.path.cmp(&b.path))
}

/// Extract literal substrings from a regex pattern for trigram narrowing.
/// This is a best-effort extraction — it finds contiguous literal characters.
fn extract_regex_literals(pattern: &str) -> Vec<String> {
    let mut literals = Vec::new();
    let mut current = String::new();
    let mut escape = false;

    for ch in pattern.chars() {
        if escape {
            // Escaped character — some are literals (e.g., \. \- \/)
            match ch {
                '.' | '-' | '/' | '_' | ' ' => current.push(ch),
                _ => {
                    if current.len() >= 3 {
                        literals.push(current.clone());
                    }
                    current.clear();
                }
            }
            escape = false;
        } else if ch == '\\' {
            escape = true;
        } else if ch.is_alphanumeric() || ch == '_' || ch == '-' || ch == '/' {
            current.push(ch);
        } else {
            // Metacharacter — break the current literal
            if current.len() >= 3 {
                literals.push(current.clone());
            }
            current.clear();
        }
    }

    if current.len() >= 3 {
        literals.push(current);
    }

    literals
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_trigrams() {
        let trigrams = TrigramIndex::extract_trigrams("hello");
        assert_eq!(trigrams.len(), 3); // hel, ell, llo
        assert_eq!(trigrams[0], [b'h', b'e', b'l']);
        assert_eq!(trigrams[1], [b'e', b'l', b'l']);
        assert_eq!(trigrams[2], [b'l', b'l', b'o']);
    }

    #[test]
    fn test_extract_trigrams_short() {
        assert!(TrigramIndex::extract_trigrams("ab").is_empty());
        assert!(TrigramIndex::extract_trigrams("").is_empty());
    }

    #[test]
    fn test_extract_trigrams_case_insensitive() {
        let upper = TrigramIndex::extract_trigrams("HELLO");
        let lower = TrigramIndex::extract_trigrams("hello");
        assert_eq!(upper, lower);
    }

    #[test]
    fn test_add_and_search() {
        let mut index = TrigramIndex::new();
        index.bookmarks.push(Bookmark {
            name: "test".to_string(),
            path: "/tmp/test".to_string(),
            is_network: false,
        });

        index.add("/tmp/test/readme.md".to_string(), false, 1000, 100);
        index.add("/tmp/test/src/main.rs".to_string(), false, 2000, 200);
        index.add("/tmp/test/docs".to_string(), true, 500, 0);

        let query = ParsedQuery {
            mode: QueryMode::Substring("readme".to_string()),
            extension_filter: None,
            file_type_mode: FileTypeMode::All,
            sort_order: SortOrder::MtimeDesc,
            path_segments: None,
            ..ParsedQuery::parse("", SortOrder::MtimeDesc)
        };

        let results = index.search(&query, "/tmp/test");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "/tmp/test/readme.md");
    }

    #[test]
    fn test_add_and_remove() {
        let mut index = TrigramIndex::new();
        index.add("/tmp/test/file.txt".to_string(), false, 1000, 50);
        assert_eq!(index.file_count(), 1);

        index.remove("/tmp/test/file.txt");
        assert_eq!(index.file_count(), 0);
    }

    #[test]
    fn test_sort_orders() {
        let mut index = TrigramIndex::new();
        index.bookmarks.push(Bookmark {
            name: "test".to_string(),
            path: "/tmp".to_string(),
            is_network: false,
        });

        index.add("/tmp/aaa.txt".to_string(), false, 100, 300);
        index.add("/tmp/zzz.txt".to_string(), false, 300, 100);
        index.add("/tmp/mmm.txt".to_string(), false, 200, 200);

        // Sort by name ascending
        let query = ParsedQuery {
            mode: QueryMode::Substring(String::new()),
            extension_filter: Some("txt".to_string()),
            file_type_mode: FileTypeMode::All,
            sort_order: SortOrder::NameAsc,
            path_segments: None,
            ..ParsedQuery::parse("", SortOrder::MtimeDesc)
        };
        let results = index.search(&query, "/tmp");
        assert_eq!(results[0].path, "/tmp/aaa.txt");
        assert_eq!(results[2].path, "/tmp/zzz.txt");

        // Sort by size desc
        let query = ParsedQuery {
            sort_order: SortOrder::SizeDesc,
            ..query
        };
        let results = index.search(&query, "/tmp");
        assert_eq!(results[0].size, 300);
    }

    #[test]
    fn test_bookmark_boundary_no_sibling_prefix_leak() {
        let mut index = TrigramIndex::new();
        index.bookmarks.push(Bookmark {
            name: "data".to_string(),
            path: "/mnt/data".to_string(),
            is_network: false,
        });
        index.add("/mnt/data/file.txt".to_string(), false, 100, 1);
        index.add("/mnt/database/other.txt".to_string(), false, 100, 1);

        let query = ParsedQuery::parse("*.txt", SortOrder::MtimeDesc);
        let results = index.search(&query, "/mnt/data");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "/mnt/data/file.txt");

        let all = index.search_all(&query, &[]);
        assert_eq!(
            all.len(),
            1,
            "sibling-prefix paths must not be attributed to a bookmark"
        );
        assert_eq!(all[0].bookmark, "data");
    }

    #[test]
    fn test_max_results_truncation() {
        let mut index = TrigramIndex::new();
        index.bookmarks.push(Bookmark {
            name: "t".to_string(),
            path: "/t".to_string(),
            is_network: false,
        });
        index.max_results = 3;
        for i in 0..10 {
            index.add(format!("/t/file{}.txt", i), false, i, 0);
        }
        let query = ParsedQuery::parse("*.txt", SortOrder::MtimeDesc);
        assert_eq!(index.search(&query, "/t").len(), 3);
        assert_eq!(index.search_all(&query, &[]).len(), 3);
    }

    #[test]
    fn test_extract_regex_literals() {
        let lits = extract_regex_literals("^README\\.md$");
        assert!(lits.iter().any(|l| l.contains("README")));

        let lits = extract_regex_literals("foo.*bar");
        assert!(lits.iter().any(|l| l == "foo"));
        assert!(lits.iter().any(|l| l == "bar"));
    }

    /// Three files whose name, mtime, size, and path orders all disagree, so
    /// each sort order produces a distinct, checkable sequence.
    fn sort_fixture() -> TrigramIndex {
        let mut index = TrigramIndex::new();
        index.bookmarks.push(Bookmark {
            name: "t".to_string(),
            path: "/t".to_string(),
            is_network: false,
        });
        index.add("/t/b/beta-report.txt".to_string(), false, 300, 20);
        index.add("/t/a/alpha-report.txt".to_string(), false, 100, 30);
        index.add("/t/c/gamma-report.txt".to_string(), false, 200, 10);
        index
    }

    fn names<P: AsRef<str>>(paths: impl IntoIterator<Item = P>) -> Vec<String> {
        paths
            .into_iter()
            .map(|p| {
                Path::new(p.as_ref())
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    #[test]
    fn every_sort_order_ranks_results_as_named() {
        let mut index = sort_fixture();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        // gamma opened often and just now, beta once long ago, alpha never
        let gamma = index.path_to_id["/t/c/gamma-report.txt"];
        let beta = index.path_to_id["/t/b/beta-report.txt"];
        index.access_data.insert(
            gamma,
            AccessInfo {
                open_count: 5,
                last_opened: now,
            },
        );
        index.access_data.insert(
            beta,
            AccessInfo {
                open_count: 1,
                last_opened: now - 90 * 86400,
            },
        );

        let expected = [
            (SortOrder::MtimeDesc, ["beta", "gamma", "alpha"]),
            (SortOrder::MtimeAsc, ["alpha", "gamma", "beta"]),
            (SortOrder::NameAsc, ["alpha", "beta", "gamma"]),
            (SortOrder::NameDesc, ["gamma", "beta", "alpha"]),
            (SortOrder::SizeDesc, ["alpha", "beta", "gamma"]),
            (SortOrder::SizeAsc, ["gamma", "beta", "alpha"]),
            (SortOrder::PathAsc, ["alpha", "beta", "gamma"]),
            (SortOrder::Frecency, ["gamma", "beta", "alpha"]),
        ];
        for (order, want) in expected {
            let want: Vec<String> = want.iter().map(|n| format!("{n}-report.txt")).collect();
            let q = ParsedQuery::parse("report", order);
            let all = names(index.search_all(&q, &[]).iter().map(|r| r.path.clone()));
            assert_eq!(all, want, "search_all {:?}", order);
            // The single-bookmark search (used over IPC) sorts the same way
            let one = names(index.search(&q, "/t").iter().map(|r| r.path.clone()));
            assert_eq!(one, want, "search {:?}", order);
        }
    }

    #[test]
    fn regex_glob_and_fuzzy_queries_find_their_files() {
        let index = sort_fixture();
        let found = |raw: &str| {
            let q = ParsedQuery::parse(raw, SortOrder::NameAsc);
            names(index.search_all(&q, &[]).iter().map(|r| r.path.clone()))
        };
        assert_eq!(found("/alph[a]-rep"), ["alpha-report.txt"]);
        assert_eq!(found("gam*rep"), ["gamma-report.txt"]);
        // One typo away from "beta"
        assert_eq!(found("~betta"), ["beta-report.txt"]);
        assert!(found("~zzzzzz").is_empty());
    }

    /// Media files with sizes and dates for the filter and word tests.
    fn media_fixture() -> TrigramIndex {
        let mut index = TrigramIndex::new();
        index.bookmarks.push(Bookmark {
            name: "nas".to_string(),
            path: "/nas".to_string(),
            is_network: true,
        });
        let gib = 1u64 << 30;
        index.add(
            "/nas/movies/All That's Left of You (2025)".to_string(),
            true,
            500,
            0,
        );
        index.add(
            "/nas/movies/All That's Left of You (2025)/movie.mkv".to_string(),
            false,
            1000,
            10 * gib,
        );
        index.add(
            "/nas/movies/All That's Left of You (2025)/Trailer.mkv".to_string(),
            false,
            1000,
            gib / 2,
        );
        index.add(
            "/nas/movies/Left Behind (2014)/movie.mkv".to_string(),
            false,
            2000,
            3 * gib,
        );
        index.add("/nas/music/left.flac".to_string(), false, 3000, 40 << 20);
        index
    }

    fn found(index: &TrigramIndex, raw: &str) -> Vec<String> {
        let q = ParsedQuery::parse(raw, SortOrder::PathAsc);
        index
            .search_all(&q, &[])
            .into_iter()
            .map(|r| r.path)
            .collect()
    }

    #[test]
    fn words_exclusions_and_filters_combine() {
        let index = media_fixture();
        // Words in any order, across folder and file names
        assert_eq!(found(&index, "you mkv left").len(), 2);
        // Excluding a word drops the trailer
        assert_eq!(
            found(&index, "you mkv !trailer"),
            ["/nas/movies/All That's Left of You (2025)/movie.mkv"]
        );
        // Size: only files, never the folder
        assert_eq!(found(&index, "size:>1G").len(), 2);
        assert_eq!(found(&index, "left size:<100M"), ["/nas/music/left.flac"]);
        // Date: everything modified on or after the 1000 s mark
        let q = ParsedQuery {
            mtime_filter: Some(super::super::query::Interval {
                start: Some(1000),
                end: None,
            }),
            ..ParsedQuery::parse("", SortOrder::PathAsc)
        };
        assert_eq!(index.search_all(&q, &[]).len(), 4);
    }

    #[test]
    fn capped_results_are_the_top_of_the_full_order_and_report_the_total() {
        let mut index = TrigramIndex::new();
        index.bookmarks.push(Bookmark {
            name: "t".to_string(),
            path: "/t".to_string(),
            is_network: false,
        });
        for i in 0..200u64 {
            // Sizes deliberately out of order relative to insertion
            index.add(
                format!("/t/report-{i:03}.txt"),
                false,
                i as i64,
                (i * 7919) % 1000,
            );
        }
        let q = ParsedQuery::parse("report", SortOrder::SizeDesc);
        let full: Vec<String> = index
            .search_all(&q, &[])
            .into_iter()
            .map(|r| r.path)
            .collect();

        index.max_results = 10;
        let (top, total) = index.search_all_counted(&q, &[]);
        assert_eq!(total, 200);
        let top: Vec<String> = top.into_iter().map(|r| r.path).collect();
        assert_eq!(top, full[..10].to_vec());
    }

    #[test]
    fn queries_without_trigrams_scan_every_path() {
        let index = media_fixture();
        // Two letters have no trigram to narrow with
        assert_eq!(found(&index, "mo").len(), 4);
        assert_eq!(found(&index, "*.flac"), ["/nas/music/left.flac"]);
    }
}
