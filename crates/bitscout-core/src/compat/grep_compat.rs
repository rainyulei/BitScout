use crate::search::bm25::Bm25Mode;

#[derive(Debug, Clone)]
pub struct GrepParsedArgs {
    pub pattern: String,
    pub paths: Vec<String>,
    pub line_numbers: bool,
    pub case_insensitive: bool,
    pub files_only: bool,
    pub count_only: bool,
    pub show_filename: Option<bool>,
    pub word_regexp: bool,
    pub include_glob: Option<String>,
    pub fixed_strings: bool,
    pub bm25: Bm25Mode,
    pub semantic: bool,
}

const KNOWN_BOOL: &[&str] = &[
    "-r", "-R", "--recursive",
    "-n", "--line-number",
    "-i", "--ignore-case",
    "-l", "--files-with-matches",
    "-c", "--count",
    "-H", "-h", "--no-filename",
    "-w", "--word-regexp",
    "-F", "--fixed-strings",
];

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
        fixed_strings: false,
        bm25: Bm25Mode::Off,
        semantic: false,
    };

    let mut positional = Vec::new();
    let mut iter = args.iter().skip(1).peekable();
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

        // BitScout-specific: --bm25 / --bm25=full
        if arg == "--bm25" {
            parsed.bm25 = Bm25Mode::Tf;
            continue;
        }
        if arg.starts_with("--bm25=") {
            let value = &arg["--bm25=".len()..];
            parsed.bm25 = match value {
                "full" => Bm25Mode::Full,
                _ => Bm25Mode::Tf,
            };
            continue;
        }

        // BitScout-specific: --semantic
        if arg == "--semantic" {
            parsed.semantic = true;
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
            if !arg.starts_with("--") && arg.len() > 2 {
                let chars: Vec<char> = arg[1..].chars().collect();
                for ch in &chars {
                    let flag = format!("-{}", ch);
                    if !is_known_bool(&flag) {
                        return None;
                    }
                    apply_bool_flag(&mut parsed, &flag);
                }
                continue;
            }

            if !is_known_bool(arg) {
                return None;
            }
            apply_bool_flag(&mut parsed, arg);
            continue;
        }

        positional.push(arg.clone());
    }

    if positional.is_empty() {
        return None;
    }

    parsed.pattern = positional.remove(0);
    parsed.paths = positional;

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
        "-r" | "-R" | "--recursive" => {}
        "-n" | "--line-number" => parsed.line_numbers = true,
        "-i" | "--ignore-case" => parsed.case_insensitive = true,
        "-l" | "--files-with-matches" => parsed.files_only = true,
        "-c" | "--count" => parsed.count_only = true,
        "-H" => parsed.show_filename = Some(true),
        "-h" | "--no-filename" => parsed.show_filename = Some(false),
        "-w" | "--word-regexp" => parsed.word_regexp = true,
        "-F" | "--fixed-strings" => parsed.fixed_strings = true,
        _ => {}
    }
}

pub fn should_show_filename(parsed: &GrepParsedArgs) -> bool {
    match parsed.show_filename {
        Some(v) => v,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_grep() {
        let args: Vec<String> = vec!["grep", "pattern", "."]
            .into_iter().map(Into::into).collect();
        let parsed = parse_grep_args(&args).unwrap();
        assert_eq!(parsed.pattern, "pattern");
        assert_eq!(parsed.paths, vec!["."]);
    }

    #[test]
    fn test_combined_flags() {
        let args: Vec<String> = vec!["grep", "-ril", "pattern"]
            .into_iter().map(Into::into).collect();
        let parsed = parse_grep_args(&args).unwrap();
        assert!(parsed.case_insensitive);
        assert!(parsed.files_only);
    }

    #[test]
    fn test_include_glob_equals() {
        let args: Vec<String> = vec!["grep", "-r", "--include=*.rs", "pattern"]
            .into_iter().map(Into::into).collect();
        let parsed = parse_grep_args(&args).unwrap();
        assert_eq!(parsed.include_glob.as_deref(), Some("*.rs"));
    }

    #[test]
    fn test_parse_grep_bm25_flag() {
        let args: Vec<String> = vec!["grep", "-r", "--bm25", "pattern", "."]
            .into_iter().map(Into::into).collect();
        let parsed = parse_grep_args(&args).unwrap();
        assert_eq!(parsed.bm25, Bm25Mode::Tf);
    }

    #[test]
    fn test_parse_grep_semantic_flag() {
        let args: Vec<String> = vec!["grep", "-r", "--semantic", "pattern", "."]
            .into_iter().map(Into::into).collect();
        let parsed = parse_grep_args(&args).unwrap();
        assert!(parsed.semantic);
    }

    #[test]
    fn test_unsupported_flag_returns_none() {
        let args: Vec<String> = vec!["grep", "-P", "pattern"]
            .into_iter().map(Into::into).collect();
        assert!(parse_grep_args(&args).is_none());
    }

    #[test]
    fn test_double_dash_separator() {
        let args: Vec<String> = vec!["grep", "-r", "--", "-pattern", "."]
            .into_iter().map(Into::into).collect();
        let parsed = parse_grep_args(&args).unwrap();
        assert_eq!(parsed.pattern, "-pattern");
        assert_eq!(parsed.paths, vec!["."]);
    }
}
