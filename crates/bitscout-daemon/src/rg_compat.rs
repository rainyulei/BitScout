use crate::rg_flags::{lookup_rg_flag, FlagKind};

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
}

/// Flags we actively accelerate (handle in our search engine).
/// These are the flags commonly used by AI coding agents (Claude Code, Cursor, etc).
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
    // Display control — no-ops for us since we always produce the same format
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
/// Layer 1 (Flag Registry): Uses the complete rg flag table to correctly
///   identify every flag as Bool or Value, so we never misparse a value as a pattern.
/// Layer 2 (Generic Parser): Walks args, skips values for Value flags, collects positionals.
/// Layer 3 (Accelerator): Checks if all encountered flags are in our accelerated subset.
///   If any flag is unknown or known-but-not-accelerated, returns None to trigger fallback.
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
    };

    let mut positional = Vec::new();

    while let Some(arg) = iter.next() {
        if arg == "--" {
            // Everything after -- is positional
            positional.extend(iter.map(|s| s.clone()));
            break;
        }

        if !arg.starts_with('-') {
            positional.push(arg.clone());
            continue;
        }

        // Handle --flag=value style
        if arg.contains('=') {
            let parts: Vec<&str> = arg.splitn(2, '=').collect();
            let flag_name = parts[0];
            let value = parts[1];

            if lookup_rg_flag(flag_name).is_none() {
                return None; // completely unknown flag -> fallback
            }

            // Known flag with =value. Check if we accelerate it.
            if !is_accelerated(flag_name) {
                return None; // known but not acceleratable -> fallback
            }

            // Handle accelerated =value flags
            apply_value_flag(&mut parsed, flag_name, value);
            continue;
        }

        // Layer 1: Look up in complete registry
        match lookup_rg_flag(arg) {
            None => return None, // completely unknown -> fallback

            Some(FlagKind::Bool) => {
                if !is_accelerated(arg) {
                    return None; // known bool, not accelerated -> fallback
                }
                apply_bool_flag(&mut parsed, arg);
            }

            Some(FlagKind::Value) => {
                let value = iter.next()?; // consume the value
                if !is_accelerated(arg) {
                    return None; // known value flag, not accelerated -> fallback
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
        "-S" | "--smart-case" => {} // treat as default
        "-l" | "--files-with-matches" => parsed.files_only = true,
        "-c" | "--count" => parsed.count_only = true,
        "-U" | "--multiline" => parsed.multiline = true,
        "--multiline-dotall" => parsed.multiline = true,
        // Display flags — no-ops since our output format is always flat
        "--no-heading" | "--heading" | "--no-config" | "--no-ignore"
        | "--hidden" | "--no-messages" => {}
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
        // Flags we accept but don't act on (no-ops for our output)
        "--color" | "--colors" => {} // we never colorize
        "-m" | "--max-count" => {}    // TODO: implement max-count per file
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
        // --max-depth takes a value; "3" must not become the pattern
        let args = vec![
            "rg".into(),
            "--max-depth".into(),
            "3".into(),
            "real_pattern".into(),
            ".".into(),
        ];
        // --max-depth is known but not accelerated -> fallback
        let parsed = parse_rg_args(&args);
        assert!(parsed.is_none());
    }

    #[test]
    fn test_equals_style_value_flag() {
        // --max-depth=3 is known but not accelerated -> fallback
        let args = vec!["rg".into(), "--max-depth=3".into(), "pattern".into()];
        let parsed = parse_rg_args(&args);
        assert!(parsed.is_none());
    }

    #[test]
    fn test_known_but_unaccelerated_flag_parses_correctly() {
        // --pcre2 is a known Bool flag, parsed correctly, but triggers fallback
        let args = vec!["rg".into(), "--pcre2".into(), "pattern".into()];
        let parsed = parse_rg_args(&args);
        assert!(parsed.is_none()); // known flag but not acceleratable -> fallback
    }

    #[test]
    fn test_completely_unknown_flag_fallback() {
        let args = vec!["rg".into(), "--some-future-flag".into(), "pattern".into()];
        let parsed = parse_rg_args(&args);
        assert!(parsed.is_none()); // unknown -> fallback
    }

    #[test]
    fn test_combined_short_flags() {
        // rg doesn't combine short flags like -in, but test resilience
        let args = vec![
            "rg".into(),
            "-i".into(),
            "-n".into(),
            "pattern".into(),
        ];
        let parsed = parse_rg_args(&args).unwrap();
        assert!(parsed.case_insensitive);
        assert!(parsed.line_numbers);
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

    #[test]
    fn test_multiple_value_flags() {
        let args = vec![
            "rg".into(),
            "-A".into(),
            "2".into(),
            "-B".into(),
            "3".into(),
            "--type".into(),
            "rust".into(),
            "--glob".into(),
            "*.rs".into(),
            "pattern".into(),
            "src/".into(),
        ];
        let parsed = parse_rg_args(&args).unwrap();
        assert_eq!(parsed.after_context, 2);
        assert_eq!(parsed.before_context, 3);
        assert_eq!(parsed.file_type.as_deref(), Some("rust"));
        assert_eq!(parsed.glob.as_deref(), Some("*.rs"));
        assert_eq!(parsed.pattern, "pattern");
        assert_eq!(parsed.path, "src/");
    }
}
