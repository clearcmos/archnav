//! Marks where a query matched inside a result's file name, for the window
//! to show in bold.

use regex::Regex;

use super::query::{ParsedQuery, QueryMode};

/// What to look for in a file name.
#[derive(Debug, Clone)]
pub enum Highlight {
    /// The words of a plain search (lowercase), matched case-insensitively.
    Words(Vec<String>),
    /// A `/regex` search, run on the full path as the search does.
    Regex(Regex),
}

impl Highlight {
    /// The highlight for `query`, or None for searches whose matches do not
    /// map onto the name (glob, fuzzy) or that have no words.
    pub fn for_query(query: &ParsedQuery) -> Option<Self> {
        match &query.mode {
            QueryMode::Regex(re) => Some(Self::Regex(re.clone())),
            QueryMode::Substring(_) => {
                let words: Vec<String> = query
                    .search_words()
                    .filter(|w| !w.is_empty())
                    .map(str::to_string)
                    .collect();
                (!words.is_empty()).then_some(Self::Words(words))
            }
            QueryMode::Glob(_) | QueryMode::Fuzzy { .. } => None,
        }
    }

    /// The file name of `path` as Qt StyledText with each match in bold, or
    /// None when nothing in the name matched.
    pub fn markup(&self, path: &str) -> Option<String> {
        let name_start = path.rfind('/').map_or(0, |i| i + 1);
        let name = &path[name_start..];
        let mut marked = vec![false; name.len()];
        match self {
            Self::Words(words) => {
                let (lower, origin) = lowercase_with_origin(name);
                for word in words {
                    for (start, _) in lower.match_indices(word.as_str()) {
                        let end = start + word.len();
                        for &byte in &origin[start..end] {
                            marked[byte] = true;
                        }
                    }
                }
            }
            Self::Regex(re) => {
                for m in re.find_iter(path) {
                    let start = m.start().max(name_start) - name_start;
                    let end = m.end().saturating_sub(name_start);
                    for flag in marked.iter_mut().take(end).skip(start) {
                        *flag = true;
                    }
                }
            }
        }
        marked.contains(&true).then(|| styled(name, &marked))
    }
}

/// `s` lowercased, and for each byte of the result the byte of `s` it came
/// from, since lowercasing can change a character's length.
fn lowercase_with_origin(s: &str) -> (String, Vec<usize>) {
    let mut lower = String::with_capacity(s.len());
    let mut origin = Vec::with_capacity(s.len());
    for (i, c) in s.char_indices() {
        for l in c.to_lowercase() {
            for _ in 0..l.len_utf8() {
                origin.push(i);
            }
            lower.push(l);
        }
    }
    (lower, origin)
}

/// `name` as StyledText, bold where `marked` (one flag per byte) is set.
fn styled(name: &str, marked: &[bool]) -> String {
    let mut out = String::with_capacity(name.len() + 16);
    let mut bold = false;
    for (i, c) in name.char_indices() {
        if marked[i] != bold {
            out.push_str(if marked[i] { "<b>" } else { "</b>" });
            bold = marked[i];
        }
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    if bold {
        out.push_str("</b>");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::query::SortOrder;

    fn markup(query: &str, path: &str) -> Option<String> {
        Highlight::for_query(&ParsedQuery::parse(query, SortOrder::MtimeDesc))?.markup(path)
    }

    #[test]
    fn words_are_bold_in_the_name_in_any_case() {
        assert_eq!(
            markup("left all", "/m/All That's Left of You.mkv").as_deref(),
            Some("<b>All</b> That's <b>Left</b> of You.mkv")
        );
        // Every occurrence, and overlapping words merge into one run
        assert_eq!(
            markup("con conf", "/x/config.conf").as_deref(),
            Some("<b>conf</b>ig.<b>conf</b>")
        );
    }

    #[test]
    fn matches_only_in_folders_leave_the_name_plain() {
        assert_eq!(markup("music", "/music/song.flac"), None);
    }

    #[test]
    fn markup_characters_are_escaped() {
        assert_eq!(
            markup("b", "/d/a<b>&c.txt").as_deref(),
            Some("a&lt;<b>b</b>&gt;&amp;c.txt")
        );
    }

    #[test]
    fn non_ascii_case_folding_keeps_positions() {
        // 'İ' lowercases to two characters; the match after it must still
        // land on the right bytes
        assert_eq!(
            markup("tea", "/d/İstanbul tea.txt").as_deref(),
            Some("İstanbul <b>tea</b>.txt")
        );
        assert_eq!(markup("été", "/d/Été.md").as_deref(), Some("<b>Été</b>.md"));
    }

    #[test]
    fn regex_matches_are_clipped_to_the_name() {
        assert_eq!(
            markup("/docs/read", "/home/docs/readme.md").as_deref(),
            Some("<b>read</b>me.md")
        );
        assert_eq!(markup("/^/home", "/home/readme.md"), None);
    }

    #[test]
    fn exclusions_and_filters_alone_highlight_nothing() {
        assert!(
            Highlight::for_query(&ParsedQuery::parse("!trailer", SortOrder::MtimeDesc)).is_none()
        );
        assert!(
            Highlight::for_query(&ParsedQuery::parse("size:>1G", SortOrder::MtimeDesc)).is_none()
        );
        assert!(
            Highlight::for_query(&ParsedQuery::parse("~confg", SortOrder::MtimeDesc)).is_none()
        );
        assert_eq!(
            markup("movies !trailer", "/m/Movies.txt").as_deref(),
            Some("<b>Movies</b>.txt")
        );
    }
}
