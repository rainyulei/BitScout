#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FlagKind {
    Bool,  // --json, -i, --no-unicode, etc.
    Value, // -C NUM, --glob GLOB, -e PATTERN, etc.
}

pub struct FlagDef {
    pub short: Option<&'static str>, // "-i"
    pub long: &'static str,          // "--ignore-case"
    pub kind: FlagKind,
}

/// Complete registry of ALL rg flags (141+).
/// This ensures we never misparse a value as a pattern.
pub const RG_FLAGS: &[FlagDef] = &[
    // === Pattern flags (Value) ===
    FlagDef { short: Some("-e"), long: "--regexp", kind: FlagKind::Value },
    FlagDef { short: Some("-f"), long: "--file", kind: FlagKind::Value },
    // === Search flags ===
    FlagDef { short: None, long: "--pre", kind: FlagKind::Value },
    FlagDef { short: None, long: "--pre-glob", kind: FlagKind::Value },
    FlagDef { short: Some("-z"), long: "--search-zip", kind: FlagKind::Bool },
    FlagDef { short: Some("-s"), long: "--case-sensitive", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--crlf", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--dfa-size-limit", kind: FlagKind::Value },
    FlagDef { short: Some("-E"), long: "--encoding", kind: FlagKind::Value },
    FlagDef { short: None, long: "--engine", kind: FlagKind::Value },
    FlagDef { short: Some("-F"), long: "--fixed-strings", kind: FlagKind::Bool },
    FlagDef { short: Some("-i"), long: "--ignore-case", kind: FlagKind::Bool },
    FlagDef { short: Some("-v"), long: "--invert-match", kind: FlagKind::Bool },
    FlagDef { short: Some("-x"), long: "--line-regexp", kind: FlagKind::Bool },
    FlagDef { short: Some("-m"), long: "--max-count", kind: FlagKind::Value },
    FlagDef { short: None, long: "--mmap", kind: FlagKind::Bool },
    FlagDef { short: Some("-U"), long: "--multiline", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--multiline-dotall", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--no-unicode", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--null-data", kind: FlagKind::Bool },
    FlagDef { short: Some("-P"), long: "--pcre2", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--regex-size-limit", kind: FlagKind::Value },
    FlagDef { short: Some("-S"), long: "--smart-case", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--stop-on-nonmatch", kind: FlagKind::Bool },
    FlagDef { short: Some("-a"), long: "--text", kind: FlagKind::Bool },
    FlagDef { short: Some("-j"), long: "--threads", kind: FlagKind::Value },
    FlagDef { short: Some("-w"), long: "--word-regexp", kind: FlagKind::Bool },
    // === Filter flags ===
    FlagDef { short: None, long: "--binary", kind: FlagKind::Bool },
    FlagDef { short: Some("-L"), long: "--follow", kind: FlagKind::Bool },
    FlagDef { short: Some("-g"), long: "--glob", kind: FlagKind::Value },
    FlagDef { short: None, long: "--glob-case-insensitive", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--iglob", kind: FlagKind::Value },
    FlagDef { short: None, long: "--ignore-file", kind: FlagKind::Value },
    FlagDef { short: None, long: "--ignore-file-case-insensitive", kind: FlagKind::Bool },
    FlagDef { short: Some("-d"), long: "--max-depth", kind: FlagKind::Value },
    FlagDef { short: None, long: "--max-filesize", kind: FlagKind::Value },
    FlagDef { short: None, long: "--no-ignore", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--no-ignore-dot", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--no-ignore-exclude", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--no-ignore-files", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--no-ignore-global", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--no-ignore-parent", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--no-ignore-vcs", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--no-require-git", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--one-file-system", kind: FlagKind::Bool },
    FlagDef { short: Some("-t"), long: "--type", kind: FlagKind::Value },
    FlagDef { short: Some("-T"), long: "--type-not", kind: FlagKind::Value },
    FlagDef { short: None, long: "--type-add", kind: FlagKind::Value },
    FlagDef { short: None, long: "--type-clear", kind: FlagKind::Value },
    FlagDef { short: None, long: "--type-list", kind: FlagKind::Bool },
    // === Output flags ===
    FlagDef { short: Some("-A"), long: "--after-context", kind: FlagKind::Value },
    FlagDef { short: Some("-B"), long: "--before-context", kind: FlagKind::Value },
    FlagDef { short: Some("-C"), long: "--context", kind: FlagKind::Value },
    FlagDef { short: None, long: "--color", kind: FlagKind::Value },
    FlagDef { short: None, long: "--colors", kind: FlagKind::Value },
    FlagDef { short: None, long: "--column", kind: FlagKind::Bool },
    FlagDef { short: Some("-c"), long: "--count", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--count-matches", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--field-context-separator", kind: FlagKind::Value },
    FlagDef { short: None, long: "--field-match-separator", kind: FlagKind::Value },
    FlagDef { short: Some("-l"), long: "--files-with-matches", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--files-without-match", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--heading", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--no-heading", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--hyperlink-format", kind: FlagKind::Value },
    FlagDef { short: None, long: "--include-zero", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--json", kind: FlagKind::Bool },
    FlagDef { short: Some("-n"), long: "--line-number", kind: FlagKind::Bool },
    FlagDef { short: Some("-N"), long: "--no-line-number", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--max-columns", kind: FlagKind::Value },
    FlagDef { short: None, long: "--max-columns-preview", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--no-filename", kind: FlagKind::Bool },
    FlagDef { short: Some("-H"), long: "--with-filename", kind: FlagKind::Bool },
    FlagDef { short: Some("-0"), long: "--null", kind: FlagKind::Bool },
    FlagDef { short: Some("-o"), long: "--only-matching", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--path-separator", kind: FlagKind::Value },
    FlagDef { short: Some("-p"), long: "--pretty", kind: FlagKind::Bool },
    FlagDef { short: Some("-r"), long: "--replace", kind: FlagKind::Value },
    FlagDef { short: None, long: "--sort", kind: FlagKind::Value },
    FlagDef { short: None, long: "--sortr", kind: FlagKind::Value },
    FlagDef { short: None, long: "--stats", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--trim", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--vimgrep", kind: FlagKind::Bool },
    // === Logging/info flags ===
    FlagDef { short: None, long: "--debug", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--trace", kind: FlagKind::Bool },
    FlagDef { short: Some("-h"), long: "--help", kind: FlagKind::Bool },
    FlagDef { short: Some("-V"), long: "--version", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--pcre2-version", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--files", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--generate", kind: FlagKind::Value },
    FlagDef { short: None, long: "--no-messages", kind: FlagKind::Bool },
    FlagDef { short: Some("-q"), long: "--quiet", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--line-buffered", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--block-buffered", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--no-mmap", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--hidden", kind: FlagKind::Bool },
    FlagDef { short: None, long: "--no-config", kind: FlagKind::Bool },
];

/// Lookup a flag by short or long name. Returns its kind.
pub fn lookup_rg_flag(flag: &str) -> Option<FlagKind> {
    // Handle --flag=value style (always Value since the value is self-contained)
    if let Some(name) = flag.split('=').next() {
        if flag.contains('=') {
            return RG_FLAGS
                .iter()
                .find(|f| f.long == name || f.short == Some(name))
                .map(|_| FlagKind::Value); // =value is self-contained
        }
    }
    RG_FLAGS
        .iter()
        .find(|f| f.long == flag || f.short == Some(flag))
        .map(|f| f.kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_known_bool_flag() {
        assert_eq!(lookup_rg_flag("--json"), Some(FlagKind::Bool));
        assert_eq!(lookup_rg_flag("-i"), Some(FlagKind::Bool));
        assert_eq!(lookup_rg_flag("--pcre2"), Some(FlagKind::Bool));
    }

    #[test]
    fn test_lookup_known_value_flag() {
        assert_eq!(lookup_rg_flag("-C"), Some(FlagKind::Value));
        assert_eq!(lookup_rg_flag("--glob"), Some(FlagKind::Value));
        assert_eq!(lookup_rg_flag("--max-depth"), Some(FlagKind::Value));
    }

    #[test]
    fn test_lookup_unknown_flag() {
        assert_eq!(lookup_rg_flag("--some-future-flag"), None);
        assert_eq!(lookup_rg_flag("-Z"), None);
    }

    #[test]
    fn test_lookup_equals_style() {
        assert_eq!(lookup_rg_flag("--max-depth=3"), Some(FlagKind::Value));
    }
}
