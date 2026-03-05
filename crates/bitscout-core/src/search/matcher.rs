use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};

#[derive(Debug, Clone)]
pub struct Match {
    pub offset: usize,
    pub pattern_index: usize,
    pub length: usize,
}

#[derive(Debug, Clone, Default)]
pub struct MatchOptions {
    pub case_insensitive: bool,
    pub use_regex: bool,
}

enum MatcherKind {
    Literal(AhoCorasick),
    Regex(regex::Regex),
}

pub struct Matcher {
    kind: MatcherKind,
}

impl Matcher {
    pub fn new(patterns: &[&str]) -> Result<Self, crate::Error> {
        Self::with_options(patterns, MatchOptions::default())
    }

    pub fn with_options(patterns: &[&str], opts: MatchOptions) -> Result<Self, crate::Error> {
        if opts.use_regex {
            // For regex mode, join multiple patterns with alternation
            let combined = if patterns.len() == 1 {
                patterns[0].to_string()
            } else {
                patterns.iter().map(|p| format!("(?:{})", p)).collect::<Vec<_>>().join("|")
            };
            let re = regex::RegexBuilder::new(&combined)
                .case_insensitive(opts.case_insensitive)
                .build()
                .map_err(|e| crate::Error::Search(e.to_string()))?;
            Ok(Self { kind: MatcherKind::Regex(re) })
        } else {
            let ac = AhoCorasickBuilder::new()
                .ascii_case_insensitive(opts.case_insensitive)
                .match_kind(MatchKind::Standard)
                .build(patterns)
                .map_err(|e| crate::Error::Search(e.to_string()))?;
            Ok(Self { kind: MatcherKind::Literal(ac) })
        }
    }

    pub fn find_all(&self, haystack: &[u8]) -> Vec<Match> {
        match &self.kind {
            MatcherKind::Literal(ac) => {
                ac.find_iter(haystack)
                    .map(|m| Match {
                        offset: m.start(),
                        pattern_index: m.pattern().as_usize(),
                        length: m.end() - m.start(),
                    })
                    .collect()
            }
            MatcherKind::Regex(re) => {
                let text = String::from_utf8_lossy(haystack);
                re.find_iter(&text)
                    .map(|m| Match {
                        offset: m.start(),
                        pattern_index: 0,
                        length: m.end() - m.start(),
                    })
                    .collect()
            }
        }
    }

    pub fn is_match(&self, haystack: &[u8]) -> bool {
        match &self.kind {
            MatcherKind::Literal(ac) => ac.is_match(haystack),
            MatcherKind::Regex(re) => {
                let text = String::from_utf8_lossy(haystack);
                re.is_match(&text)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_pattern_literal() {
        let matcher = Matcher::new(&["hello"]).unwrap();
        let matches = matcher.find_all(b"say hello world");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].offset, 4);
    }

    #[test]
    fn test_multi_pattern() {
        let matcher = Matcher::new(&["login", "auth", "session"]).unwrap();
        let matches = matcher.find_all(b"login requires auth and a valid session");
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn test_case_insensitive() {
        let opts = MatchOptions {
            case_insensitive: true,
            ..Default::default()
        };
        let matcher = Matcher::with_options(&["Hello"], opts).unwrap();
        let matches = matcher.find_all(b"HELLO hello Hello");
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn test_no_match() {
        let matcher = Matcher::new(&["xyz"]).unwrap();
        let matches = matcher.find_all(b"nothing here");
        assert!(matches.is_empty());
    }

    #[test]
    fn test_regex_whitespace_pattern() {
        let opts = MatchOptions { use_regex: true, ..Default::default() };
        let matcher = Matcher::with_options(&[r"fn\s+\w+"], opts).unwrap();
        assert!(matcher.is_match(b"fn  hello_world"));
        assert!(matcher.is_match(b"pub fn foo()"));
        assert!(!matcher.is_match(b"fnhello"));
    }

    #[test]
    fn test_regex_alternation() {
        let opts = MatchOptions { use_regex: true, ..Default::default() };
        let matcher = Matcher::with_options(&["TODO|FIXME"], opts).unwrap();
        assert!(matcher.is_match(b"// TODO: fix this"));
        assert!(matcher.is_match(b"// FIXME: broken"));
        assert!(!matcher.is_match(b"// NOTE: ok"));
    }

    #[test]
    fn test_regex_digits() {
        let opts = MatchOptions { use_regex: true, ..Default::default() };
        let matcher = Matcher::with_options(&[r"\d+"], opts).unwrap();
        let matches = matcher.find_all(b"abc 123 def 456");
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn test_regex_word_boundary() {
        let opts = MatchOptions { use_regex: true, ..Default::default() };
        let matcher = Matcher::with_options(&[r"\bfoo\b"], opts).unwrap();
        assert!(matcher.is_match(b"foo bar"));
        assert!(matcher.is_match(b"a foo b"));
        assert!(!matcher.is_match(b"foobar"));
        assert!(!matcher.is_match(b"barfoo2"));
    }

    #[test]
    fn test_regex_case_insensitive() {
        let opts = MatchOptions { use_regex: true, case_insensitive: true };
        let matcher = Matcher::with_options(&[r"hello\s+world"], opts).unwrap();
        assert!(matcher.is_match(b"HELLO  WORLD"));
        assert!(matcher.is_match(b"Hello World"));
    }
}
