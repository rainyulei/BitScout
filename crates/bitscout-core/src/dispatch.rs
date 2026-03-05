use crate::cache::content_cache::ContentCache;
use crate::protocol::SearchResponse;
use crate::search::bm25::Bm25Mode;
use crate::search::engine::{SearchEngine, SearchOptions, SearchResult};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::compat::grep_compat::{self, GrepParsedArgs};
use crate::compat::rg_compat::RgParsedArgs;

/// Special exit code that tells the caller to fall back to the original command.
pub const FALLBACK_EXIT_CODE: i32 = 200;

/// Cold-start dispatch: scans FileTree on each call (0.12ms overhead, negligible).
pub fn dispatch(command: &str, args: &[String], cwd: &str) -> SearchResponse {
    match command {
        "rg" => handle_rg(command, args, cwd),
        "grep" => handle_grep(command, args, cwd),
        "find" => handle_find_cmd(args, cwd),
        "fd" => handle_fd_cmd(args, cwd),
        "cat" => handle_cat(args, cwd),
        _ => fallback_response(&format!("unknown command: {}", command)),
    }
}

fn fallback_response(reason: &str) -> SearchResponse {
    SearchResponse {
        exit_code: FALLBACK_EXIT_CODE,
        stdout: String::new(),
        stderr: format!("BITSCOUT_FALLBACK: {}", reason),
    }
}

fn cold_start_engine(search_path: &Path, cmd: &str) -> Result<SearchEngine, SearchResponse> {
    let mut engine = SearchEngine::new(search_path).map_err(|e| SearchResponse {
        exit_code: 2,
        stdout: String::new(),
        stderr: format!("{}: {}", cmd, e),
    })?;
    // Attach content cache from default dir (~/.bitscout/cache/content/)
    if let Some(home) = std::env::var_os("HOME") {
        let cache_dir = PathBuf::from(home).join(".bitscout/cache/content");
        let cache = ContentCache::new(&cache_dir);
        engine = engine.with_cache(cache);
    }
    Ok(engine)
}

fn handle_rg(command: &str, args: &[String], cwd: &str) -> SearchResponse {
    let mut full_args = vec![command.to_string()];
    full_args.extend(args.iter().cloned());

    let parsed = match crate::compat::rg_compat::parse_rg_args(&full_args) {
        Some(p) => p,
        None => return fallback_response("unsupported rg flags"),
    };

    let search_path = resolve_path(cwd, &parsed.path);
    let use_regex = !parsed.fixed_strings;

    let results = if search_path.is_file() {
        match search_single_file_rg(&parsed, &search_path, use_regex) {
            Ok(r) => r,
            Err(resp) => return resp,
        }
    } else {
        let engine = match cold_start_engine(&search_path, "rg") {
            Ok(e) => e,
            Err(resp) => return resp,
        };

        let opts = SearchOptions {
            case_insensitive: parsed.case_insensitive,
            context_lines: parsed.before_context.max(parsed.after_context),
            max_results: 100_000,
            use_regex,
            bm25: parsed.bm25,
            semantic: parsed.semantic,
            search_root: Some(search_path),
        };

        match engine.search(&parsed.pattern, &opts) {
            Ok(r) => r,
            Err(e) => {
                return SearchResponse {
                    exit_code: 2,
                    stdout: String::new(),
                    stderr: format!("rg: {}", e),
                }
            }
        }
    };

    if results.is_empty() {
        let stderr = if parsed.semantic {
            format!("No semantically relevant files found for '{}'. Try more specific terms or use plain rg without --semantic.\n", parsed.pattern)
        } else {
            String::new()
        };
        return SearchResponse {
            exit_code: 1,
            stdout: String::new(),
            stderr,
        };
    }

    let stdout = if parsed.semantic {
        format_semantic_output(&results)
    } else {
        format_rg_output(&parsed, &results, parsed.bm25)
    };

    SearchResponse {
        exit_code: 0,
        stdout,
        stderr: String::new(),
    }
}

fn search_single_file_rg(
    parsed: &RgParsedArgs,
    search_path: &Path,
    use_regex: bool,
) -> Result<Vec<SearchResult>, SearchResponse> {
    let text = crate::extract::pipeline::extract_text(search_path).map_err(|e| SearchResponse {
        exit_code: 2,
        stdout: String::new(),
        stderr: format!("rg: {}: {}", search_path.display(), e),
    })?;
    let matcher = crate::search::matcher::Matcher::with_options(
        &[&parsed.pattern],
        crate::search::matcher::MatchOptions {
            case_insensitive: parsed.case_insensitive,
            use_regex,
        },
    )
    .map_err(|e| SearchResponse {
        exit_code: 2,
        stdout: String::new(),
        stderr: format!("rg: {}", e),
    })?;

    let bm25_score = if parsed.bm25 != Bm25Mode::Off {
        let tf = matcher.find_all(text.as_bytes()).len();
        let doc_len = text.len();
        let scorer = crate::search::bm25::Bm25Scorer::new(1, doc_len as f64);
        Some(scorer.tf_score(tf, doc_len))
    } else {
        None
    };

    let lines: Vec<&str> = text.lines().collect();
    let context = parsed.before_context.max(parsed.after_context);
    let mut results = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        if matcher.is_match(line.as_bytes()) {
            let before = if context > 0 {
                lines[idx.saturating_sub(context)..idx]
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            } else {
                Vec::new()
            };
            let after = if context > 0 {
                lines[idx + 1..lines.len().min(idx + 1 + context)]
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            } else {
                Vec::new()
            };
            results.push(SearchResult {
                path: search_path.to_path_buf(),
                line_number: idx + 1,
                line_content: line.to_string(),
                context_before: before,
                context_after: after,
                bm25_score,
            });
        }
    }
    Ok(results)
}

fn handle_grep(command: &str, args: &[String], cwd: &str) -> SearchResponse {
    let mut full_args = vec![command.to_string()];
    full_args.extend(args.iter().cloned());

    let parsed = match grep_compat::parse_grep_args(&full_args) {
        Some(p) => p,
        None => return fallback_response("unsupported grep flags"),
    };

    let use_regex = !parsed.fixed_strings || parsed.word_regexp;
    let effective_pattern = if parsed.word_regexp {
        if parsed.fixed_strings {
            format!(r"\b{}\b", regex::escape(&parsed.pattern))
        } else {
            format!(r"\b(?:{})\b", parsed.pattern)
        }
    } else {
        parsed.pattern.clone()
    };

    let mut all_results: Vec<SearchResult> = Vec::new();
    for path_str in &parsed.paths {
        let search_path = resolve_path(cwd, path_str);

        if search_path.is_file() {
            match search_single_file_grep(&parsed, &search_path, &effective_pattern, use_regex) {
                Ok(mut r) => all_results.append(&mut r),
                Err(resp) => return resp,
            }
        } else {
            let engine = match cold_start_engine(&search_path, "grep") {
                Ok(e) => e,
                Err(resp) => return resp,
            };

            let opts = SearchOptions {
                case_insensitive: parsed.case_insensitive,
                context_lines: 0,
                max_results: 100_000,
                use_regex,
                bm25: parsed.bm25,
                semantic: parsed.semantic,
                search_root: Some(search_path.clone()),
            };

            match engine.search(&effective_pattern, &opts) {
                Ok(mut results) => {
                    if let Some(ref glob_pat) = parsed.include_glob {
                        results.retain(|r| match_glob(glob_pat, r.path.as_path()));
                    }
                    all_results.extend(results);
                }
                Err(e) => {
                    return SearchResponse {
                        exit_code: 2,
                        stdout: String::new(),
                        stderr: format!("grep: {}", e),
                    }
                }
            }
        }
    }

    if all_results.is_empty() {
        return SearchResponse {
            exit_code: 1,
            stdout: String::new(),
            stderr: String::new(),
        };
    }

    let stdout = format_grep_output(&parsed, &all_results, parsed.bm25);

    SearchResponse {
        exit_code: 0,
        stdout,
        stderr: String::new(),
    }
}

fn search_single_file_grep(
    parsed: &GrepParsedArgs,
    search_path: &Path,
    effective_pattern: &str,
    use_regex: bool,
) -> Result<Vec<SearchResult>, SearchResponse> {
    let text = crate::extract::pipeline::extract_text(search_path).map_err(|e| SearchResponse {
        exit_code: 2,
        stdout: String::new(),
        stderr: format!("grep: {}: {}", search_path.display(), e),
    })?;
    let matcher = crate::search::matcher::Matcher::with_options(
        &[effective_pattern],
        crate::search::matcher::MatchOptions {
            case_insensitive: parsed.case_insensitive,
            use_regex,
        },
    )
    .map_err(|e| SearchResponse {
        exit_code: 2,
        stdout: String::new(),
        stderr: format!("grep: {}", e),
    })?;

    let bm25_score = if parsed.bm25 != Bm25Mode::Off {
        let tf = matcher.find_all(text.as_bytes()).len();
        let doc_len = text.len();
        let scorer = crate::search::bm25::Bm25Scorer::new(1, doc_len as f64);
        Some(scorer.tf_score(tf, doc_len))
    } else {
        None
    };

    let mut results = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        if matcher.is_match(line.as_bytes()) {
            results.push(SearchResult {
                path: search_path.to_path_buf(),
                line_number: idx + 1,
                line_content: line.to_string(),
                context_before: Vec::new(),
                context_after: Vec::new(),
                bm25_score,
            });
        }
    }
    Ok(results)
}

fn match_glob(pattern: &str, path: &Path) -> bool {
    let file_name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };

    if pattern.starts_with('*') && pattern.ends_with('*') && pattern.len() > 2 {
        let mid = &pattern[1..pattern.len() - 1];
        file_name.contains(mid)
    } else if let Some(suffix) = pattern.strip_prefix('*') {
        file_name.ends_with(suffix)
    } else if let Some(prefix) = pattern.strip_suffix('*') {
        file_name.starts_with(prefix)
    } else {
        file_name == pattern
    }
}

// ---------------------------------------------------------------------------
// find/fd handlers
// ---------------------------------------------------------------------------

fn handle_find_cmd(args: &[String], cwd: &str) -> SearchResponse {
    use crate::compat::find_compat::{parse_find_args, EntryType, glob_match, glob_match_ci};

    let mut full_args = vec!["find".to_string()];
    full_args.extend(args.iter().cloned());

    let parsed = match parse_find_args(&full_args) {
        Some(p) => p,
        None => return fallback_response("unsupported find flags"),
    };

    let search_dir = resolve_path(cwd, &parsed.search_dir);
    let canonical_dir = match search_dir.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            return SearchResponse {
                exit_code: 1,
                stdout: String::new(),
                stderr: format!("find: {}: {}", parsed.search_dir, crate::clean_io_error(&e)),
            }
        }
    };
    let entries = match walk_dir_recursive(&search_dir) {
        Ok(e) => e,
        Err(e) => {
            return SearchResponse {
                exit_code: 1,
                stdout: String::new(),
                stderr: format!("find: {}: {}", parsed.search_dir, e),
            }
        }
    };

    let mut output = String::new();
    for entry in &entries {
        if let Some(ref et) = parsed.entry_type {
            match et {
                EntryType::File => {
                    if entry.is_dir {
                        continue;
                    }
                }
                EntryType::Dir => {
                    if !entry.is_dir {
                        continue;
                    }
                }
            }
        }

        let file_name = entry
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        if let Some(ref pat) = parsed.name_pattern {
            if !glob_match(pat, &file_name) {
                continue;
            }
        }

        if let Some(ref pat) = parsed.iname_pattern {
            if !glob_match_ci(pat, &file_name) {
                continue;
            }
        }

        if let Some(ref pat) = parsed.path_pattern {
            let display = make_find_display_path(&parsed.search_dir, &canonical_dir, &entry.path);
            if !glob_match(pat, &display) {
                continue;
            }
        }

        let display_path = make_find_display_path(&parsed.search_dir, &canonical_dir, &entry.path);
        output.push_str(&display_path);
        output.push('\n');
    }

    SearchResponse {
        exit_code: 0,
        stdout: output,
        stderr: String::new(),
    }
}

fn handle_fd_cmd(args: &[String], cwd: &str) -> SearchResponse {
    use crate::compat::find_compat::{parse_fd_args, EntryType};

    let mut full_args = vec!["fd".to_string()];
    full_args.extend(args.iter().cloned());

    let parsed = match parse_fd_args(&full_args) {
        Some(p) => p,
        None => return fallback_response("unsupported fd flags"),
    };

    let search_dir = resolve_path(cwd, &parsed.search_dir);
    let canonical_dir = match search_dir.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            return SearchResponse {
                exit_code: 1,
                stdout: String::new(),
                stderr: format!("fd: {}: {}", parsed.search_dir, crate::clean_io_error(&e)),
            }
        }
    };
    let entries = match walk_dir_recursive(&search_dir) {
        Ok(e) => e,
        Err(e) => {
            return SearchResponse {
                exit_code: 1,
                stdout: String::new(),
                stderr: format!("fd: {}: {}", parsed.search_dir, e),
            }
        }
    };

    let mut output = String::new();
    let mut match_count = 0;

    for entry in &entries {
        if let Some(ref et) = parsed.entry_type {
            match et {
                EntryType::File => {
                    if entry.is_dir {
                        continue;
                    }
                }
                EntryType::Dir => {
                    if !entry.is_dir {
                        continue;
                    }
                }
            }
        }

        let file_name = entry
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        if let Some(ref ext) = parsed.extension {
            let entry_ext = entry
                .path
                .extension()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !entry_ext.eq_ignore_ascii_case(ext) {
                continue;
            }
        }

        if let Some(ref pat) = parsed.pattern {
            let matches = if parsed.fixed_strings {
                if parsed.ignore_case {
                    file_name.to_lowercase().contains(&pat.to_lowercase())
                } else {
                    file_name.contains(pat.as_str())
                }
            } else {
                let re = match regex::RegexBuilder::new(pat)
                    .case_insensitive(parsed.ignore_case)
                    .build()
                {
                    Ok(r) => r,
                    Err(_) => return fallback_response("fd: invalid regex pattern"),
                };
                re.is_match(&file_name)
            };
            if !matches {
                continue;
            }
        }

        let display_path = match entry.path.strip_prefix(&canonical_dir) {
            Ok(rel) => rel.display().to_string(),
            Err(_) => entry.path.display().to_string(),
        };

        if !display_path.is_empty() {
            output.push_str(&display_path);
            output.push('\n');
            match_count += 1;
        }
    }

    let exit_code = if match_count == 0 { 1 } else { 0 };

    SearchResponse {
        exit_code,
        stdout: output,
        stderr: String::new(),
    }
}

fn handle_cat(args: &[String], cwd: &str) -> SearchResponse {
    let mut show_line_numbers = false;
    let mut files = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-n" | "--number" => show_line_numbers = true,
            s if s.starts_with('-') => {
                return fallback_response(&format!("unsupported cat flag: {}", s));
            }
            _ => files.push(arg.clone()),
        }
    }

    if files.is_empty() {
        return fallback_response("cat: no files specified");
    }

    let mut output = String::new();
    for file in &files {
        let path = resolve_path(cwd, file);

        match crate::extract::pipeline::extract_text(&path) {
            Ok(content) => {
                if show_line_numbers {
                    for (i, line) in content.lines().enumerate() {
                        output.push_str(&format!("     {}\t{}\n", i + 1, line));
                    }
                } else {
                    output.push_str(&content);
                }
            }
            Err(e) => {
                return SearchResponse {
                    exit_code: 1,
                    stdout: output,
                    stderr: format!("cat: {}: {}\n", file, e),
                };
            }
        }
    }

    SearchResponse {
        exit_code: 0,
        stdout: output,
        stderr: String::new(),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_path(cwd: &str, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        PathBuf::from(cwd).join(path)
    }
}

fn make_find_display_path(search_dir_arg: &str, resolved_dir: &Path, entry_path: &Path) -> String {
    if Path::new(search_dir_arg).is_absolute() {
        return entry_path.display().to_string();
    }
    match entry_path.strip_prefix(resolved_dir) {
        Ok(rel) => {
            let base = search_dir_arg.trim_end_matches('/');
            if rel.as_os_str().is_empty() {
                base.to_string()
            } else {
                format!("{}/{}", base, rel.display())
            }
        }
        Err(_) => entry_path.display().to_string(),
    }
}

struct FindDirEntry {
    path: PathBuf,
    is_dir: bool,
}

fn walk_dir_recursive(root: &Path) -> Result<Vec<FindDirEntry>, String> {
    let root = root.canonicalize().map_err(|e| crate::clean_io_error(&e))?;
    let mut entries = Vec::new();
    let mut stack = vec![root.clone()];

    while let Some(dir) = stack.pop() {
        let read_dir = std::fs::read_dir(&dir).map_err(|e| crate::clean_io_error(&e))?;
        for result in read_dir {
            let entry = result.map_err(|e| crate::clean_io_error(&e))?;
            let path = entry.path();
            let is_dir = entry.file_type().is_ok_and(|ft| ft.is_dir());

            entries.push(FindDirEntry {
                path: path.clone(),
                is_dir,
            });

            if is_dir {
                stack.push(path);
            }
        }
    }

    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

// ---------------------------------------------------------------------------
// semantic output formatting
// ---------------------------------------------------------------------------

fn format_semantic_output(results: &[SearchResult]) -> String {
    // Group results by file, preserving the order from engine (sorted by RP score desc)
    let mut file_groups: Vec<(&PathBuf, f64, Vec<&SearchResult>)> = Vec::new();
    let mut seen: std::collections::HashSet<&PathBuf> = std::collections::HashSet::new();

    for r in results {
        if !seen.contains(&r.path) {
            seen.insert(&r.path);
            file_groups.push((&r.path, r.bm25_score.unwrap_or(0.0), Vec::new()));
        }
        if let Some(group) = file_groups.iter_mut().find(|(p, _, _)| *p == &r.path) {
            group.2.push(r);
        }
    }

    let mut output = String::new();
    for (path, score, lines) in &file_groups {
        // File header with score
        output.push_str(&format!("\x1b[36m[{:.4}]\x1b[0m \x1b[1;35m{}\x1b[0m\n", score, path.display()));
        // Show up to 5 matching lines per file
        for r in lines.iter().take(5) {
            output.push_str(&format!(
                "  \x1b[32m{}\x1b[0m: {}\n",
                r.line_number, r.line_content
            ));
        }
        if lines.len() > 5 {
            output.push_str(&format!("  ... and {} more matches\n", lines.len() - 5));
        }
        output.push('\n');
    }

    if !file_groups.is_empty() {
        output.push_str(&format!(
            "\x1b[33m{} files matched, ranked by semantic relevance\x1b[0m\n",
            file_groups.len()
        ));
    }

    output
}

// ---------------------------------------------------------------------------
// rg output formatting
// ---------------------------------------------------------------------------

fn format_rg_output(parsed: &RgParsedArgs, results: &[SearchResult], bm25: Bm25Mode) -> String {
    if parsed.json_output {
        return format_rg_json(results, bm25);
    }

    if parsed.count_only {
        return format_rg_count(results);
    }

    if parsed.files_only {
        return format_rg_files_only(results);
    }

    let mut output = String::new();
    let score_prefix = |r: &SearchResult| -> String {
        if bm25 != Bm25Mode::Off {
            if let Some(score) = r.bm25_score {
                return format!("[{:.2}] ", score);
            }
        }
        String::new()
    };
    for r in results {
        let path_str = r.path.display();
        let prefix = score_prefix(r);
        for (i, ctx) in r.context_before.iter().enumerate() {
            let ctx_line = r.line_number - r.context_before.len() + i;
            output.push_str(&format!("{}{}-{}-{}\n", prefix, path_str, ctx_line, ctx));
        }
        if parsed.line_numbers {
            output.push_str(&format!(
                "{}{}:{}:{}\n",
                prefix, path_str, r.line_number, r.line_content
            ));
        } else {
            output.push_str(&format!("{}{}:{}\n", prefix, path_str, r.line_content));
        }
        for (i, ctx) in r.context_after.iter().enumerate() {
            let ctx_line = r.line_number + 1 + i;
            output.push_str(&format!("{}{}-{}-{}\n", prefix, path_str, ctx_line, ctx));
        }
    }
    output
}

fn format_rg_count(results: &[SearchResult]) -> String {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for r in results {
        *counts.entry(r.path.display().to_string()).or_insert(0) += 1;
    }
    let mut output = String::new();
    for (path, count) in &counts {
        output.push_str(&format!("{}:{}\n", path, count));
    }
    output
}

fn format_rg_files_only(results: &[SearchResult]) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut output = String::new();
    for r in results {
        let path_str = r.path.display().to_string();
        if seen.insert(path_str.clone()) {
            output.push_str(&format!("{}\n", path_str));
        }
    }
    output
}

fn format_rg_json(results: &[SearchResult], bm25: Bm25Mode) -> String {
    let mut output = String::new();
    for r in results {
        let mut data = serde_json::json!({
            "path": { "text": r.path.display().to_string() },
            "lines": { "text": &r.line_content },
            "line_number": r.line_number,
            "absolute_offset": 0,
            "submatches": [{
                "match": { "text": "" },
                "start": 0,
                "end": 0,
            }],
        });
        if bm25 != Bm25Mode::Off {
            if let Some(score) = r.bm25_score {
                data["bm25_score"] = serde_json::json!(score);
            }
        }
        let json = serde_json::json!({
            "type": "match",
            "data": data,
        });
        output.push_str(&json.to_string());
        output.push('\n');
    }
    output
}

fn format_grep_output(
    parsed: &GrepParsedArgs,
    results: &[SearchResult],
    bm25: Bm25Mode,
) -> String {
    let show_filename = grep_compat::should_show_filename(parsed);

    if parsed.files_only {
        return format_grep_files_only(results);
    }

    if parsed.count_only {
        return format_grep_count(results, show_filename);
    }

    let mut output = String::new();
    for r in results {
        let path_str = r.path.display().to_string();
        let prefix = if bm25 != Bm25Mode::Off {
            if let Some(score) = r.bm25_score {
                format!("[{:.2}] ", score)
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        if show_filename && parsed.line_numbers {
            output.push_str(&format!(
                "{}{}:{}:{}\n",
                prefix, path_str, r.line_number, r.line_content
            ));
        } else if show_filename {
            output.push_str(&format!("{}{}:{}\n", prefix, path_str, r.line_content));
        } else if parsed.line_numbers {
            output.push_str(&format!("{}{}:{}\n", prefix, r.line_number, r.line_content));
        } else {
            output.push_str(&prefix);
            output.push_str(&r.line_content);
            output.push('\n');
        }
    }
    output
}

fn format_grep_files_only(results: &[SearchResult]) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut output = String::new();
    for r in results {
        let path_str = r.path.display().to_string();
        if seen.insert(path_str.clone()) {
            output.push_str(&path_str);
            output.push('\n');
        }
    }
    output
}

fn format_grep_count(results: &[SearchResult], show_filename: bool) -> String {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for r in results {
        *counts.entry(r.path.display().to_string()).or_insert(0) += 1;
    }
    let mut output = String::new();
    for (path, count) in &counts {
        if show_filename {
            output.push_str(&format!("{}:{}\n", path, count));
        } else {
            output.push_str(&format!("{}\n", count));
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_dispatch_unknown_command_returns_fallback() {
        let resp = dispatch("unknown_cmd", &[], "/tmp");
        assert_eq!(resp.exit_code, FALLBACK_EXIT_CODE);
        assert!(resp.stderr.contains("BITSCOUT_FALLBACK"));
    }

    #[test]
    fn test_dispatch_rg_basic_search() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("hello.txt"), "hello world\ngoodbye world\n").unwrap();

        let resp = dispatch(
            "rg",
            &["hello".into(), ".".into()],
            tmp.path().to_str().unwrap(),
        );
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("hello world"));
    }

    #[test]
    fn test_dispatch_rg_no_match() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("hello.txt"), "hello world\n").unwrap();

        let resp = dispatch(
            "rg",
            &["nonexistent_pattern_xyz".into(), ".".into()],
            tmp.path().to_str().unwrap(),
        );
        assert_eq!(resp.exit_code, 1);
    }

    #[test]
    fn test_dispatch_grep_basic_search() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("hello.txt"), "hello world\ngoodbye world\n").unwrap();

        let resp = dispatch(
            "grep",
            &["-r".into(), "hello".into(), ".".into()],
            tmp.path().to_str().unwrap(),
        );
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("hello world"));
    }

    #[test]
    fn test_dispatch_cat_basic() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("hello.txt"), "hello world\n").unwrap();

        let resp = dispatch(
            "cat",
            &["hello.txt".into()],
            tmp.path().to_str().unwrap(),
        );
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout, "hello world\n");
    }

    #[test]
    fn test_dispatch_find_name_glob() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::write(tmp.path().join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(tmp.path().join("README.md"), "# readme").unwrap();

        let resp = dispatch(
            "find",
            &[".".into(), "-name".into(), "*.rs".into()],
            tmp.path().to_str().unwrap(),
        );
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("main.rs"));
        assert!(!resp.stdout.contains("README.md"));
    }

    #[test]
    fn test_dispatch_fd_basic_pattern() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("main.rs"), "fn main() {}").unwrap();
        fs::write(tmp.path().join("lib.rs"), "// lib").unwrap();

        let resp = dispatch("fd", &["main".into()], tmp.path().to_str().unwrap());
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("main.rs"));
        assert!(!resp.stdout.contains("lib.rs"));
    }
}
