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
    pub fixed_strings: bool,
}

pub fn parse_find_args(args: &[String]) -> Option<FindParsedArgs> {
    let mut parsed = FindParsedArgs {
        search_dir: ".".into(),
        name_pattern: None,
        iname_pattern: None,
        path_pattern: None,
        entry_type: None,
    };

    let mut iter = args.iter().skip(1).peekable();

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
            _ => return None,
        }
    }

    Some(parsed)
}

pub fn parse_fd_args(args: &[String]) -> Option<FdParsedArgs> {
    let mut parsed = FdParsedArgs {
        pattern: None,
        search_dir: ".".into(),
        extension: None,
        entry_type: None,
        ignore_case: false,
        fixed_strings: false,
    };

    let mut iter = args.iter().skip(1).peekable();
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
            "-F" | "--fixed-strings" => {
                parsed.fixed_strings = true;
            }
            s if s.starts_with('-') => {
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

fn parse_type_char(s: &str) -> Option<EntryType> {
    match s {
        "f" | "file" => Some(EntryType::File),
        "d" | "directory" => Some(EntryType::Dir),
        _ => None,
    }
}

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

pub fn glob_match_ci(pattern: &str, text: &str) -> bool {
    glob_match(&pattern.to_lowercase(), &text.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_star() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(!glob_match("*.rs", "main.txt"));
    }

    #[test]
    fn test_glob_match_ci() {
        assert!(glob_match_ci("*.RS", "main.rs"));
    }

    #[test]
    fn test_parse_find_basic() {
        let args: Vec<String> = vec!["find", "."]
            .into_iter().map(String::from).collect();
        let parsed = parse_find_args(&args).unwrap();
        assert_eq!(parsed.search_dir, ".");
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
    fn test_parse_fd_basic() {
        let args: Vec<String> = vec!["fd", "pattern"]
            .into_iter().map(String::from).collect();
        let parsed = parse_fd_args(&args).unwrap();
        assert_eq!(parsed.pattern.as_deref(), Some("pattern"));
    }

    #[test]
    fn test_parse_fd_extension() {
        let args: Vec<String> = vec!["fd", "-e", "rs"]
            .into_iter().map(String::from).collect();
        let parsed = parse_fd_args(&args).unwrap();
        assert_eq!(parsed.extension.as_deref(), Some("rs"));
    }

    #[test]
    fn test_parse_find_unsupported_flag() {
        let args: Vec<String> = vec!["find", ".", "-maxdepth", "3"]
            .into_iter().map(String::from).collect();
        assert!(parse_find_args(&args).is_none());
    }
}
