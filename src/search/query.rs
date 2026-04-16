use globset::Glob;
use regex::Regex;

#[derive(Debug, Clone)]
pub enum QueryMode {
    Substring(String),
    Regex(Regex),
    Glob(globset::GlobMatcher),
    /// Fuzzy matching with maximum edit distance
    Fuzzy { query: String, max_distance: usize },
}

/// Calculate the maximum allowed edit distance based on query length.
fn max_distance_for_query(query_len: usize) -> usize {
    match query_len {
        0..=3 => 0,   // Too short for fuzzy - exact match only
        4..=6 => 1,   // 1 typo allowed
        7..=12 => 2,  // 2 typos allowed
        _ => 3,       // 3 typos max
    }
}

/// Levenshtein distance with early termination if distance exceeds max.
/// Returns None if distance > max_dist.
pub fn levenshtein_bounded(a: &str, b: &str, max_dist: usize) -> Option<usize> {
    let a = a.to_lowercase();
    let b = b.to_lowercase();

    let a_len = a.chars().count();
    let b_len = b.chars().count();

    // Quick length check
    let len_diff = (a_len as isize - b_len as isize).unsigned_abs();
    if len_diff > max_dist {
        return None;
    }

    // Use iterative DP with two rows
    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0; b_len + 1];

    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        let mut min_in_row = curr[0];

        for (j, cb) in b.chars().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j] + cost)
                .min(prev[j + 1] + 1)
                .min(curr[j] + 1);
            min_in_row = min_in_row.min(curr[j + 1]);
        }

        // Early termination: if all values in this row exceed max, no point continuing
        if min_in_row > max_dist {
            return None;
        }

        std::mem::swap(&mut prev, &mut curr);
    }

    if prev[b_len] <= max_dist {
        Some(prev[b_len])
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortOrder {
    MtimeDesc,
    MtimeAsc,
    NameAsc,
    NameDesc,
    SizeDesc,
    SizeAsc,
    PathAsc,
    Frecency,
}

impl SortOrder {
    pub fn from_index(index: i32) -> Self {
        match index {
            0 => SortOrder::MtimeDesc,
            1 => SortOrder::MtimeAsc,
            2 => SortOrder::NameAsc,
            3 => SortOrder::NameDesc,
            4 => SortOrder::SizeDesc,
            5 => SortOrder::SizeAsc,
            6 => SortOrder::PathAsc,
            7 => SortOrder::Frecency,
            _ => SortOrder::MtimeDesc,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FileTypeMode {
    All,
    Edit,
    GotoFile,
    GotoDir,
}

#[derive(Debug, Clone)]
pub struct ParsedQuery {
    pub mode: QueryMode,
    pub extension_filter: Option<String>,
    pub file_type_mode: FileTypeMode,
    pub sort_order: SortOrder,
    /// Path segments for path-aware search (e.g., "src/config" -> ["src", "config"])
    pub path_segments: Option<Vec<String>>,
}

impl ParsedQuery {
    /// Check if a path matches this query (for cache filtering).
    pub fn matches_path(&self, path: &str) -> bool {
        // Check extension filter first
        if let Some(ref ext_filter) = self.extension_filter {
            if let Some(ext) = std::path::Path::new(path).extension().and_then(|e| e.to_str()) {
                if ext.to_lowercase() != ext_filter.to_lowercase() {
                    return false;
                }
            } else {
                return false;
            }
        }

        // Check path segments (for path-aware search like "src/config")
        if !self.matches_path_segments(path) {
            return false;
        }

        // Check query mode
        match &self.mode {
            QueryMode::Substring(q) => {
                if q.is_empty() {
                    return true;
                }
                path.to_lowercase().contains(&q.to_lowercase())
            }
            QueryMode::Regex(re) => re.is_match(path),
            QueryMode::Glob(matcher) => {
                let filename = std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path);
                matcher.is_match(filename) || matcher.is_match(path)
            }
            QueryMode::Fuzzy { query, max_distance } => {
                if query.is_empty() {
                    return true;
                }
                // For fuzzy, we match against filename only
                let filename = std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path);

                // First try exact substring match
                if filename.to_lowercase().contains(&query.to_lowercase()) {
                    return true;
                }

                // Then try fuzzy match on filename (without extension)
                let name_without_ext = std::path::Path::new(filename)
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or(filename);

                levenshtein_bounded(query, name_without_ext, *max_distance).is_some()
            }
        }
    }

    /// Parse a raw query string with optional extension filter.
    ///
    /// Syntax:
    ///   "query"                — substring search across all locations
    ///   "*.py query"           — filter by extension (position-independent)
    ///   "query *.py"           — same as above
    ///   "/regex"               — regex search (prefix with /)
    ///   "foo*bar"              — glob pattern (contains * or ?)
    ///   "src/config"           — path-aware search (matches "config" under "src")
    pub fn parse(raw: &str, sort_order: SortOrder) -> Self {
        let mut extension_filter: Option<String> = None;
        let mut path_segments: Option<Vec<String>> = None;

        // Split into tokens and find extension filter anywhere in query
        let tokens: Vec<&str> = raw.trim().split_whitespace().collect();
        let mut query_tokens: Vec<&str> = Vec::new();

        for token in tokens {
            if token.starts_with("*.") && token.len() > 2 {
                // Extension filter like "*.pdf" - extract extension
                extension_filter = Some(token[2..].to_string());
            } else {
                query_tokens.push(token);
            }
        }

        let mut text = query_tokens.join(" ");

        // Check for fuzzy search: starts with "~"
        if text.starts_with('~') && text.len() > 1 {
            let fuzzy_query = text[1..].to_string();
            let max_distance = max_distance_for_query(fuzzy_query.len());

            return Self {
                mode: QueryMode::Fuzzy {
                    query: fuzzy_query,
                    max_distance,
                },
                extension_filter,
                file_type_mode: FileTypeMode::All,
                sort_order,
                path_segments: None,
            };
        }

        // Check for path-aware search: contains "/" but doesn't start with "/" (regex)
        // and doesn't contain glob wildcards
        let is_path_query = !text.is_empty()
            && !text.starts_with('/')
            && text.contains('/')
            && !text.contains('*')
            && !text.contains('?');

        if is_path_query {
            // Split into path segments
            let segments: Vec<String> = text
                .split('/')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();

            if segments.len() >= 2 {
                // Use last segment as the search term for trigram lookup
                let search_term = segments.last().unwrap().clone();
                path_segments = Some(segments);

                return Self {
                    mode: QueryMode::Substring(search_term),
                    extension_filter,
                    file_type_mode: FileTypeMode::All,
                    sort_order,
                    path_segments,
                };
            } else if segments.len() == 1 {
                // "src/" with trailing slash - treat as normal search for "src"
                text = segments[0].clone();
            }
        }

        // Determine query mode
        let mode = if text.starts_with('/') {
            // Regex mode
            let pattern = &text[1..];
            match Regex::new(&format!("(?i){}", pattern)) {
                Ok(re) => QueryMode::Regex(re),
                Err(_) => QueryMode::Substring(text),
            }
        } else if !text.starts_with('/') && (text.contains('*') || text.contains('?')) {
            // Glob mode (but not if it's already been parsed as extension filter)
            match Glob::new(&format!("*{}*", text)) {
                Ok(glob) => QueryMode::Glob(glob.compile_matcher()),
                Err(_) => QueryMode::Substring(text),
            }
        } else {
            QueryMode::Substring(text)
        };

        Self {
            mode,
            extension_filter,
            file_type_mode: FileTypeMode::All,
            sort_order,
            path_segments,
        }
    }

    /// Check if a path matches the path segment pattern.
    /// For "src/config", checks that "src" appears as a directory component
    /// before "config" appears in the filename or path.
    pub fn matches_path_segments(&self, file_path: &str) -> bool {
        let segments = match &self.path_segments {
            Some(s) if s.len() >= 2 => s,
            _ => return true, // No path segments to match
        };

        let file_lower = file_path.to_lowercase();
        let path_parts: Vec<&str> = file_lower.split('/').collect();

        if path_parts.is_empty() {
            return false;
        }

        // Last segment must match filename (substring match)
        let filename_pattern = segments.last().unwrap().to_lowercase();
        let filename = path_parts.last().unwrap_or(&"");
        if !filename.contains(&filename_pattern) {
            return false;
        }

        // Earlier segments must appear in order in the directory path
        let dir_patterns: Vec<String> = segments[..segments.len() - 1]
            .iter()
            .map(|s| s.to_lowercase())
            .collect();
        let dir_parts = &path_parts[..path_parts.len().saturating_sub(1)];

        let mut pattern_idx = 0;
        for part in dir_parts {
            if pattern_idx < dir_patterns.len() && part.contains(&dir_patterns[pattern_idx]) {
                pattern_idx += 1;
            }
        }

        pattern_idx == dir_patterns.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        let q = ParsedQuery::parse("readme", SortOrder::MtimeDesc);
        assert!(matches!(q.mode, QueryMode::Substring(ref s) if s == "readme"));
        assert!(q.extension_filter.is_none());
    }

    #[test]
    fn test_parse_extension_filter() {
        let q = ParsedQuery::parse("*.py test", SortOrder::MtimeDesc);
        assert!(matches!(q.mode, QueryMode::Substring(ref s) if s == "test"));
        assert_eq!(q.extension_filter, Some("py".to_string()));
    }

    #[test]
    fn test_parse_extension_filter_flexible() {
        // Extension filter at end (like Search Everything)
        let q = ParsedQuery::parse("marois *.pdf", SortOrder::MtimeDesc);
        assert!(matches!(q.mode, QueryMode::Substring(ref s) if s == "marois"));
        assert_eq!(q.extension_filter, Some("pdf".to_string()));

        // Extension filter in middle
        let q = ParsedQuery::parse("foo *.txt bar", SortOrder::MtimeDesc);
        assert!(matches!(q.mode, QueryMode::Substring(ref s) if s == "foo bar"));
        assert_eq!(q.extension_filter, Some("txt".to_string()));
    }

    #[test]
    fn test_parse_extension_only() {
        let q = ParsedQuery::parse("*.md", SortOrder::MtimeDesc);
        assert!(matches!(q.mode, QueryMode::Substring(ref s) if s.is_empty()));
        assert_eq!(q.extension_filter, Some("md".to_string()));
    }

    #[test]
    fn test_parse_regex() {
        let q = ParsedQuery::parse("/^README", SortOrder::MtimeDesc);
        assert!(matches!(q.mode, QueryMode::Regex(_)));
    }

    #[test]
    fn test_parse_glob() {
        let q = ParsedQuery::parse("foo*bar", SortOrder::MtimeDesc);
        assert!(matches!(q.mode, QueryMode::Glob(_)));
    }

    #[test]
    fn test_parse_fuzzy() {
        let q = ParsedQuery::parse("~confg", SortOrder::MtimeDesc);
        assert!(matches!(q.mode, QueryMode::Fuzzy { ref query, max_distance: 1 } if query == "confg"));
    }

    #[test]
    fn test_parse_path_aware() {
        let q = ParsedQuery::parse("src/config", SortOrder::MtimeDesc);
        assert!(matches!(q.mode, QueryMode::Substring(ref s) if s == "config"));
        assert_eq!(q.path_segments, Some(vec!["src".to_string(), "config".to_string()]));
    }

    #[test]
    fn test_path_segments_matching() {
        let q = ParsedQuery::parse("src/main", SortOrder::MtimeDesc);

        // Should match: "main" under "src"
        assert!(q.matches_path_segments("/home/user/project/src/main.rs"));
        assert!(q.matches_path_segments("/home/user/src/components/main.js"));

        // Should not match: no "src" in path
        assert!(!q.matches_path_segments("/home/user/project/lib/main.rs"));

        // Should not match: "src" comes after "main"
        assert!(!q.matches_path_segments("/home/user/main/src/file.rs"));
    }

    #[test]
    fn test_levenshtein() {
        // Exact match
        assert_eq!(levenshtein_bounded("config", "config", 2), Some(0));

        // One typo (deletion)
        assert_eq!(levenshtein_bounded("confg", "config", 2), Some(1));
        assert_eq!(levenshtein_bounded("cofig", "config", 2), Some(1)); // missing 'n'

        // One typo (substitution)
        assert_eq!(levenshtein_bounded("comfig", "config", 2), Some(1));

        // Two typos
        assert_eq!(levenshtein_bounded("cofg", "config", 2), Some(2)); // missing 'n' and 'i'

        // Too many typos
        assert_eq!(levenshtein_bounded("xyz", "config", 2), None);
    }
}
