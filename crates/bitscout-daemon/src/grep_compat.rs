/// grep-compatible argument parser for BitScout.
///
/// Supports the subset of GNU grep flags commonly used by AI coding agents.
/// Unsupported flags cause a fallback to the real grep binary.

#[derive(Debug, Clone)]
pub struct GrepParsedArgs {
    pub pattern: String,
    pub paths: Vec<String>,
    pub line_numbers: bool,
    pub case_insensitive: bool,
    pub files_only: bool,
    pub count_only: bool,
    pub show_filename: Option<bool>, // None = auto (multi-file => show)
    pub word_regexp: bool,
    pub include_glob: Option<String>,
}

/// Known grep boolean flags that we accelerate.
const KNOWN_BOOL: &[&str] = &[
    "-r",
    "-R",
    "--recursive",
    "-n",
    "--line-number",
    "-i",
    "--ignore-case",
    "-l",
    "--files-with-matches",
    "-c",
    "--count",
    "-H",
    "-h",
    "--no-filename",
    "-w",
    "--word-regexp",
];

/// Parse grep-style arguments.
///
/// Returns `Some(GrepParsedArgs)` when all flags are in the supported subset.
/// Returns `None` for any unsupported flag, triggering fallback to real grep.
pub fn parse_grep_args(args: &[String]) -> Option<GrepParsedArgs> {
    let mut parsed = GrepParsedArgs {
        pattern: String::new(),
        paths: Vec::new(),
        line_numbers: false,
        case_insensitive: false,
        files_only: false,
        count_only: false,
        show_filename: None,
        word_regexp: false,
        include_glob: None,
    };

    let mut positional = Vec::new();
    let mut iter = args.iter().skip(1).peekable(); // skip argv[0]
    let mut seen_double_dash = false;

    while let Some(arg) = iter.next() {
        if seen_double_dash {
            positional.push(arg.clone());
            continue;
        }

        if arg == "--" {
            seen_double_dash = true;
            continue;
        }

        // Handle --include=GLOB
        if arg.starts_with("--include=") {
            let value = &arg["--include=".len()..];
            parsed.include_glob = Some(value.to_string());
            continue;
        }

        // Handle --include GLOB (space-separated)
        if arg == "--include" {
            let value = iter.next()?;
            parsed.include_glob = Some(value.clone());
            continue;
        }

        if arg.starts_with('-') && arg.len() > 1 {
            // Could be combined short flags like -rn or -ril
            if !arg.starts_with("--") && arg.len() > 2 {
                // Combined short flags: split and handle each
                let chars: Vec<char> = arg[1..].chars().collect();
                for ch in &chars {
                    let flag = format!("-{}", ch);
                    if !is_known_bool(&flag) {
                        return None; // unknown flag in combined set
                    }
                    apply_bool_flag(&mut parsed, &flag);
                }
                continue;
            }

            // Single flag
            if !is_known_bool(arg) {
                return None; // unsupported flag -> fallback
            }
            apply_bool_flag(&mut parsed, arg);
            continue;
        }

        // Positional argument
        positional.push(arg.clone());
    }

    if positional.is_empty() {
        return None; // no pattern
    }

    parsed.pattern = positional.remove(0);
    parsed.paths = positional;

    // Default path is "." if none given
    if parsed.paths.is_empty() {
        parsed.paths.push(".".into());
    }

    Some(parsed)
}

fn is_known_bool(flag: &str) -> bool {
    KNOWN_BOOL.contains(&flag)
}

fn apply_bool_flag(parsed: &mut GrepParsedArgs, flag: &str) {
    match flag {
        "-r" | "-R" | "--recursive" => {} // always recursive for us, no-op
        "-n" | "--line-number" => parsed.line_numbers = true,
        "-i" | "--ignore-case" => parsed.case_insensitive = true,
        "-l" | "--files-with-matches" => parsed.files_only = true,
        "-c" | "--count" => parsed.count_only = true,
        "-H" => parsed.show_filename = Some(true),
        "-h" | "--no-filename" => parsed.show_filename = Some(false),
        "-w" | "--word-regexp" => parsed.word_regexp = true,
        _ => {}
    }
}

/// Determine if filenames should be shown, considering explicit flags and path count.
pub fn should_show_filename(parsed: &GrepParsedArgs) -> bool {
    match parsed.show_filename {
        Some(v) => v,
        None => true, // multi-file recursive search: always show by default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_grep() {
        let args: Vec<String> = vec!["grep", "pattern", "."]
            .into_iter()
            .map(Into::into)
            .collect();
        let parsed = parse_grep_args(&args).unwrap();
        assert_eq!(parsed.pattern, "pattern");
        assert_eq!(parsed.paths, vec!["."]);
    }

    #[test]
    fn test_no_pattern_returns_none() {
        let args: Vec<String> = vec!["grep"].into_iter().map(Into::into).collect();
        assert!(parse_grep_args(&args).is_none());
    }

    #[test]
    fn test_recursive_and_line_numbers() {
        let args: Vec<String> = vec!["grep", "-rn", "pattern", "src/"]
            .into_iter()
            .map(Into::into)
            .collect();
        let parsed = parse_grep_args(&args).unwrap();
        assert!(parsed.line_numbers);
        assert_eq!(parsed.pattern, "pattern");
        assert_eq!(parsed.paths, vec!["src/"]);
    }

    #[test]
    fn test_combined_flags() {
        let args: Vec<String> = vec!["grep", "-ril", "pattern"]
            .into_iter()
            .map(Into::into)
            .collect();
        let parsed = parse_grep_args(&args).unwrap();
        assert!(parsed.case_insensitive);
        assert!(parsed.files_only);
    }

    #[test]
    fn test_include_glob_equals() {
        let args: Vec<String> = vec!["grep", "-r", "--include=*.rs", "pattern"]
            .into_iter()
            .map(Into::into)
            .collect();
        let parsed = parse_grep_args(&args).unwrap();
        assert_eq!(parsed.include_glob.as_deref(), Some("*.rs"));
    }

    #[test]
    fn test_include_glob_space() {
        let args: Vec<String> = vec!["grep", "-r", "--include", "*.py", "pattern"]
            .into_iter()
            .map(Into::into)
            .collect();
        let parsed = parse_grep_args(&args).unwrap();
        assert_eq!(parsed.include_glob.as_deref(), Some("*.py"));
    }

    #[test]
    fn test_word_regexp() {
        let args: Vec<String> = vec!["grep", "-rw", "pattern"]
            .into_iter()
            .map(Into::into)
            .collect();
        let parsed = parse_grep_args(&args).unwrap();
        assert!(parsed.word_regexp);
    }

    #[test]
    fn test_suppress_filename() {
        let args: Vec<String> = vec!["grep", "-rh", "pattern"]
            .into_iter()
            .map(Into::into)
            .collect();
        let parsed = parse_grep_args(&args).unwrap();
        assert_eq!(parsed.show_filename, Some(false));
        assert!(!should_show_filename(&parsed));
    }

    #[test]
    fn test_force_filename() {
        let args: Vec<String> = vec!["grep", "-H", "pattern", "file.txt"]
            .into_iter()
            .map(Into::into)
            .collect();
        let parsed = parse_grep_args(&args).unwrap();
        assert_eq!(parsed.show_filename, Some(true));
        assert!(should_show_filename(&parsed));
    }

    #[test]
    fn test_unsupported_flag_returns_none() {
        let args: Vec<String> = vec!["grep", "-P", "pattern"]
            .into_iter()
            .map(Into::into)
            .collect();
        assert!(parse_grep_args(&args).is_none());
    }

    #[test]
    fn test_unsupported_long_flag_returns_none() {
        let args: Vec<String> = vec!["grep", "--perl-regexp", "pattern"]
            .into_iter()
            .map(Into::into)
            .collect();
        assert!(parse_grep_args(&args).is_none());
    }

    #[test]
    fn test_double_dash_separator() {
        let args: Vec<String> = vec!["grep", "-r", "--", "-pattern", "."]
            .into_iter()
            .map(Into::into)
            .collect();
        let parsed = parse_grep_args(&args).unwrap();
        assert_eq!(parsed.pattern, "-pattern");
        assert_eq!(parsed.paths, vec!["."]);
    }

    #[test]
    fn test_default_path_when_omitted() {
        let args: Vec<String> = vec!["grep", "-r", "pattern"]
            .into_iter()
            .map(Into::into)
            .collect();
        let parsed = parse_grep_args(&args).unwrap();
        assert_eq!(parsed.paths, vec!["."]);
    }

    #[test]
    fn test_count_flag() {
        let args: Vec<String> = vec!["grep", "-rc", "pattern", "."]
            .into_iter()
            .map(Into::into)
            .collect();
        let parsed = parse_grep_args(&args).unwrap();
        assert!(parsed.count_only);
    }

    #[test]
    fn test_multiple_paths() {
        let args: Vec<String> = vec!["grep", "-r", "pattern", "src/", "tests/"]
            .into_iter()
            .map(Into::into)
            .collect();
        let parsed = parse_grep_args(&args).unwrap();
        assert_eq!(parsed.pattern, "pattern");
        assert_eq!(parsed.paths, vec!["src/", "tests/"]);
    }

    #[test]
    fn test_auto_filename_display() {
        let args: Vec<String> = vec!["grep", "-r", "pattern"]
            .into_iter()
            .map(Into::into)
            .collect();
        let parsed = parse_grep_args(&args).unwrap();
        assert_eq!(parsed.show_filename, None);
        // Default for recursive: show filenames
        assert!(should_show_filename(&parsed));
    }
}
