use globset::Glob;
use regex::Regex;

#[derive(Debug, Clone)]
pub enum QueryMode {
    Substring(String),
    Regex(Regex),
    Glob(globset::GlobMatcher),
    /// Fuzzy matching with maximum edit distance
    Fuzzy {
        query: String,
        max_distance: usize,
    },
}

/// Calculate the maximum allowed edit distance based on query length.
fn max_distance_for_query(query_len: usize) -> usize {
    match query_len {
        0..=3 => 0,  // Too short for fuzzy - exact match only
        4..=6 => 1,  // 1 typo allowed
        7..=12 => 2, // 2 typos allowed
        _ => 3,      // 3 typos max
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
            curr[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(curr[j] + 1);
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

/// A half-open range `[start, end)` used by the `size:` and `dm:` filters;
/// a missing end is unbounded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Interval {
    pub start: Option<i64>,
    pub end: Option<i64>,
}

impl Interval {
    pub fn contains(&self, value: i64) -> bool {
        self.start.is_none_or(|s| value >= s) && self.end.is_none_or(|e| value < e)
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
    /// Further words a plain search requires, in any order (lowercase). The
    /// longest word is in `mode` instead, where it drives the trigram lookup.
    pub extra_terms: Vec<String>,
    /// Words written as `!word`: a match contains none of them (lowercase).
    pub excluded_terms: Vec<String>,
    /// `size:` filter in bytes; directories never match it.
    pub size_filter: Option<Interval>,
    /// `dm:` filter on the modification time, Unix seconds.
    pub mtime_filter: Option<Interval>,
}

/// One whitespace-separated piece of a query. Double quotes keep spaces inside
/// a single word (`"all that"`); a leading `!` marks a word to exclude.
struct Token {
    text: String,
    quoted: bool,
    negated: bool,
}

fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut text = String::new();
    let (mut started, mut quoted, mut negated, mut in_quote) = (false, false, false, false);
    for c in input.chars() {
        if in_quote {
            if c == '"' {
                in_quote = false;
            } else {
                text.push(c);
            }
            continue;
        }
        match c {
            '"' => {
                in_quote = true;
                quoted = true;
                started = true;
            }
            // Only a leading `!` negates; "wow!" stays a literal word
            '!' if !started => {
                negated = true;
                started = true;
            }
            c if c.is_whitespace() => {
                if started {
                    tokens.push(Token {
                        text: std::mem::take(&mut text),
                        quoted,
                        negated,
                    });
                }
                (started, quoted, negated) = (false, false, false);
            }
            c => {
                text.push(c);
                started = true;
            }
        }
    }
    if started {
        tokens.push(Token {
            text,
            quoted,
            negated,
        });
    }
    tokens
}

fn is_extension_token(token: &str) -> bool {
    token.starts_with("*.") && token.len() > 2
}

/// A `size:` or `dm:` token. The interval is None when the value does not
/// parse yet (mid-typing, such as `size:>`), so the filter is simply ignored.
enum FilterToken {
    Size(Option<Interval>),
    Modified(Option<Interval>),
}

fn filter_token(token: &str, now: i64) -> Option<FilterToken> {
    let (key, value) = token.split_once(':')?;
    match key.to_ascii_lowercase().as_str() {
        "size" => Some(FilterToken::Size(parse_comparison(value, parse_size_term))),
        "dm" | "datemodified" => Some(FilterToken::Modified(parse_comparison(value, |t| {
            parse_date_term(t, now)
        }))),
        _ => None,
    }
}

/// Apply a comparison to a value whose terms each name an interval:
/// `x` or `=x` is the interval itself, `>x` starts after it, `>=x` starts at
/// it, `<x` ends before it, `<=x` ends after it, and `a..b` spans from the
/// start of `a` to the end of `b`.
fn parse_comparison(value: &str, term: impl Fn(&str) -> Option<Interval>) -> Option<Interval> {
    let value = value.trim();
    if let Some((a, b)) = value.split_once("..") {
        return Some(Interval {
            start: term(a)?.start,
            end: term(b)?.end,
        });
    }
    let (op, rest) = [">=", "<=", ">", "<", "="]
        .iter()
        .find_map(|op| value.strip_prefix(op).map(|rest| (*op, rest)))
        .unwrap_or(("=", value));
    let t = term(rest)?;
    Some(match op {
        ">" => Interval {
            start: t.end,
            end: None,
        },
        ">=" => Interval {
            start: t.start,
            end: None,
        },
        "<" => Interval {
            start: None,
            end: t.start,
        },
        "<=" => Interval {
            start: None,
            end: t.end,
        },
        _ => t,
    })
}

/// A size such as `100`, `500k`, `1.5G`, or `2tb` (binary units, matching the
/// KiB/MiB/GiB shown in the results), as the one-byte interval it names.
fn parse_size_term(term: &str) -> Option<Interval> {
    let term = term.trim().to_ascii_lowercase();
    let split = term
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(term.len());
    let (number, unit) = term.split_at(split);
    let number: f64 = number.parse().ok()?;
    let shift = match unit {
        "" | "b" => 0,
        "k" | "kb" | "kib" => 10,
        "m" | "mb" | "mib" => 20,
        "g" | "gb" | "gib" => 30,
        "t" | "tb" | "tib" => 40,
        _ => return None,
    };
    let bytes = (number * (1u64 << shift) as f64).round() as i64;
    Some(Interval {
        start: Some(bytes),
        end: Some(bytes + 1),
    })
}

/// Local calendar fields of a Unix time.
fn local_tm(time: i64) -> libc::tm {
    let t = time as libc::time_t;
    // SAFETY: localtime_r only writes the tm it is given.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm
    }
}

/// Unix time of a local date and time. Out-of-range fields roll over the
/// way mktime normalizes them, so day 0 is the last day of the month before.
fn local_time(year: i32, month0: i32, day: i32, hour: i32, min: i32, sec: i32) -> i64 {
    // SAFETY: mktime reads and normalizes the tm it is given.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        tm.tm_year = year - 1900;
        tm.tm_mon = month0;
        tm.tm_mday = day;
        tm.tm_hour = hour;
        tm.tm_min = min;
        tm.tm_sec = sec;
        tm.tm_isdst = -1;
        libc::mktime(&mut tm) as i64
    }
}

fn day_span(year: i32, month0: i32, day: i32) -> Interval {
    Interval {
        start: Some(local_time(year, month0, day, 0, 0, 0)),
        end: Some(local_time(year, month0, day + 1, 0, 0, 0)),
    }
}

/// A date term as an interval of local time: `today`, `yesterday`,
/// `thisweek`/`lastweek` (weeks start on Monday), `thismonth`/`lastmonth`,
/// `thisyear`/`lastyear`, `lastNunits` such as `last7days` or `last24h`
/// (up to `now`), or a calendar date `2026`, `2026-10`, `2026-10-29`.
fn parse_date_term(term: &str, now: i64) -> Option<Interval> {
    let term = term.trim().to_ascii_lowercase();
    let tm = local_tm(now);
    let (y, m, d) = (tm.tm_year + 1900, tm.tm_mon, tm.tm_mday);
    let week_start = d - (tm.tm_wday + 6) % 7;
    let span = |start: i64, end: i64| {
        Some(Interval {
            start: Some(start),
            end: Some(end),
        })
    };
    match term.as_str() {
        "today" => return Some(day_span(y, m, d)),
        "yesterday" => return Some(day_span(y, m, d - 1)),
        "thisweek" => {
            return span(
                local_time(y, m, week_start, 0, 0, 0),
                local_time(y, m, week_start + 7, 0, 0, 0),
            )
        }
        "lastweek" => {
            return span(
                local_time(y, m, week_start - 7, 0, 0, 0),
                local_time(y, m, week_start, 0, 0, 0),
            )
        }
        "thismonth" => {
            return span(
                local_time(y, m, 1, 0, 0, 0),
                local_time(y, m + 1, 1, 0, 0, 0),
            )
        }
        "lastmonth" => {
            return span(
                local_time(y, m - 1, 1, 0, 0, 0),
                local_time(y, m, 1, 0, 0, 0),
            )
        }
        "thisyear" => {
            return span(
                local_time(y, 0, 1, 0, 0, 0),
                local_time(y + 1, 0, 1, 0, 0, 0),
            )
        }
        "lastyear" => {
            return span(
                local_time(y - 1, 0, 1, 0, 0, 0),
                local_time(y, 0, 1, 0, 0, 0),
            )
        }
        _ => {}
    }

    if let Some(rest) = term.strip_prefix("last") {
        let split = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        let (count, unit) = rest.split_at(split);
        let n: i32 = count.parse().ok()?;
        let (h, mi, s) = (tm.tm_hour, tm.tm_min, tm.tm_sec);
        let start = match unit {
            "h" | "hour" | "hours" => now - i64::from(n) * 3600,
            "d" | "day" | "days" => local_time(y, m, d - n, h, mi, s),
            "w" | "week" | "weeks" => local_time(y, m, d - 7 * n, h, mi, s),
            "month" | "months" => local_time(y, m - n, d, h, mi, s),
            "y" | "year" | "years" => local_time(y - n, m, d, h, mi, s),
            _ => return None,
        };
        return Some(Interval {
            start: Some(start),
            end: None,
        });
    }

    let parts: Vec<&str> = term.split('-').collect();
    let num = |i: usize| parts.get(i).and_then(|p| p.parse::<i32>().ok());
    let year = num(0).filter(|y| (1900..=9999).contains(y) && parts[0].len() == 4)?;
    match parts.len() {
        1 => span(
            local_time(year, 0, 1, 0, 0, 0),
            local_time(year + 1, 0, 1, 0, 0, 0),
        ),
        2 => {
            let month = num(1).filter(|m| (1..=12).contains(m))?;
            span(
                local_time(year, month - 1, 1, 0, 0, 0),
                local_time(year, month, 1, 0, 0, 0),
            )
        }
        3 => {
            let month = num(1).filter(|m| (1..=12).contains(m))?;
            let day = num(2).filter(|d| (1..=31).contains(d))?;
            Some(day_span(year, month - 1, day))
        }
        _ => None,
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl ParsedQuery {
    /// Check a cached result against this query: the text match plus the
    /// extension, size, and date filters.
    pub fn matches_result(&self, path: &str, is_dir: bool, size: u64, mtime: i64) -> bool {
        self.meta_matches(is_dir, size, mtime) && self.matches_path(path)
    }

    /// The `size:` and `dm:` filters. Directories have no indexed size, so a
    /// size filter never matches one.
    pub fn meta_matches(&self, is_dir: bool, size: u64, mtime: i64) -> bool {
        if let Some(range) = self.size_filter {
            if is_dir || !range.contains(size as i64) {
                return false;
            }
        }
        self.mtime_filter.is_none_or(|range| range.contains(mtime))
    }

    /// Check if a path matches this query (for cache filtering).
    pub fn matches_path(&self, path: &str) -> bool {
        // Check extension filter first
        if let Some(ref ext_filter) = self.extension_filter {
            if let Some(ext) = std::path::Path::new(path)
                .extension()
                .and_then(|e| e.to_str())
            {
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
        let matched = match &self.mode {
            QueryMode::Substring(_) => self.substring_matches(path),
            QueryMode::Regex(re) => re.is_match(path),
            QueryMode::Glob(matcher) => {
                let filename = std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path);
                matcher.is_match(filename) || matcher.is_match(path)
            }
            QueryMode::Fuzzy {
                query,
                max_distance,
            } => {
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
        };
        matched && !self.is_excluded(path)
    }

    /// The words a plain search requires: the one in `mode` and `extra_terms`.
    pub fn search_words(&self) -> impl Iterator<Item = &str> {
        let primary = match &self.mode {
            QueryMode::Substring(word) if !word.is_empty() => Some(word.as_str()),
            _ => None,
        };
        primary
            .into_iter()
            .chain(self.extra_terms.iter().map(String::as_str))
    }

    /// Whether `path` holds every search word, in any order (case-insensitive,
    /// against the full path, or the folder name for `folder:`).
    pub fn substring_matches(&self, path: &str) -> bool {
        let mut words = self.search_words().peekable();
        if words.peek().is_none() {
            return true;
        }
        let haystack = self.substring_haystack(path).to_lowercase();
        words.all(|word| haystack.contains(word))
    }

    /// Whether `path` holds any `!word` of the query.
    pub fn is_excluded(&self, path: &str) -> bool {
        if self.excluded_terms.is_empty() {
            return false;
        }
        let haystack = self.substring_haystack(path).to_lowercase();
        self.excluded_terms
            .iter()
            .any(|word| haystack.contains(word.as_str()))
    }

    /// Whether every match of `self` is also a match of `old`, so `old`'s
    /// complete results can be filtered instead of searching again. Holds for
    /// plain searches when each old word sits inside some new word, each old
    /// `!word` contains some new one (excluding "ba" drops fewer paths than
    /// excluding "b"), and every filter is unchanged.
    pub fn narrows(&self, old: &ParsedQuery) -> bool {
        if self.file_type_mode != old.file_type_mode
            || self.extension_filter != old.extension_filter
            || self.path_segments != old.path_segments
            || self.size_filter != old.size_filter
            || self.mtime_filter != old.mtime_filter
        {
            return false;
        }
        if !matches!(
            (&self.mode, &old.mode),
            (QueryMode::Substring(_), QueryMode::Substring(_))
        ) {
            return false;
        }
        let new_words: Vec<&str> = self.search_words().collect();
        old.search_words()
            .all(|o| new_words.iter().any(|n| n.contains(o)))
            && old
                .excluded_terms
                .iter()
                .all(|e| self.excluded_terms.iter().any(|n| e.contains(n.as_str())))
    }

    /// Parse a raw query string.
    ///
    /// Syntax:
    ///   "left all"             - every word, in any order, anywhere in the path
    ///   "\"all that\""         - quotes keep a phrase together
    ///   "movies !trailer"      - `!word` excludes paths containing the word
    ///   "*.py query"           - filter by extension (position-independent)
    ///   "size:>1G"             - size filter: >, >=, <, <=, =, or a..b; k/m/g/t units
    ///   "dm:thisweek"          - modified date: today, yesterday, this/last week,
    ///                            month, year, lastNdays, or 2026 / 2026-10 / 2026-10-29
    ///   "/regex"               - regex search (prefix with /)
    ///   "foo*bar"              - glob pattern (contains * or ?)
    ///   "~confg"               - fuzzy match on the file name
    ///   "src/config"           - path-aware search (matches "config" under "src")
    ///   "folder:movies"        - directories whose own name matches (not nested children)
    pub fn parse(raw: &str, sort_order: SortOrder) -> Self {
        Self::parse_at(raw, sort_order, now_secs())
    }

    /// `parse` with the current time given, so relative dates are testable.
    pub fn parse_at(raw: &str, sort_order: SortOrder, now: i64) -> Self {
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
            (
                FileTypeMode::GotoDir,
                raw_trimmed[FOLDER_PREFIX_LEN..].trim_start(),
            )
        } else {
            (FileTypeMode::All, raw_trimmed)
        };

        let mut query = Self {
            mode: QueryMode::Substring(String::new()),
            extension_filter: None,
            file_type_mode,
            sort_order,
            path_segments: None,
            extra_terms: Vec::new(),
            excluded_terms: Vec::new(),
            size_filter: None,
            mtime_filter: None,
        };

        // A regex keeps everything after "/" as its pattern (only *.ext tokens
        // are pulled out), so `!`, quotes, and `size:` inside it stay literal.
        let is_regex = raw_effective
            .split_whitespace()
            .find(|t| !is_extension_token(t))
            .is_some_and(|t| t.starts_with('/'));
        if is_regex {
            let mut parts: Vec<&str> = Vec::new();
            for token in raw_effective.split_whitespace() {
                if is_extension_token(token) {
                    query.extension_filter = Some(token[2..].to_string());
                } else {
                    parts.push(token);
                }
            }
            let text = parts.join(" ");
            query.mode = match Regex::new(&format!("(?i){}", &text[1..])) {
                Ok(re) => QueryMode::Regex(re),
                Err(_) => QueryMode::Substring(text.to_lowercase()),
            };
            return query;
        }

        let mut words: Vec<Token> = Vec::new();
        for token in tokenize(raw_effective) {
            let plain = !token.quoted && !token.negated;
            if plain && is_extension_token(&token.text) {
                query.extension_filter = Some(token.text[2..].to_string());
            } else if let Some(filter) = filter_token(&token.text, now).filter(|_| plain) {
                match filter {
                    FilterToken::Size(range) => query.size_filter = range,
                    FilterToken::Modified(range) => query.mtime_filter = range,
                }
            } else if token.negated {
                if !token.text.is_empty() {
                    query.excluded_terms.push(token.text.to_lowercase());
                }
            } else if !token.text.is_empty() {
                words.push(token);
            }
        }

        let text = words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let any_quoted = words.iter().any(|w| w.quoted);

        // Check for fuzzy search: starts with "~"
        if !any_quoted && text.starts_with('~') && text.len() > 1 {
            let fuzzy_query = text[1..].to_string();
            let max_distance = max_distance_for_query(fuzzy_query.len());
            query.mode = QueryMode::Fuzzy {
                query: fuzzy_query,
                max_distance,
            };
            return query;
        }

        // Check for path-aware search: contains "/" and no glob wildcards
        let is_path_query =
            !any_quoted && text.contains('/') && !text.contains('*') && !text.contains('?');
        if is_path_query {
            let segments: Vec<String> = text
                .split('/')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            if segments.len() >= 2 {
                // Use last segment as the search term for trigram lookup
                query.mode = QueryMode::Substring(segments.last().unwrap().to_lowercase());
                query.path_segments = Some(segments);
                return query;
            } else if segments.len() == 1 {
                // "src/" with trailing slash - treat as normal search for "src"
                query.mode = QueryMode::Substring(segments[0].to_lowercase());
                return query;
            }
        }

        // Glob mode: unquoted text with * or ?
        if !any_quoted && (text.contains('*') || text.contains('?')) {
            query.mode = match Glob::new(&format!("*{}*", text)) {
                Ok(glob) => QueryMode::Glob(glob.compile_matcher()),
                Err(_) => QueryMode::Substring(text.to_lowercase()),
            };
            return query;
        }

        // Plain words: all of them, in any order. The longest one has the
        // most trigrams, so it leads the candidate lookup.
        let mut terms: Vec<String> = words.into_iter().map(|w| w.text.to_lowercase()).collect();
        if let Some(longest) = (0..terms.len()).max_by_key(|&i| terms[i].chars().count()) {
            query.mode = QueryMode::Substring(terms.remove(longest));
            query.extra_terms = terms;
        }
        query
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

        // Extension filter in middle; the words around it are both required
        let q = ParsedQuery::parse("foo *.txt bar", SortOrder::MtimeDesc);
        let mut words: Vec<&str> = q.search_words().collect();
        words.sort();
        assert_eq!(words, ["bar", "foo"]);
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
        assert!(
            matches!(q.mode, QueryMode::Fuzzy { ref query, max_distance: 1 } if query == "confg")
        );
    }

    #[test]
    fn test_parse_path_aware() {
        let q = ParsedQuery::parse("src/config", SortOrder::MtimeDesc);
        assert!(matches!(q.mode, QueryMode::Substring(ref s) if s == "config"));
        assert_eq!(
            q.path_segments,
            Some(vec!["src".to_string(), "config".to_string()])
        );
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
        // Words are stored lowercase; matching ignores case either way
        assert!(matches!(q.mode, QueryMode::Substring(ref s) if s == "movies"));
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
        for q in [
            "éééé",
            "ééééééé",
            "日本語データ",
            "héllo wörld",
            "folder:éé",
        ] {
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

    // Fixed reference time for date tests: local noon on Wednesday 2026-10-28
    fn noon_oct_28() -> i64 {
        local_time(2026, 9, 28, 12, 0, 0)
    }

    fn at(raw: &str) -> ParsedQuery {
        ParsedQuery::parse_at(raw, SortOrder::MtimeDesc, noon_oct_28())
    }

    #[test]
    fn words_match_in_any_order() {
        let path = "/mnt/nas/movies/All That's Left of You (2025)/movie.mkv";
        assert!(at("left all").matches_path(path));
        assert!(at("you 2025 ALL").matches_path(path));
        assert!(!at("left nothing").matches_path(path));
        // The longest word leads the trigram lookup
        assert!(matches!(at("ab movies").mode, QueryMode::Substring(ref s) if s == "movies"));
    }

    #[test]
    fn quotes_keep_a_phrase_together() {
        let path = "/v/All That's Left of You.mkv";
        assert!(at("\"all that's\"").matches_path(path));
        assert!(!at("\"that's all\"").matches_path(path));
        // An unfinished quote still searches what was typed
        assert!(at("\"all tha").matches_path(path));
    }

    #[test]
    fn bang_words_exclude_paths() {
        let q = at("movies !trailer");
        assert_eq!(q.excluded_terms, ["trailer"]);
        assert!(q.matches_path("/v/movies/film.mkv"));
        assert!(!q.matches_path("/v/movies/Film Trailer.mkv"));
        // A lone "!" (mid-typing) and a "!" inside a word are not exclusions
        assert!(at("movies !").excluded_terms.is_empty());
        let q = at("wow!");
        assert!(q.excluded_terms.is_empty());
        assert!(matches!(q.mode, QueryMode::Substring(ref s) if s == "wow!"));
        // Quoted exclusions keep their spaces
        assert_eq!(at("x !\"bonus disc\"").excluded_terms, ["bonus disc"]);
    }

    #[test]
    fn regex_queries_keep_bangs_quotes_and_filters_literal() {
        let q = at("/a!b \"c\" size:>1");
        assert!(matches!(q.mode, QueryMode::Regex(_)));
        assert!(q.excluded_terms.is_empty() && q.size_filter.is_none());
        // An extension filter before the pattern still applies
        let q = at("*.rs /^main");
        assert!(matches!(q.mode, QueryMode::Regex(_)));
        assert_eq!(q.extension_filter, Some("rs".to_string()));
    }

    #[test]
    fn size_filter_forms() {
        let size = |raw: &str| at(raw).size_filter;
        let g = 1i64 << 30;
        assert_eq!(
            size("size:>1G"),
            Some(Interval {
                start: Some(g + 1),
                end: None
            })
        );
        assert_eq!(
            size("size:>=1g"),
            Some(Interval {
                start: Some(g),
                end: None
            })
        );
        assert_eq!(
            size("size:<500k"),
            Some(Interval {
                start: None,
                end: Some(500 * 1024)
            })
        );
        assert_eq!(
            size("size:<=2mb"),
            Some(Interval {
                start: None,
                end: Some(2 * (1 << 20) + 1)
            })
        );
        assert_eq!(
            size("size:100"),
            Some(Interval {
                start: Some(100),
                end: Some(101)
            })
        );
        assert_eq!(
            size("size:100M..1G"),
            Some(Interval {
                start: Some(100 << 20),
                end: Some(g + 1)
            })
        );
        assert_eq!(
            size("size:1.5G"),
            Some(Interval {
                start: Some(g * 3 / 2),
                end: Some(g * 3 / 2 + 1)
            })
        );
        // Incomplete or unknown values are ignored rather than searched as text
        let q = at("size:> movies");
        assert!(q.size_filter.is_none());
        assert!(matches!(q.mode, QueryMode::Substring(ref s) if s == "movies"));
        assert!(at("size:12parsecs").size_filter.is_none());
    }

    #[test]
    fn size_filter_skips_directories() {
        let q = at("size:<1k");
        assert!(q.meta_matches(false, 10, 0));
        assert!(!q.meta_matches(false, 4096, 0));
        assert!(!q.meta_matches(true, 0, 0), "folders have no indexed size");
    }

    #[test]
    fn date_filter_calendar_terms() {
        let dm = |raw: &str| at(raw).mtime_filter.unwrap();
        let day = |m0, d| Interval {
            start: Some(local_time(2026, m0, d, 0, 0, 0)),
            end: Some(local_time(2026, m0, d + 1, 0, 0, 0)),
        };
        assert_eq!(dm("dm:2026-10-29"), day(9, 29));
        assert_eq!(dm("dm:today"), day(9, 28));
        assert_eq!(dm("datemodified:yesterday"), day(9, 27));
        assert_eq!(
            dm("dm:2026-10"),
            Interval {
                start: Some(local_time(2026, 9, 1, 0, 0, 0)),
                end: Some(local_time(2026, 10, 1, 0, 0, 0))
            }
        );
        // > means after the whole year, < before it begins
        assert_eq!(dm("dm:>2025").start, Some(local_time(2026, 0, 1, 0, 0, 0)));
        assert_eq!(dm("dm:<2026").end, Some(local_time(2026, 0, 1, 0, 0, 0)));
        assert_eq!(
            dm("dm:2026-01..2026-03"),
            Interval {
                start: Some(local_time(2026, 0, 1, 0, 0, 0)),
                end: Some(local_time(2026, 3, 1, 0, 0, 0))
            }
        );
        assert!(at("dm:2026-13").mtime_filter.is_none());
    }

    #[test]
    fn date_filter_relative_terms() {
        let dm = |raw: &str| at(raw).mtime_filter.unwrap();
        // 2026-10-28 is a Wednesday; weeks start on Monday the 26th
        assert_eq!(
            dm("dm:thisweek"),
            Interval {
                start: Some(local_time(2026, 9, 26, 0, 0, 0)),
                end: Some(local_time(2026, 10, 2, 0, 0, 0))
            }
        );
        assert_eq!(
            dm("dm:lastweek").start,
            Some(local_time(2026, 9, 19, 0, 0, 0))
        );
        assert_eq!(
            dm("dm:thismonth").start,
            Some(local_time(2026, 9, 1, 0, 0, 0))
        );
        assert_eq!(
            dm("dm:lastyear").start,
            Some(local_time(2025, 0, 1, 0, 0, 0))
        );
        assert_eq!(
            dm("dm:last7days"),
            Interval {
                start: Some(local_time(2026, 9, 21, 12, 0, 0)),
                end: None
            }
        );
        assert_eq!(dm("dm:last24h").start, Some(noon_oct_28() - 24 * 3600));
        assert!(at("dm:last7fortnights").mtime_filter.is_none());
    }

    #[test]
    fn narrowing_rules_for_the_result_cache() {
        let narrows = |new: &str, old: &str| at(new).narrows(&at(old));
        assert!(narrows("conf", "con"));
        assert!(narrows("left all", "all"), "an added word removes results");
        assert!(
            narrows("foo !b", "foo"),
            "an added exclusion removes results"
        );
        assert!(
            narrows("foo !b", "foo !ba"),
            "a shorter exclusion removes more"
        );
        assert!(
            !narrows("foo !ba", "foo !b"),
            "a longer exclusion brings results back"
        );
        assert!(!narrows("foo", "foo bar"), "dropping a word widens");
        assert!(
            !narrows("foo size:>1G", "foo"),
            "a new filter is not a refinement"
        );
        assert!(!narrows("foo*", "foo"), "glob does not refine");
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
