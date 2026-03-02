/// Argument parsers for `find` and `fd` commands.
///
/// These parsers extract the subset of flags that AI agents commonly use.
/// Unsupported flags cause a `None` return, signalling fallback to the real binary.

// ---------------------------------------------------------------------------
// Common types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    File,
    Dir,
}

#[derive(Debug)]
pub struct FindParsedArgs {
    pub search_dir: String,
    pub name_pattern: Option<String>,
    pub iname_pattern: Option<String>,
    pub path_pattern: Option<String>,
    pub entry_type: Option<EntryType>,
}

#[derive(Debug)]
pub struct FdParsedArgs {
    pub pattern: Option<String>,
    pub search_dir: String,
    pub extension: Option<String>,
    pub entry_type: Option<EntryType>,
    pub ignore_case: bool,
}

// ---------------------------------------------------------------------------
// find parser
// ---------------------------------------------------------------------------

/// Parse `find` arguments.
///
/// Supported:
///   find [dir] -name PATTERN -iname PATTERN -type f|d -path PATTERN
///
/// Returns `None` for any unsupported flag (triggers fallback).
pub fn parse_find_args(args: &[String]) -> Option<FindParsedArgs> {
    let mut parsed = FindParsedArgs {
        search_dir: ".".into(),
        name_pattern: None,
        iname_pattern: None,
        path_pattern: None,
        entry_type: None,
    };

    let mut iter = args.iter().skip(1).peekable(); // skip argv[0]

    // Collect leading positional args (search dir) before any flags
    // find allows the directory to come first before any expressions
    if let Some(first) = iter.peek() {
        if !first.starts_with('-') && *first != "(" && *first != "!" {
            parsed.search_dir = iter.next().unwrap().clone();
        }
    }

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-name" => {
                parsed.name_pattern = Some(iter.next()?.clone());
            }
            "-iname" => {
                parsed.iname_pattern = Some(iter.next()?.clone());
            }
            "-type" => {
                let t = iter.next()?;
                parsed.entry_type = Some(parse_type_char(t)?);
            }
            "-path" => {
                parsed.path_pattern = Some(iter.next()?.clone());
            }
            // Unsupported flag -> fallback
            _ => return None,
        }
    }

    Some(parsed)
}

// ---------------------------------------------------------------------------
// fd parser
// ---------------------------------------------------------------------------

/// Parse `fd` arguments.
///
/// Supported:
///   fd [pattern] [dir] -e EXT --extension EXT -t TYPE --type TYPE -i --ignore-case
///
/// Returns `None` for any unsupported flag (triggers fallback).
pub fn parse_fd_args(args: &[String]) -> Option<FdParsedArgs> {
    let mut parsed = FdParsedArgs {
        pattern: None,
        search_dir: ".".into(),
        extension: None,
        entry_type: None,
        ignore_case: false,
    };

    let mut iter = args.iter().skip(1).peekable(); // skip argv[0]
    let mut positionals = Vec::new();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-e" | "--extension" => {
                parsed.extension = Some(iter.next()?.clone());
            }
            "-t" | "--type" => {
                let t = iter.next()?;
                parsed.entry_type = Some(parse_type_char(t)?);
            }
            "-i" | "--ignore-case" => {
                parsed.ignore_case = true;
            }
            s if s.starts_with('-') => {
                // Unsupported flag -> fallback
                return None;
            }
            _ => {
                positionals.push(arg.clone());
            }
        }
    }

    if let Some(first) = positionals.first() {
        parsed.pattern = Some(first.clone());
    }
    if let Some(second) = positionals.get(1) {
        parsed.search_dir = second.clone();
    }

    Some(parsed)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_type_char(s: &str) -> Option<EntryType> {
    match s {
        "f" | "file" => Some(EntryType::File),
        "d" | "directory" => Some(EntryType::Dir),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Glob matching (simple, no external crate)
// ---------------------------------------------------------------------------

/// Match a filename against a simple glob pattern.
///
/// Supports:
///   `*`  - matches any sequence of characters
///   `?`  - matches any single character
///   rest - literal match
///
/// This is intentionally simple and covers patterns like `*.rs`, `test_*`, `*.{rs}` etc.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    glob_match_inner(pattern.as_bytes(), text.as_bytes())
}

fn glob_match_inner(pattern: &[u8], text: &[u8]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi = usize::MAX;
    let mut star_ti = 0;

    while ti < text.len() {
        if pi < pattern.len() && (pattern[pi] == b'?' || pattern[pi] == text[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }

    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }

    pi == pattern.len()
}

/// Case-insensitive glob match.
pub fn glob_match_ci(pattern: &str, text: &str) -> bool {
    glob_match(&pattern.to_lowercase(), &text.to_lowercase())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- glob matching tests ------------------------------------------------

    #[test]
    fn test_glob_star() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(glob_match("*.rs", "lib.rs"));
        assert!(!glob_match("*.rs", "main.txt"));
        assert!(glob_match("test_*", "test_foo"));
        assert!(!glob_match("test_*", "foo_test"));
    }

    #[test]
    fn test_glob_question() {
        assert!(glob_match("?.rs", "a.rs"));
        assert!(!glob_match("?.rs", "ab.rs"));
    }

    #[test]
    fn test_glob_literal() {
        assert!(glob_match("Makefile", "Makefile"));
        assert!(!glob_match("Makefile", "makefile"));
    }

    #[test]
    fn test_glob_match_ci() {
        assert!(glob_match_ci("*.RS", "main.rs"));
        assert!(glob_match_ci("Makefile", "makefile"));
    }

    #[test]
    fn test_glob_complex() {
        assert!(glob_match("*test*.rs", "my_test_file.rs"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("**", "anything"));
    }

    // -- find parser tests --------------------------------------------------

    #[test]
    fn test_parse_find_basic() {
        let args: Vec<String> = vec!["find", "."]
            .into_iter().map(String::from).collect();
        let parsed = parse_find_args(&args).unwrap();
        assert_eq!(parsed.search_dir, ".");
        assert!(parsed.name_pattern.is_none());
    }

    #[test]
    fn test_parse_find_name() {
        let args: Vec<String> = vec!["find", "src", "-name", "*.rs"]
            .into_iter().map(String::from).collect();
        let parsed = parse_find_args(&args).unwrap();
        assert_eq!(parsed.search_dir, "src");
        assert_eq!(parsed.name_pattern.as_deref(), Some("*.rs"));
    }

    #[test]
    fn test_parse_find_iname() {
        let args: Vec<String> = vec!["find", ".", "-iname", "Readme*"]
            .into_iter().map(String::from).collect();
        let parsed = parse_find_args(&args).unwrap();
        assert_eq!(parsed.iname_pattern.as_deref(), Some("Readme*"));
    }

    #[test]
    fn test_parse_find_type_f() {
        let args: Vec<String> = vec!["find", ".", "-type", "f"]
            .into_iter().map(String::from).collect();
        let parsed = parse_find_args(&args).unwrap();
        assert_eq!(parsed.entry_type, Some(EntryType::File));
    }

    #[test]
    fn test_parse_find_type_d() {
        let args: Vec<String> = vec!["find", ".", "-type", "d"]
            .into_iter().map(String::from).collect();
        let parsed = parse_find_args(&args).unwrap();
        assert_eq!(parsed.entry_type, Some(EntryType::Dir));
    }

    #[test]
    fn test_parse_find_path() {
        let args: Vec<String> = vec!["find", ".", "-path", "*/src/*.rs"]
            .into_iter().map(String::from).collect();
        let parsed = parse_find_args(&args).unwrap();
        assert_eq!(parsed.path_pattern.as_deref(), Some("*/src/*.rs"));
    }

    #[test]
    fn test_parse_find_combined() {
        let args: Vec<String> = vec!["find", "/tmp", "-name", "*.log", "-type", "f"]
            .into_iter().map(String::from).collect();
        let parsed = parse_find_args(&args).unwrap();
        assert_eq!(parsed.search_dir, "/tmp");
        assert_eq!(parsed.name_pattern.as_deref(), Some("*.log"));
        assert_eq!(parsed.entry_type, Some(EntryType::File));
    }

    #[test]
    fn test_parse_find_unsupported_flag() {
        let args: Vec<String> = vec!["find", ".", "-maxdepth", "3"]
            .into_iter().map(String::from).collect();
        assert!(parse_find_args(&args).is_none());
    }

    #[test]
    fn test_parse_find_unsupported_exec() {
        let args: Vec<String> = vec!["find", ".", "-exec", "rm", "{}", ";"]
            .into_iter().map(String::from).collect();
        assert!(parse_find_args(&args).is_none());
    }

    // -- fd parser tests ----------------------------------------------------

    #[test]
    fn test_parse_fd_basic() {
        let args: Vec<String> = vec!["fd", "pattern"]
            .into_iter().map(String::from).collect();
        let parsed = parse_fd_args(&args).unwrap();
        assert_eq!(parsed.pattern.as_deref(), Some("pattern"));
        assert_eq!(parsed.search_dir, ".");
    }

    #[test]
    fn test_parse_fd_with_dir() {
        let args: Vec<String> = vec!["fd", "test", "src/"]
            .into_iter().map(String::from).collect();
        let parsed = parse_fd_args(&args).unwrap();
        assert_eq!(parsed.pattern.as_deref(), Some("test"));
        assert_eq!(parsed.search_dir, "src/");
    }

    #[test]
    fn test_parse_fd_extension() {
        let args: Vec<String> = vec!["fd", "-e", "rs"]
            .into_iter().map(String::from).collect();
        let parsed = parse_fd_args(&args).unwrap();
        assert_eq!(parsed.extension.as_deref(), Some("rs"));
        assert!(parsed.pattern.is_none());
    }

    #[test]
    fn test_parse_fd_type() {
        let args: Vec<String> = vec!["fd", "-t", "f", "main"]
            .into_iter().map(String::from).collect();
        let parsed = parse_fd_args(&args).unwrap();
        assert_eq!(parsed.entry_type, Some(EntryType::File));
        assert_eq!(parsed.pattern.as_deref(), Some("main"));
    }

    #[test]
    fn test_parse_fd_ignore_case() {
        let args: Vec<String> = vec!["fd", "-i", "README"]
            .into_iter().map(String::from).collect();
        let parsed = parse_fd_args(&args).unwrap();
        assert!(parsed.ignore_case);
        assert_eq!(parsed.pattern.as_deref(), Some("README"));
    }

    #[test]
    fn test_parse_fd_combined() {
        let args: Vec<String> = vec!["fd", "-e", "rs", "-t", "f", "-i", "test", "src"]
            .into_iter().map(String::from).collect();
        let parsed = parse_fd_args(&args).unwrap();
        assert_eq!(parsed.extension.as_deref(), Some("rs"));
        assert_eq!(parsed.entry_type, Some(EntryType::File));
        assert!(parsed.ignore_case);
        assert_eq!(parsed.pattern.as_deref(), Some("test"));
        assert_eq!(parsed.search_dir, "src");
    }

    #[test]
    fn test_parse_fd_long_flags() {
        let args: Vec<String> = vec!["fd", "--extension", "py", "--type", "f", "--ignore-case"]
            .into_iter().map(String::from).collect();
        let parsed = parse_fd_args(&args).unwrap();
        assert_eq!(parsed.extension.as_deref(), Some("py"));
        assert_eq!(parsed.entry_type, Some(EntryType::File));
        assert!(parsed.ignore_case);
    }

    #[test]
    fn test_parse_fd_unsupported_flag() {
        let args: Vec<String> = vec!["fd", "--hidden", "pattern"]
            .into_iter().map(String::from).collect();
        assert!(parse_fd_args(&args).is_none());
    }

    #[test]
    fn test_parse_fd_no_pattern() {
        // fd with no args at all - list everything
        let args: Vec<String> = vec!["fd"]
            .into_iter().map(String::from).collect();
        let parsed = parse_fd_args(&args).unwrap();
        assert!(parsed.pattern.is_none());
    }
}
