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
}

pub struct Matcher {
    ac: AhoCorasick,
}

impl Matcher {
    pub fn new(patterns: &[&str]) -> Result<Self, crate::Error> {
        Self::with_options(patterns, MatchOptions::default())
    }

    pub fn with_options(patterns: &[&str], opts: MatchOptions) -> Result<Self, crate::Error> {
        let ac = AhoCorasickBuilder::new()
            .ascii_case_insensitive(opts.case_insensitive)
            .match_kind(MatchKind::Standard)
            .build(patterns)
            .map_err(|e| crate::Error::Search(e.to_string()))?;
        Ok(Self { ac })
    }

    pub fn find_all(&self, haystack: &[u8]) -> Vec<Match> {
        self.ac
            .find_iter(haystack)
            .map(|m| Match {
                offset: m.start(),
                pattern_index: m.pattern().as_usize(),
                length: m.end() - m.start(),
            })
            .collect()
    }

    pub fn is_match(&self, haystack: &[u8]) -> bool {
        self.ac.is_match(haystack)
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
}
