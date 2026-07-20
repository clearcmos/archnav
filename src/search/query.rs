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

/// One tag filter group from a `t:` token, in disjunctive normal form:
/// the group matches when ANY alternative matches, and an alternative
/// (an AND-set built with `&`/`AND`) matches when ALL its names do.
/// Empty alternatives (a bare trailing `t:`) mean "any tagged file".
#[derive(Debug, Clone, PartialEq)]
pub struct TagFilter {
    pub alternatives: Vec<Vec<String>>,
}

impl TagFilter {
    /// Parse the tokens of one tag group: spaces separate OR-alternatives,
    /// `&` (attached or standalone) and uppercase `AND` join an AND-set,
    /// uppercase `OR` is an explicit no-op separator.
    fn parse_group(tokens: &[&str]) -> Self {
        let mut alternatives: Vec<Vec<String>> = Vec::new();
        let mut pending_and = false;
        for &raw in tokens {
            if raw == "&" || raw == "AND" {
                pending_and = true;
                continue;
            }
            if raw == "OR" {
                pending_and = false;
                continue;
            }
            if raw.starts_with('&') {
                pending_and = true;
            }
            let parts: Vec<String> = raw
                .split('&')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect();
            let trails = raw.ends_with('&');
            if !parts.is_empty() {
                if pending_and && !alternatives.is_empty() {
                    alternatives.last_mut().unwrap().extend(parts);
                } else {
                    alternatives.push(parts);
                }
                pending_and = false;
            }
            if trails {
                pending_and = true;
            }
        }
        Self { alternatives }
    }

    fn matches(&self, tags_lower: &[String]) -> bool {
        if self.alternatives.is_empty() {
            return !tags_lower.is_empty();
        }
        self.alternatives.iter().any(|and_set| {
            and_set.iter().all(|name| {
                let n = name.to_lowercase();
                tags_lower.iter().any(|t| t.contains(&n))
            })
        })
    }
}

#[derive(Debug, Clone)]
pub struct ParsedQuery {
    pub mode: QueryMode,
    pub extension_filter: Option<String>,
    pub file_type_mode: FileTypeMode,
    pub sort_order: SortOrder,
    /// Path segments for path-aware search (e.g., "src/config" -> ["src", "config"])
    pub path_segments: Option<Vec<String>>,
    /// Tag filter groups from `t:` tokens; every group must match (AND).
    pub tag_filters: Vec<TagFilter>,
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
                self.substring_haystack(path)
                    .to_lowercase()
                    .contains(&q.to_lowercase())
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
    ///   "query"                - substring search across all locations
    ///   "*.py query"           - filter by extension (position-independent)
    ///   "query *.py"           - same as above
    ///   "/regex"               - regex search (prefix with /)
    ///   "foo*bar"              - glob pattern (contains * or ?)
    ///   "src/config"           - path-aware search (matches "config" under "src")
    ///   "folder:movies"        - directories whose own name matches (not nested children)
    pub fn parse(raw: &str, sort_order: SortOrder) -> Self {
        let mut extension_filter: Option<String> = None;
        let mut path_segments: Option<Vec<String>> = None;

        // Strip "folder:" prefix (case-insensitive, with optional whitespace after).
        // Restricts results to directories only. The is_char_boundary guard
        // matters: slicing at a fixed byte offset panics when that byte falls
        // inside a multibyte character (e.g. a query of accented or CJK text),
        // and with panic=abort that kills the whole app mid-keystroke.
        let raw_trimmed = raw.trim();
        const FOLDER_PREFIX_LEN: usize = "folder:".len();
        let has_folder_prefix = raw_trimmed.len() >= FOLDER_PREFIX_LEN
            && raw_trimmed.is_char_boundary(FOLDER_PREFIX_LEN)
            && raw_trimmed[..FOLDER_PREFIX_LEN].eq_ignore_ascii_case("folder:");
        let (file_type_mode, raw_effective) = if has_folder_prefix {
            (FileTypeMode::GotoDir, raw_trimmed[FOLDER_PREFIX_LEN..].trim_start())
        } else {
            (FileTypeMode::All, raw_trimmed)
        };

        // Split into tokens and find extension and tag filters anywhere in query
        let tokens: Vec<&str> = raw_effective.split_whitespace().collect();
        let mut query_tokens: Vec<&str> = Vec::new();
        let mut tag_filters: Vec<TagFilter> = Vec::new();

        let is_tag_token =
            |t: &str| t.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("t:"));

        let mut i = 0;
        while i < tokens.len() {
            let token = tokens[i];
            if token.starts_with("*.") && token.len() > 2 {
                // Extension filter like "*.pdf" - extract extension
                extension_filter = Some(token[2..].to_string());
                i += 1;
            } else if is_tag_token(token) {
                // Tag filter. Attached form "t:coffee" is a single group from
                // this token, extended only by explicit connectives so that
                // "t:coffee AND outdoor" joins while "t:coffee patio" keeps
                // patio as search text. Detached form "t: ..." consumes the
                // remaining tokens as one tag group - spaces are OR, & / AND
                // join - so search text must come before it. A bare trailing
                // "t:" means "any tagged file".
                let value = &token[2..];
                if !value.is_empty() {
                    let mut group_tokens: Vec<&str> = vec![value];
                    while i + 1 < tokens.len() {
                        let next = tokens[i + 1];
                        let is_operator = next == "&" || next == "AND" || next == "OR";
                        if is_operator {
                            // consume the operator and its operand (unless the
                            // operand starts its own filter, e.g. "AND t:x")
                            group_tokens.push(next);
                            i += 1;
                            if i + 1 < tokens.len()
                                && !is_tag_token(tokens[i + 1])
                                && !tokens[i + 1].starts_with("*.")
                            {
                                group_tokens.push(tokens[i + 1]);
                                i += 1;
                            }
                        } else if next.starts_with('&') {
                            // "t:coffee &outdoor"
                            group_tokens.push(next);
                            i += 1;
                        } else if group_tokens.last().is_some_and(|t| t.ends_with('&')) {
                            // "t:coffee& outdoor"
                            group_tokens.push(next);
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    tag_filters.push(TagFilter::parse_group(&group_tokens));
                    i += 1;
                } else {
                    i += 1;
                    let mut group_tokens: Vec<&str> = Vec::new();
                    while i < tokens.len() {
                        let t = tokens[i];
                        if is_tag_token(t) {
                            break; // next t: group; the outer loop handles it
                        }
                        if t.starts_with("*.") && t.len() > 2 {
                            extension_filter = Some(t[2..].to_string());
                            i += 1;
                            continue;
                        }
                        group_tokens.push(t);
                        i += 1;
                    }
                    tag_filters.push(TagFilter::parse_group(&group_tokens));
                }
            } else {
                query_tokens.push(token);
                i += 1;
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
                file_type_mode,
                sort_order,
                path_segments: None,
                tag_filters,
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
                    file_type_mode,
                    sort_order,
                    path_segments,
                    tag_filters,
                };
            } else if segments.len() == 1 {
                // "src/" with trailing slash - treat as normal search for "src"
                text = segments[0].clone();
            }
        }

        // Determine query mode
        let mode = if let Some(pattern) = text.strip_prefix('/') {
            // Regex mode
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
            file_type_mode,
            sort_order,
            path_segments,
            tag_filters,
        }
    }

    /// Whether this query filters by tagdex tags (t: tokens).
    pub fn has_tag_filter(&self) -> bool {
        !self.tag_filters.is_empty()
    }

    /// Check a file's tags against every t: filter group (groups are ANDed;
    /// within a group, spaces are OR and & / AND join). Name matching is
    /// case-insensitive substring, consistent with the rest of the search
    /// syntax; an empty group (bare "t:") requires any tag at all.
    pub fn tags_match(&self, tags: &[String]) -> bool {
        let tags_lower: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();
        self.tag_filters.iter().all(|filter| filter.matches(&tags_lower))
    }

    /// Whether this query restricts results to directories only.
    pub fn dirs_only(&self) -> bool {
        matches!(self.file_type_mode, FileTypeMode::GotoDir)
    }

    /// The text a substring query should be tested against for `path`.
    ///
    /// `folder:` (GotoDir) queries match the directory's own name only, so
    /// `folder:tattoo` matches `.../tattoo` but not folders nested beneath it
    /// like `.../tattoo/designs`. Every other mode matches the full path,
    /// keeping normal search path-inclusive (typing `tattoo` still finds
    /// anything living under a tattoo folder).
    pub fn substring_haystack<'a>(&self, path: &'a str) -> &'a str {
        if self.dirs_only() {
            std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path)
        } else {
            path
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
    fn test_parse_folder_prefix_no_space() {
        let q = ParsedQuery::parse("folder:movies", SortOrder::MtimeDesc);
        assert!(matches!(q.mode, QueryMode::Substring(ref s) if s == "movies"));
        assert_eq!(q.file_type_mode, FileTypeMode::GotoDir);
    }

    #[test]
    fn test_parse_folder_prefix_with_space() {
        let q = ParsedQuery::parse("folder: movies", SortOrder::MtimeDesc);
        assert!(matches!(q.mode, QueryMode::Substring(ref s) if s == "movies"));
        assert_eq!(q.file_type_mode, FileTypeMode::GotoDir);
    }

    #[test]
    fn test_parse_folder_prefix_case_insensitive() {
        let q = ParsedQuery::parse("Folder:Movies", SortOrder::MtimeDesc);
        assert!(matches!(q.mode, QueryMode::Substring(ref s) if s == "Movies"));
        assert_eq!(q.file_type_mode, FileTypeMode::GotoDir);
    }

    #[test]
    fn test_parse_folder_prefix_only() {
        let q = ParsedQuery::parse("folder:", SortOrder::MtimeDesc);
        assert!(matches!(q.mode, QueryMode::Substring(ref s) if s.is_empty()));
        assert_eq!(q.file_type_mode, FileTypeMode::GotoDir);
    }

    #[test]
    fn test_parse_folder_prefix_with_extension() {
        let q = ParsedQuery::parse("folder:foo *.bar", SortOrder::MtimeDesc);
        assert!(matches!(q.mode, QueryMode::Substring(ref s) if s == "foo"));
        assert_eq!(q.extension_filter, Some("bar".to_string()));
        assert_eq!(q.file_type_mode, FileTypeMode::GotoDir);
    }

    #[test]
    fn test_parse_no_folder_prefix() {
        let q = ParsedQuery::parse("movies", SortOrder::MtimeDesc);
        assert_eq!(q.file_type_mode, FileTypeMode::All);
    }

    #[test]
    fn test_parse_multibyte_query_does_not_panic() {
        // Regression: the folder: prefix check sliced raw_trimmed[..7] by byte
        // offset, which panics when byte 7 is inside a multibyte character.
        for q in ["éééé", "ééééééé", "日本語データ", "héllo wörld", "folder:éé"] {
            let parsed = ParsedQuery::parse(q, SortOrder::MtimeDesc);
            assert!(matches!(parsed.mode, QueryMode::Substring(_)));
        }
        let q = ParsedQuery::parse("folder:été", SortOrder::MtimeDesc);
        assert_eq!(q.file_type_mode, FileTypeMode::GotoDir);
    }

    #[test]
    fn test_folder_query_matches_name_not_ancestor() {
        let q = ParsedQuery::parse("folder:tattoo", SortOrder::MtimeDesc);
        // The folder itself matches: its own name contains the query.
        assert!(q.matches_path("/home/u/Pictures/tattoo"));
        assert!(q.matches_path("/home/u/Pictures/my-tattoo-ideas"));
        // Folders nested under a matching ancestor must NOT match.
        assert!(!q.matches_path("/home/u/Pictures/tattoo/designs"));
        assert!(!q.matches_path("/home/u/Pictures/tattoo/designs/flash"));
    }

    #[test]
    fn test_folder_query_name_match_is_case_insensitive() {
        let q = ParsedQuery::parse("folder:Tattoo", SortOrder::MtimeDesc);
        assert!(q.matches_path("/home/u/TATTOO"));
        assert!(!q.matches_path("/home/u/TATTOO/inner"));
    }

    #[test]
    fn test_non_folder_substring_stays_path_inclusive() {
        // Regular (non-folder:) search still matches anywhere in the path.
        let q = ParsedQuery::parse("tattoo", SortOrder::MtimeDesc);
        assert!(q.matches_path("/home/u/tattoo/designs/photo.jpg"));
    }

    fn alts(q: &ParsedQuery) -> Vec<Vec<Vec<String>>> {
        q.tag_filters.iter().map(|f| f.alternatives.clone()).collect()
    }

    fn group(alternatives: &[&[&str]]) -> Vec<Vec<String>> {
        alternatives
            .iter()
            .map(|a| a.iter().map(|s| s.to_string()).collect())
            .collect()
    }

    #[test]
    fn test_parse_tag_filter_attached() {
        let q = ParsedQuery::parse("t:coffee", SortOrder::MtimeDesc);
        assert_eq!(alts(&q), vec![group(&[&["coffee"]])]);
        assert!(matches!(q.mode, QueryMode::Substring(ref s) if s.is_empty()));
    }

    #[test]
    fn test_parse_tag_filter_detached_single() {
        let q = ParsedQuery::parse("t: coffee", SortOrder::MtimeDesc);
        assert_eq!(alts(&q), vec![group(&[&["coffee"]])]);
        assert!(matches!(q.mode, QueryMode::Substring(ref s) if s.is_empty()));
    }

    #[test]
    fn test_parse_tag_filter_detached_or_list() {
        // "t: coffee outdoor" - either tag is enough
        let q = ParsedQuery::parse("t: coffee outdoor", SortOrder::MtimeDesc);
        assert_eq!(alts(&q), vec![group(&[&["coffee"], &["outdoor"]])]);
        assert!(q.tags_match(&["coffee".into()]));
        assert!(q.tags_match(&["outdoor".into()]));
        assert!(q.tags_match(&["coffee".into(), "outdoor".into()]));
        assert!(!q.tags_match(&["dog".into()]));
    }

    #[test]
    fn test_parse_tag_filter_ampersand_forms_are_and() {
        for raw in ["t: coffee&outdoor", "t: coffee & outdoor", "t: coffee AND outdoor"] {
            let q = ParsedQuery::parse(raw, SortOrder::MtimeDesc);
            assert_eq!(alts(&q), vec![group(&[&["coffee", "outdoor"]])], "raw: {raw}");
            assert!(q.tags_match(&["coffee".into(), "outdoor".into()]), "raw: {raw}");
            assert!(!q.tags_match(&["coffee".into()]), "raw: {raw}");
        }
    }

    #[test]
    fn test_parse_tag_filter_and_binds_tighter_than_or() {
        // (a AND b) OR c
        let q = ParsedQuery::parse("t: a&b c", SortOrder::MtimeDesc);
        assert_eq!(alts(&q), vec![group(&[&["a", "b"], &["c"]])]);
        assert!(q.tags_match(&["a".into(), "b".into()]));
        assert!(q.tags_match(&["c".into()]));
        assert!(!q.tags_match(&["a".into()]));
    }

    #[test]
    fn test_parse_attached_tag_extends_via_connectives() {
        // No space after t: - explicit operators still join the group
        for raw in [
            "t:coffee AND outdoor",
            "t:coffee & outdoor",
            "t:coffee &outdoor",
            "t:coffee& outdoor",
            "t:coffee&outdoor",
        ] {
            let q = ParsedQuery::parse(raw, SortOrder::MtimeDesc);
            assert_eq!(alts(&q), vec![group(&[&["coffee", "outdoor"]])], "raw: {raw}");
            assert!(matches!(q.mode, QueryMode::Substring(ref s) if s.is_empty()), "raw: {raw}");
        }
        // OR connective on the attached form
        let q = ParsedQuery::parse("t:coffee OR outdoor", SortOrder::MtimeDesc);
        assert_eq!(alts(&q), vec![group(&[&["coffee"], &["outdoor"]])]);
        // Chaining
        let q = ParsedQuery::parse("t:a AND b AND c", SortOrder::MtimeDesc);
        assert_eq!(alts(&q), vec![group(&[&["a", "b", "c"]])]);
    }

    #[test]
    fn test_parse_attached_tag_plain_word_stays_text() {
        // Regression guard: without a connective, text after an attached
        // t:tag remains search text, not a tag.
        let q = ParsedQuery::parse("t:coffee patio", SortOrder::MtimeDesc);
        assert_eq!(alts(&q), vec![group(&[&["coffee"]])]);
        assert!(matches!(q.mode, QueryMode::Substring(ref s) if s == "patio"));
    }

    #[test]
    fn test_parse_attached_tag_connective_into_new_group() {
        // "AND t:x" starts its own group rather than swallowing it as a name
        let q = ParsedQuery::parse("t:coffee AND t:2024", SortOrder::MtimeDesc);
        assert_eq!(q.tag_filters.len(), 2);
        assert!(q.tags_match(&["coffee".into(), "2024".into()]));
        assert!(!q.tags_match(&["coffee".into()]));
    }

    #[test]
    fn test_parse_tag_filter_lowercase_and_is_a_tag_name() {
        // Only "&" and uppercase "AND" are operators; "and" stays a name.
        let q = ParsedQuery::parse("t: rock and roll", SortOrder::MtimeDesc);
        assert_eq!(alts(&q), vec![group(&[&["rock"], &["and"], &["roll"]])]);
    }

    #[test]
    fn test_parse_text_before_detached_tag_filter() {
        let q = ParsedQuery::parse("patio t: coffee outdoor", SortOrder::MtimeDesc);
        assert!(matches!(q.mode, QueryMode::Substring(ref s) if s == "patio"));
        assert_eq!(alts(&q), vec![group(&[&["coffee"], &["outdoor"]])]);
    }

    #[test]
    fn test_parse_extension_inside_tag_list() {
        let q = ParsedQuery::parse("t: coffee *.jpg outdoor", SortOrder::MtimeDesc);
        assert_eq!(q.extension_filter, Some("jpg".to_string()));
        assert_eq!(alts(&q), vec![group(&[&["coffee"], &["outdoor"]])]);
    }

    #[test]
    fn test_parse_multiple_tag_groups_are_anded() {
        let q = ParsedQuery::parse("t:coffee t:2024", SortOrder::MtimeDesc);
        assert_eq!(q.tag_filters.len(), 2);
        assert!(q.tags_match(&["coffee".into(), "2024".into(), "x".into()]));
        assert!(!q.tags_match(&["coffee".into()])); // AND: both groups must hold
    }

    #[test]
    fn test_parse_bare_trailing_tag_filter_means_any_tag() {
        let q = ParsedQuery::parse("t:", SortOrder::MtimeDesc);
        assert_eq!(alts(&q), vec![Vec::<Vec<String>>::new()]);
        assert!(q.tags_match(&["anything".into()]));
        assert!(!q.tags_match(&[]));
    }

    #[test]
    fn test_tags_match_case_insensitive_substring() {
        let q = ParsedQuery::parse("t:Head", SortOrder::MtimeDesc);
        assert!(q.tags_match(&["headshot".into()]));
        assert!(!q.tags_match(&["outdoor".into()]));
    }

    #[test]
    fn test_no_tag_filter_on_plain_query() {
        let q = ParsedQuery::parse("coffee", SortOrder::MtimeDesc);
        assert!(!q.has_tag_filter());
        // "toffee.txt" style names starting with t but no colon are untouched
        let q = ParsedQuery::parse("toffee", SortOrder::MtimeDesc);
        assert!(!q.has_tag_filter());
        assert!(matches!(q.mode, QueryMode::Substring(ref s) if s == "toffee"));
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
