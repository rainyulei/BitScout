use super::rg_flags::{lookup_rg_flag, FlagKind};
use crate::search::bm25::Bm25Mode;

#[derive(Debug)]
pub struct RgParsedArgs {
    pub pattern: String,
    pub path: String,
    pub json_output: bool,
    pub case_insensitive: bool,
    pub context_lines: usize,
    pub before_context: usize,
    pub after_context: usize,
    pub glob: Option<String>,
    pub file_type: Option<String>,
    pub count_only: bool,
    pub files_only: bool,
    pub line_numbers: bool,
    pub multiline: bool,
    pub fixed_strings: bool,
    pub bm25: Bm25Mode,
    pub semantic: bool,
}

/// Flags we actively accelerate (handle in our search engine).
const ACCELERATED_BOOL: &[&str] = &[
    "--json",
    "-n",
    "--line-number",
    "--no-line-number",
    "-i",
    "--ignore-case",
    "-l",
    "--files-with-matches",
    "-c",
    "--count",
    "-U",
    "--multiline",
    "--multiline-dotall",
    "-F",
    "--fixed-strings",
    "--no-heading",
    "--heading",
    "--no-config",
    "--no-ignore",
    "--hidden",
    "-s",
    "--case-sensitive",
    "--smart-case",
    "-S",
    "--no-messages",
];
const ACCELERATED_VALUE: &[&str] = &[
    "-C",
    "--context",
    "-A",
    "--after-context",
    "-B",
    "--before-context",
    "-g",
    "--glob",
    "-t",
    "--type",
    "--color",
    "--colors",
    "-m",
    "--max-count",
];

/// 3-layer rg argument parser.
///
/// Returns `Some(RgParsedArgs)` only when ALL flags are in the accelerated subset.
/// Returns `None` for unknown flags OR known-but-not-accelerated flags, triggering fallback.
pub fn parse_rg_args(args: &[String]) -> Option<RgParsedArgs> {
    let mut iter = args.iter().skip(1).peekable();
    let mut parsed = RgParsedArgs {
        pattern: String::new(),
        path: ".".into(),
        json_output: false,
        case_insensitive: false,
        context_lines: 0,
        before_context: 0,
        after_context: 0,
        glob: None,
        file_type: None,
        count_only: false,
        files_only: false,
        line_numbers: false,
        multiline: false,
        fixed_strings: false,
        bm25: Bm25Mode::Off,
        semantic: false,
    };

    let mut positional = Vec::new();

    while let Some(arg) = iter.next() {
        if arg == "--" {
            positional.extend(iter.cloned());
            break;
        }

        if !arg.starts_with('-') {
            positional.push(arg.clone());
            continue;
        }

        // BitScout-specific: --bm25 / --bm25=full
        if arg == "--bm25" {
            parsed.bm25 = Bm25Mode::Tf;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--bm25=") {
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

        // Handle --flag=value style
        if arg.contains('=') {
            let parts: Vec<&str> = arg.splitn(2, '=').collect();
            let flag_name = parts[0];
            let value = parts[1];

            lookup_rg_flag(flag_name)?;

            if !is_accelerated(flag_name) {
                return None;
            }

            apply_value_flag(&mut parsed, flag_name, value);
            continue;
        }

        // Layer 1: Look up in complete registry
        match lookup_rg_flag(arg) {
            None => return None,

            Some(FlagKind::Bool) => {
                if !is_accelerated(arg) {
                    return None;
                }
                apply_bool_flag(&mut parsed, arg);
            }

            Some(FlagKind::Value) => {
                let value = iter.next()?;
                if !is_accelerated(arg) {
                    return None;
                }
                apply_value_flag(&mut parsed, arg, value);
            }
        }
    }

    if positional.is_empty() {
        return None;
    }
    parsed.pattern = positional[0].clone();
    if positional.len() > 1 {
        parsed.path = positional[1].clone();
    }

    if parsed.context_lines > 0 {
        parsed.before_context = parsed.context_lines;
        parsed.after_context = parsed.context_lines;
    }

    Some(parsed)
}

fn is_accelerated(flag: &str) -> bool {
    ACCELERATED_BOOL.contains(&flag) || ACCELERATED_VALUE.contains(&flag)
}

fn apply_bool_flag(parsed: &mut RgParsedArgs, flag: &str) {
    match flag {
        "--json" => parsed.json_output = true,
        "-n" | "--line-number" => parsed.line_numbers = true,
        "--no-line-number" => parsed.line_numbers = false,
        "-i" | "--ignore-case" => parsed.case_insensitive = true,
        "-s" | "--case-sensitive" => parsed.case_insensitive = false,
        "-S" | "--smart-case" => {}
        "-l" | "--files-with-matches" => parsed.files_only = true,
        "-c" | "--count" => parsed.count_only = true,
        "-U" | "--multiline" => parsed.multiline = true,
        "--multiline-dotall" => parsed.multiline = true,
        "-F" | "--fixed-strings" => parsed.fixed_strings = true,
        "--no-heading" | "--heading" | "--no-config" | "--no-ignore" | "--hidden"
        | "--no-messages" => {}
        _ => {}
    }
}

fn apply_value_flag(parsed: &mut RgParsedArgs, flag: &str, value: &str) {
    match flag {
        "-C" | "--context" => parsed.context_lines = value.parse().unwrap_or(0),
        "-A" | "--after-context" => parsed.after_context = value.parse().unwrap_or(0),
        "-B" | "--before-context" => parsed.before_context = value.parse().unwrap_or(0),
        "-g" | "--glob" => parsed.glob = Some(value.to_string()),
        "-t" | "--type" => parsed.file_type = Some(value.to_string()),
        "--color" | "--colors" => {}
        "-m" | "--max-count" => {}
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_rg_args() {
        let args = vec!["rg".into(), "pattern".into(), ".".into()];
        let parsed = parse_rg_args(&args).unwrap();
        assert_eq!(parsed.pattern, "pattern");
        assert_eq!(parsed.path, ".");
    }

    #[test]
    fn test_parse_rg_json_flag() {
        let args = vec!["rg".into(), "--json".into(), "pattern".into(), ".".into()];
        let parsed = parse_rg_args(&args).unwrap();
        assert!(parsed.json_output);
    }

    #[test]
    fn test_parse_rg_context_flags() {
        let args = vec![
            "rg".into(),
            "-C".into(),
            "3".into(),
            "-i".into(),
            "pattern".into(),
            "src/".into(),
        ];
        let parsed = parse_rg_args(&args).unwrap();
        assert_eq!(parsed.context_lines, 3);
        assert!(parsed.case_insensitive);
        assert_eq!(parsed.path, "src/");
    }

    #[test]
    fn test_value_flag_not_mistaken_for_pattern() {
        let args = vec![
            "rg".into(),
            "--max-depth".into(),
            "3".into(),
            "real_pattern".into(),
            ".".into(),
        ];
        assert!(parse_rg_args(&args).is_none());
    }

    #[test]
    fn test_known_but_unaccelerated_flag_parses_correctly() {
        let args = vec!["rg".into(), "--pcre2".into(), "pattern".into()];
        assert!(parse_rg_args(&args).is_none());
    }

    #[test]
    fn test_parse_rg_bm25_flag() {
        let args = vec!["rg".into(), "--bm25".into(), "pattern".into(), ".".into()];
        let parsed = parse_rg_args(&args).unwrap();
        assert_eq!(parsed.bm25, Bm25Mode::Tf);
    }

    #[test]
    fn test_parse_rg_semantic_flag() {
        let args = vec![
            "rg".into(),
            "--semantic".into(),
            "pattern".into(),
            ".".into(),
        ];
        let parsed = parse_rg_args(&args).unwrap();
        assert!(parsed.semantic);
    }

    #[test]
    fn test_glob_with_value() {
        let args = vec![
            "rg".into(),
            "--glob".into(),
            "*.rs".into(),
            "pattern".into(),
        ];
        let parsed = parse_rg_args(&args).unwrap();
        assert_eq!(parsed.glob.as_deref(), Some("*.rs"));
        assert_eq!(parsed.pattern, "pattern");
    }
}
