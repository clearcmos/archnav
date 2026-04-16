use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;

use super::query::{FileTypeMode, ParsedQuery, QueryMode, SortOrder};

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
        if self.open_count == 0 {
            return 0.0;
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let hours_ago = (now - self.last_opened) as f64 / 3600.0;
        let recency_weight = if hours_ago < 1.0 {
            4.0  // Last hour: 4x
        } else if hours_ago < 24.0 {
            2.0  // Last day: 2x
        } else if hours_ago < 168.0 {
            1.5  // Last week: 1.5x
        } else if hours_ago < 720.0 {
            1.0  // Last month: 1x
        } else {
            0.5  // Older: 0.5x
        };

        // Log scale for frequency to prevent runaway counts
        (self.open_count as f64).ln_1p() * recency_weight
    }
}

pub const MAX_RESULTS: usize = 2000;

const BINARY_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "ico", "webp", "svg", "tiff", "raw",
    "mp3", "mp4", "wav", "avi", "mkv", "mov", "flac", "ogg", "m4a", "aac",
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx",
    "zip", "tar", "gz", "bz2", "xz", "7z", "rar", "zst",
    "exe", "dll", "so", "dylib", "a", "o", "obj",
    "bin", "dat", "db", "sqlite", "sqlite3",
    "ttf", "otf", "woff", "woff2", "eot",
    "class", "jar", "war", "pyc", "pyo", "whl",
    "min.js", "min.css",
];

pub const EXCLUDE_PATTERNS: &[&str] = &[
    ".git", "node_modules", "__pycache__", ".cache", ".npm", ".cargo",
    "target", "build", "dist", ".next", ".nuxt", ".Trash", "Trash",
    ".steam", "dosdevices", // Wine/Proton paths with z: symlink to root
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub id: u32,
    pub path: String,
    pub is_dir: bool,
    pub mtime: i64,
    pub size: u64,
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

pub struct TrigramIndex {
    pub trigrams: HashMap<[u8; 3], HashSet<u32>>,
    pub files: HashMap<u32, FileEntry>,
    pub path_to_id: HashMap<String, u32>,
    pub next_id: u32,
    pub bookmarks: Vec<Bookmark>,
    /// Access data for frecency scoring (file_id -> AccessInfo)
    pub access_data: HashMap<u32, AccessInfo>,
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
    pub fn add(&mut self, path: String, is_dir: bool, mtime: i64, size: u64) -> (u32, Vec<[u8; 3]>) {
        // Check if already exists — update metadata
        if let Some(&existing_id) = self.path_to_id.get(&path) {
            if let Some(entry) = self.files.get_mut(&existing_id) {
                entry.mtime = mtime;
                entry.size = size;
            }
            return (existing_id, Vec::new());
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

        (id, file_trigrams)
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
    fn get_candidates(&self, query: &ParsedQuery) -> HashSet<u32> {
        match &query.mode {
            QueryMode::Substring(q) => {
                let trigrams = Self::extract_trigrams(&q.to_lowercase());
                self.intersect_trigrams(&trigrams)
            }
            QueryMode::Regex(re) => {
                // Try to extract literal fragments from regex for trigram narrowing
                let pattern = re.as_str();
                let literals = extract_regex_literals(pattern);
                if literals.is_empty() {
                    // No literals — scan all files
                    self.files.keys().copied().collect()
                } else {
                    // Use longest literal for trigram narrowing
                    let best = literals.iter().max_by_key(|s| s.len()).unwrap();
                    let trigrams = Self::extract_trigrams(&best.to_lowercase());
                    self.intersect_trigrams(&trigrams)
                }
            }
            QueryMode::Glob(_) => {
                // Glob patterns are hard to extract trigrams from; scan all
                self.files.keys().copied().collect()
            }
            QueryMode::Fuzzy { query, max_distance } => {
                // For fuzzy matching, trigram-based narrowing doesn't work well because
                // typos change the trigrams. For short queries or high edit distance,
                // scan all files. For longer queries, use lenient trigram matching.
                let trigrams = Self::extract_trigrams(&query.to_lowercase());

                // If query is short or has few trigrams, scan all files
                // (Levenshtein will filter properly)
                if query.len() < 5 || trigrams.len() < 2 || *max_distance >= 2 {
                    return self.files.keys().copied().collect();
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
                    self.files.keys().copied().collect()
                } else {
                    candidates
                }
            }
        }
    }

    /// Intersect posting lists for given trigrams
    fn intersect_trigrams(&self, trigrams: &[[u8; 3]]) -> HashSet<u32> {
        if trigrams.is_empty() {
            return self.files.keys().copied().collect();
        }

        let mut iter = trigrams.iter();
        let first = iter.next().unwrap();
        let mut candidates = self
            .trigrams
            .get(first)
            .cloned()
            .unwrap_or_default();

        for trigram in iter {
            if let Some(set) = self.trigrams.get(trigram) {
                candidates = candidates.intersection(set).copied().collect();
            } else {
                return HashSet::new();
            }
            if candidates.is_empty() {
                return HashSet::new();
            }
        }

        candidates
    }

    /// Check if a path matches the query
    fn matches_query(&self, path: &str, query: &ParsedQuery) -> bool {
        // Check path segments first (for path-aware search like "src/config")
        if !query.matches_path_segments(path) {
            return false;
        }

        match &query.mode {
            QueryMode::Substring(q) => {
                if q.is_empty() {
                    return true;
                }
                path.to_lowercase().contains(&q.to_lowercase())
            }
            QueryMode::Regex(re) => re.is_match(path),
            QueryMode::Glob(matcher) => {
                // Match against filename
                let filename = Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path);
                matcher.is_match(filename) || matcher.is_match(path)
            }
            QueryMode::Fuzzy { query: q, max_distance } => {
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
                    return super::query::levenshtein_bounded(&q_lower, &name_stem, *max_distance).is_some();
                }

                // Try sliding window of length q_len +/- max_distance
                let min_window = q_len.saturating_sub(*max_distance);
                let max_window = (q_len + *max_distance).min(stem_len);

                for window_size in min_window..=max_window {
                    for start in 0..=stem_len.saturating_sub(window_size) {
                        let window: String = stem_chars[start..start + window_size].iter().collect();
                        if super::query::levenshtein_bounded(&q_lower, &window, *max_distance).is_some() {
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
        let candidates = self.get_candidates(query);

        let mut results: Vec<SearchResult> = candidates
            .into_iter()
            .filter_map(|id| self.files.get(&id))
            .filter(|entry| {
                // Must be under the bookmark path
                if !entry.path.starts_with(bookmark_path) {
                    return false;
                }

                // File type mode filter
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
                        if let Some(ext) =
                            Path::new(&entry.path).extension().and_then(|e| e.to_str())
                        {
                            if BINARY_EXTENSIONS.contains(&ext.to_lowercase().as_str()) {
                                return false;
                            }
                        }
                    }
                }

                // Extension filter
                if let Some(ref ext_filter) = query.extension_filter {
                    if let Some(ext) =
                        Path::new(&entry.path).extension().and_then(|e| e.to_str())
                    {
                        if ext.to_lowercase() != ext_filter.to_lowercase() {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }

                // Verify actual match (trigrams can produce false positives)
                self.matches_query(&entry.path, query)
            })
            .map(|entry| SearchResult {
                path: entry.path.clone(),
                is_dir: entry.is_dir,
                mtime: entry.mtime,
                size: entry.size,
            })
            .collect();

        sort_results(&mut results, query.sort_order, &self.path_to_id, &self.access_data);
        results.truncate(MAX_RESULTS);
        results
    }

    /// Search all bookmarks at once (faster than multiple searches)
    pub fn search_all(&self, query: &ParsedQuery, bookmark_paths: &[String]) -> Vec<SearchAllResult> {
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

        let candidates = self.get_candidates(query);
        debug!("search_all: {} candidates from trigrams", candidates.len());

        let mut results: Vec<SearchAllResult> = candidates
            .into_iter()
            .filter_map(|id| self.files.get(&id))
            .filter_map(|entry| {
                // Find which bookmark this file belongs to
                let bookmark_name = search_paths
                    .iter()
                    .find(|&&bp| entry.path.starts_with(bp))
                    .and_then(|bp| bookmark_map.get(bp).copied())?;

                // Extension filter
                if let Some(ref ext_filter) = query.extension_filter {
                    if let Some(ext) =
                        Path::new(&entry.path).extension().and_then(|e| e.to_str())
                    {
                        if ext.to_lowercase() != ext_filter.to_lowercase() {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }

                // Verify actual match
                if !self.matches_query(&entry.path, query) {
                    return None;
                }

                Some(SearchAllResult {
                    path: entry.path.clone(),
                    is_dir: entry.is_dir,
                    mtime: entry.mtime,
                    size: entry.size,
                    bookmark: bookmark_name.to_string(),
                })
            })
            .collect();

        sort_all_results(&mut results, query.sort_order, &self.path_to_id, &self.access_data);
        results.truncate(MAX_RESULTS);
        results
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn trigram_count(&self) -> usize {
        self.trigrams.len()
    }
}

/// Sort SearchResult by the given order
fn sort_results(
    results: &mut Vec<SearchResult>,
    order: SortOrder,
    path_to_id: &HashMap<String, u32>,
    access_data: &HashMap<u32, AccessInfo>,
) {
    match order {
        SortOrder::MtimeDesc => results.sort_by(|a, b| b.mtime.cmp(&a.mtime)),
        SortOrder::MtimeAsc => results.sort_by(|a, b| a.mtime.cmp(&b.mtime)),
        SortOrder::NameAsc => results.sort_by(|a, b| {
            let na = Path::new(&a.path).file_name().unwrap_or_default();
            let nb = Path::new(&b.path).file_name().unwrap_or_default();
            na.cmp(&nb)
        }),
        SortOrder::NameDesc => results.sort_by(|a, b| {
            let na = Path::new(&a.path).file_name().unwrap_or_default();
            let nb = Path::new(&b.path).file_name().unwrap_or_default();
            nb.cmp(&na)
        }),
        SortOrder::SizeDesc => results.sort_by(|a, b| b.size.cmp(&a.size)),
        SortOrder::SizeAsc => results.sort_by(|a, b| a.size.cmp(&b.size)),
        SortOrder::PathAsc => results.sort_by(|a, b| a.path.cmp(&b.path)),
        SortOrder::Frecency => results.sort_by(|a, b| {
            let score_a = path_to_id
                .get(&a.path)
                .and_then(|id| access_data.get(id))
                .map(|ai| ai.frecency_score())
                .unwrap_or(0.0);
            let score_b = path_to_id
                .get(&b.path)
                .and_then(|id| access_data.get(id))
                .map(|ai| ai.frecency_score())
                .unwrap_or(0.0);
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        }),
    }
}

/// Sort SearchAllResult by the given order
fn sort_all_results(
    results: &mut Vec<SearchAllResult>,
    order: SortOrder,
    path_to_id: &HashMap<String, u32>,
    access_data: &HashMap<u32, AccessInfo>,
) {
    match order {
        SortOrder::MtimeDesc => results.sort_by(|a, b| b.mtime.cmp(&a.mtime)),
        SortOrder::MtimeAsc => results.sort_by(|a, b| a.mtime.cmp(&b.mtime)),
        SortOrder::NameAsc => results.sort_by(|a, b| {
            let na = Path::new(&a.path).file_name().unwrap_or_default();
            let nb = Path::new(&b.path).file_name().unwrap_or_default();
            na.cmp(&nb)
        }),
        SortOrder::NameDesc => results.sort_by(|a, b| {
            let na = Path::new(&a.path).file_name().unwrap_or_default();
            let nb = Path::new(&b.path).file_name().unwrap_or_default();
            nb.cmp(&na)
        }),
        SortOrder::SizeDesc => results.sort_by(|a, b| b.size.cmp(&a.size)),
        SortOrder::SizeAsc => results.sort_by(|a, b| a.size.cmp(&b.size)),
        SortOrder::PathAsc => results.sort_by(|a, b| a.path.cmp(&b.path)),
        SortOrder::Frecency => results.sort_by(|a, b| {
            let score_a = path_to_id
                .get(&a.path)
                .and_then(|id| access_data.get(id))
                .map(|ai| ai.frecency_score())
                .unwrap_or(0.0);
            let score_b = path_to_id
                .get(&b.path)
                .and_then(|id| access_data.get(id))
                .map(|ai| ai.frecency_score())
                .unwrap_or(0.0);
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        }),
    }
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
    fn test_extract_regex_literals() {
        let lits = extract_regex_literals("^README\\.md$");
        assert!(lits.iter().any(|l| l.contains("README")));

        let lits = extract_regex_literals("foo.*bar");
        assert!(lits.iter().any(|l| l == "foo"));
        assert!(lits.iter().any(|l| l == "bar"));
    }
}
