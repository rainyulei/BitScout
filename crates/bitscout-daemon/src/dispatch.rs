use bitscout_core::fs::tree::FileTree;
use bitscout_core::protocol::{SearchRequest, SearchResponse};
use bitscout_core::search::bm25::Bm25Mode;
use bitscout_core::search::engine::{SearchEngine, SearchOptions, SearchResult};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::grep_compat::{self, GrepParsedArgs};
use crate::rg_compat::RgParsedArgs;

/// Special exit code that tells the shim to fall back to the original command.
pub const FALLBACK_EXIT_CODE: i32 = 200;

pub fn dispatch(req: &SearchRequest, tree: &Arc<RwLock<FileTree>>) -> SearchResponse {
    match req.command.as_str() {
        "rg" => handle_rg(req, tree),
        "grep" => handle_grep(req, tree),
        "find" | "fd" => handle_find(req),
        "cat" => handle_cat(req),
        _ => fallback_response(&format!("unknown command: {}", req.command)),
    }
}

fn fallback_response(reason: &str) -> SearchResponse {
    SearchResponse {
        exit_code: FALLBACK_EXIT_CODE,
        stdout: String::new(),
        stderr: format!("BITSCOUT_FALLBACK: {}", reason),
    }
}

fn handle_rg(req: &SearchRequest, tree: &Arc<RwLock<FileTree>>) -> SearchResponse {
    // Build args with command name prepended (parser expects argv[0])
    let mut full_args = vec![req.command.clone()];
    full_args.extend(req.args.iter().cloned());

    let parsed = match crate::rg_compat::parse_rg_args(&full_args) {
        Some(p) => p,
        None => return fallback_response("unsupported rg flags"),
    };

    // Resolve search path relative to cwd
    let search_path = if Path::new(&parsed.path).is_absolute() {
        PathBuf::from(&parsed.path)
    } else {
        PathBuf::from(&req.cwd).join(&parsed.path)
    };

    let use_regex = !parsed.fixed_strings;

    let results = if search_path.is_file() {
        // Single file search: read and match directly
        let text = match bitscout_core::extract::pipeline::extract_text(&search_path) {
            Ok(t) => t,
            Err(e) => {
                return SearchResponse {
                    exit_code: 2,
                    stdout: String::new(),
                    stderr: format!("rg: {}: {}", search_path.display(), e),
                }
            }
        };
        let matcher = match bitscout_core::search::matcher::Matcher::with_options(
            &[&parsed.pattern],
            bitscout_core::search::matcher::MatchOptions {
                case_insensitive: parsed.case_insensitive,
                use_regex,
            },
        ) {
            Ok(m) => m,
            Err(e) => {
                return SearchResponse {
                    exit_code: 2,
                    stdout: String::new(),
                    stderr: format!("rg: {}", e),
                }
            }
        };
        let bm25_score = if parsed.bm25 != Bm25Mode::Off {
            let tf = matcher.find_all(text.as_bytes()).len();
            let doc_len = text.len();
            let scorer = bitscout_core::search::bm25::Bm25Scorer::new(1, doc_len as f64);
            Some(scorer.tf_score(tf, doc_len))
        } else {
            None
        };
        let lines: Vec<&str> = text.lines().collect();
        let context = parsed.before_context.max(parsed.after_context);
        let mut file_results = Vec::new();
        for (idx, line) in lines.iter().enumerate() {
            if matcher.is_match(line.as_bytes()) {
                let before = if context > 0 {
                    lines[idx.saturating_sub(context)..idx]
                        .iter().map(|s| s.to_string()).collect()
                } else { Vec::new() };
                let after = if context > 0 {
                    lines[idx + 1..lines.len().min(idx + 1 + context)]
                        .iter().map(|s| s.to_string()).collect()
                } else { Vec::new() };
                file_results.push(SearchResult {
                    path: search_path.clone(),
                    line_number: idx + 1,
                    line_content: line.to_string(),
                    context_before: before,
                    context_after: after,
                    bm25_score,
                });
            }
        }
        file_results
    } else {
        // Directory search — use the hot index
        let engine = {
            let t = tree.read().unwrap();
            SearchEngine::from_tree(t.clone())
        };

        let opts = SearchOptions {
            case_insensitive: parsed.case_insensitive,
            context_lines: parsed.before_context.max(parsed.after_context),
            max_results: 100_000,
            use_regex,
            bm25: parsed.bm25,
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
        return SearchResponse {
            exit_code: 1,
            stdout: String::new(),
            stderr: String::new(),
        };
    }

    let stdout = format_rg_output(&parsed, &results, parsed.bm25);

    SearchResponse {
        exit_code: 0,
        stdout,
        stderr: String::new(),
    }
}

fn handle_grep(req: &SearchRequest, tree: &Arc<RwLock<FileTree>>) -> SearchResponse {
    // Build args with command name prepended (parser expects argv[0])
    let mut full_args = vec![req.command.clone()];
    full_args.extend(req.args.iter().cloned());

    let parsed = match grep_compat::parse_grep_args(&full_args) {
        Some(p) => p,
        None => return fallback_response("unsupported grep flags"),
    };

    // Build the effective pattern: wrap with word boundaries if -w
    // When -w is used, we need regex mode for \b boundaries
    let use_regex = !parsed.fixed_strings || parsed.word_regexp;
    let effective_pattern = if parsed.word_regexp {
        if parsed.fixed_strings {
            // -F -w: escape the literal pattern, then wrap with \b
            format!(r"\b{}\b", regex::escape(&parsed.pattern))
        } else {
            format!(r"\b(?:{})\b", parsed.pattern)
        }
    } else {
        parsed.pattern.clone()
    };

    // Search each path and collect results
    let mut all_results: Vec<SearchResult> = Vec::new();
    for path_str in &parsed.paths {
        let search_path = if Path::new(path_str).is_absolute() {
            PathBuf::from(path_str)
        } else {
            PathBuf::from(&req.cwd).join(path_str)
        };

        if search_path.is_file() {
            // Single file search: read and match directly
            let text = match bitscout_core::extract::pipeline::extract_text(&search_path) {
                Ok(t) => t,
                Err(e) => {
                    return SearchResponse {
                        exit_code: 2,
                        stdout: String::new(),
                        stderr: format!("grep: {}: {}", search_path.display(), e),
                    }
                }
            };
            let matcher = match bitscout_core::search::matcher::Matcher::with_options(
                &[&effective_pattern],
                bitscout_core::search::matcher::MatchOptions {
                    case_insensitive: parsed.case_insensitive,
                    use_regex,
                },
            ) {
                Ok(m) => m,
                Err(e) => {
                    return SearchResponse {
                        exit_code: 2,
                        stdout: String::new(),
                        stderr: format!("grep: {}", e),
                    }
                }
            };
            // Compute BM25 score for single file
            let bm25_score = if parsed.bm25 != Bm25Mode::Off {
                let tf = matcher.find_all(text.as_bytes()).len();
                let doc_len = text.len();
                let scorer = bitscout_core::search::bm25::Bm25Scorer::new(1, doc_len as f64);
                Some(scorer.tf_score(tf, doc_len))
            } else {
                None
            };
            for (idx, line) in text.lines().enumerate() {
                if matcher.is_match(line.as_bytes()) {
                    all_results.push(SearchResult {
                        path: search_path.clone(),
                        line_number: idx + 1,
                        line_content: line.to_string(),
                        context_before: Vec::new(),
                        context_after: Vec::new(),
                        bm25_score,
                    });
                }
            }
        } else {
            // Directory search — use the hot index
            let engine = {
                let t = tree.read().unwrap();
                SearchEngine::from_tree(t.clone())
            };

            let opts = SearchOptions {
                case_insensitive: parsed.case_insensitive,
                context_lines: 0,
                max_results: 100_000,
                use_regex,
                bm25: parsed.bm25,
                search_root: Some(search_path.clone()),
            };

            match engine.search(&effective_pattern, &opts) {
                Ok(mut results) => {
                    // Filter by --include glob if specified
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

/// Simple glob matching for --include patterns (e.g., "*.rs", "*.py").
/// Matches against the file name component only.
fn match_glob(pattern: &str, path: &Path) -> bool {
    let file_name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };

    // Convert simple glob to a check:
    // *.ext => ends with .ext
    // prefix* => starts with prefix
    // *mid* => contains mid
    // exact => exact match
    if pattern.starts_with('*') && pattern.ends_with('*') && pattern.len() > 2 {
        let mid = &pattern[1..pattern.len() - 1];
        file_name.contains(mid)
    } else if pattern.starts_with('*') {
        let suffix = &pattern[1..];
        file_name.ends_with(suffix)
    } else if pattern.ends_with('*') {
        let prefix = &pattern[..pattern.len() - 1];
        file_name.starts_with(prefix)
    } else {
        file_name == pattern
    }
}

fn format_grep_output(parsed: &GrepParsedArgs, results: &[SearchResult], bm25: Bm25Mode) -> String {
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
            output.push_str(&format!("{}{}:{}:{}\n", prefix, path_str, r.line_number, r.line_content));
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

fn handle_find(req: &SearchRequest) -> SearchResponse {
    let mut full_args = vec![req.command.clone()];
    full_args.extend(req.args.iter().cloned());

    match req.command.as_str() {
        "find" => handle_find_cmd(req, &full_args),
        "fd" => handle_fd_cmd(req, &full_args),
        _ => fallback_response("unexpected command in handle_find"),
    }
}

fn handle_find_cmd(req: &SearchRequest, full_args: &[String]) -> SearchResponse {
    use crate::find_compat::{parse_find_args, EntryType, glob_match, glob_match_ci};

    let parsed = match parse_find_args(full_args) {
        Some(p) => p,
        None => return fallback_response("unsupported find flags"),
    };

    let search_dir = resolve_find_path(&req.cwd, &parsed.search_dir);
    let canonical_dir = match search_dir.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            return SearchResponse {
                exit_code: 1,
                stdout: String::new(),
                stderr: format!("find: {}: {}", parsed.search_dir, e),
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
        // Type filter
        if let Some(ref et) = parsed.entry_type {
            match et {
                EntryType::File => {
                    if entry.is_dir { continue; }
                }
                EntryType::Dir => {
                    if !entry.is_dir { continue; }
                }
            }
        }

        let file_name = entry.path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        // -name filter (glob on filename)
        if let Some(ref pat) = parsed.name_pattern {
            if !glob_match(pat, &file_name) {
                continue;
            }
        }

        // -iname filter (case-insensitive glob on filename)
        if let Some(ref pat) = parsed.iname_pattern {
            if !glob_match_ci(pat, &file_name) {
                continue;
            }
        }

        // -path filter (glob on full display path)
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

fn handle_fd_cmd(req: &SearchRequest, full_args: &[String]) -> SearchResponse {
    use crate::find_compat::{parse_fd_args, EntryType};

    let parsed = match parse_fd_args(full_args) {
        Some(p) => p,
        None => return fallback_response("unsupported fd flags"),
    };

    let search_dir = resolve_find_path(&req.cwd, &parsed.search_dir);
    let canonical_dir = match search_dir.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            return SearchResponse {
                exit_code: 1,
                stdout: String::new(),
                stderr: format!("fd: {}: {}", parsed.search_dir, e),
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
        // Type filter
        if let Some(ref et) = parsed.entry_type {
            match et {
                EntryType::File => {
                    if entry.is_dir { continue; }
                }
                EntryType::Dir => {
                    if !entry.is_dir { continue; }
                }
            }
        }

        let file_name = entry.path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        // Extension filter
        if let Some(ref ext) = parsed.extension {
            let entry_ext = entry.path.extension()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !entry_ext.eq_ignore_ascii_case(ext) {
                continue;
            }
        }

        // Pattern filter (regex on filename, or literal if -F)
        if let Some(ref pat) = parsed.pattern {
            let matches = if parsed.fixed_strings {
                // Literal substring match
                if parsed.ignore_case {
                    file_name.to_lowercase().contains(&pat.to_lowercase())
                } else {
                    file_name.contains(pat.as_str())
                }
            } else {
                // Real regex match
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

        // fd outputs relative to search dir by default
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

    // fd returns exit code 1 if no matches
    let exit_code = if match_count == 0 { 1 } else { 0 };

    SearchResponse {
        exit_code,
        stdout: output,
        stderr: String::new(),
    }
}

// ---------------------------------------------------------------------------
// find/fd helpers
// ---------------------------------------------------------------------------

fn resolve_find_path(cwd: &str, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        PathBuf::from(cwd).join(path)
    }
}

/// Build a display path for find output that mirrors what the real find does:
/// paths are shown relative to the given search_dir argument.
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

/// Recursively walk a directory, collecting all entries (files and dirs).
fn walk_dir_recursive(root: &Path) -> Result<Vec<FindDirEntry>, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    let mut stack = vec![root.clone()];

    while let Some(dir) = stack.pop() {
        let read_dir = std::fs::read_dir(&dir).map_err(|e| e.to_string())?;
        for result in read_dir {
            let entry = result.map_err(|e| e.to_string())?;
            let path = entry.path();
            let is_dir = entry.file_type().map_or(false, |ft| ft.is_dir());

            entries.push(FindDirEntry {
                path: path.clone(),
                is_dir,
            });

            if is_dir {
                stack.push(path);
            }
        }
    }

    // Sort for deterministic output
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

fn handle_cat(req: &SearchRequest) -> SearchResponse {
    // cat args are just file paths (may have -n for line numbers)
    let mut show_line_numbers = false;
    let mut files = Vec::new();

    for arg in &req.args {
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
        // Resolve relative to cwd
        let path = if Path::new(file).is_absolute() {
            PathBuf::from(file)
        } else {
            PathBuf::from(&req.cwd).join(file)
        };

        // Use extract_text for binary format support
        match bitscout_core::extract::pipeline::extract_text(&path) {
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

    // Default: line-by-line output like rg
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
        // Context before
        for (i, ctx) in r.context_before.iter().enumerate() {
            let ctx_line = r.line_number - r.context_before.len() + i;
            output.push_str(&format!("{}{}-{}-{}\n", prefix, path_str, ctx_line, ctx));
        }
        // Match line
        if parsed.line_numbers {
            output.push_str(&format!("{}{}:{}:{}\n", prefix, path_str, r.line_number, r.line_content));
        } else {
            output.push_str(&format!("{}{}:{}\n", prefix, path_str, r.line_content));
        }
        // Context after
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
    // rg JSON output format: one JSON object per line
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Build a shared FileTree from a directory for testing.
    fn test_tree(root: &Path) -> Arc<RwLock<FileTree>> {
        Arc::new(RwLock::new(FileTree::scan(root).unwrap()))
    }

    /// Build a dummy tree (for tests that don't need real files).
    fn dummy_tree() -> Arc<RwLock<FileTree>> {
        let tmp = TempDir::new().unwrap();
        // Leak the TempDir so it lives long enough (tests are short-lived)
        let path = tmp.path().to_path_buf();
        std::mem::forget(tmp);
        Arc::new(RwLock::new(FileTree::scan(&path).unwrap()))
    }

    #[test]
    fn test_dispatch_unknown_command_returns_fallback() {
        let tree = dummy_tree();
        let req = SearchRequest {
            command: "unknown_cmd".into(),
            args: vec![],
            cwd: "/tmp".into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, FALLBACK_EXIT_CODE);
        assert!(resp.stderr.contains("BITSCOUT_FALLBACK"));
    }

    #[test]
    fn test_dispatch_grep_unsupported_flags_returns_fallback() {
        let tree = dummy_tree();
        let req = SearchRequest {
            command: "grep".into(),
            args: vec!["-P".into(), "pattern".into()],
            cwd: "/tmp".into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, FALLBACK_EXIT_CODE);
        assert!(resp.stderr.contains("unsupported grep flags"));
    }

    #[test]
    fn test_dispatch_rg_unsupported_flags_returns_fallback() {
        let tree = dummy_tree();
        let req = SearchRequest {
            command: "rg".into(),
            args: vec!["--pcre2".into(), "pattern".into()],
            cwd: "/tmp".into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, FALLBACK_EXIT_CODE);
        assert!(resp.stderr.contains("unsupported rg flags"));
    }

    #[test]
    fn test_dispatch_rg_basic_search() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("hello.txt"), "hello world\ngoodbye world\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "rg".into(),
            args: vec!["hello".into(), ".".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("hello world"));
    }

    #[test]
    fn test_dispatch_rg_no_match() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("hello.txt"), "hello world\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "rg".into(),
            args: vec!["nonexistent_pattern_xyz".into(), ".".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 1);
        assert!(resp.stdout.is_empty());
    }

    #[test]
    fn test_dispatch_rg_count_only() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("data.txt"), "foo\nbar\nfoo\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "rg".into(),
            args: vec!["-c".into(), "foo".into(), ".".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains(":2"));
    }

    #[test]
    fn test_dispatch_rg_files_only() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.txt"), "match\n").unwrap();
        fs::write(tmp.path().join("b.txt"), "match\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "rg".into(),
            args: vec!["-l".into(), "match".into(), ".".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        // Should list file paths, each on its own line
        let lines: Vec<&str> = resp.stdout.trim().lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_dispatch_rg_line_numbers() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("num.txt"), "aaa\nbbb\nccc\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "rg".into(),
            args: vec!["-n".into(), "bbb".into(), ".".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        // Should contain path:line_number:content
        assert!(resp.stdout.contains(":2:bbb"));
    }

    #[test]
    fn test_dispatch_cat_basic() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("hello.txt"), "hello world\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "cat".into(),
            args: vec!["hello.txt".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout, "hello world\n");
        assert!(resp.stderr.is_empty());
    }

    #[test]
    fn test_dispatch_cat_line_numbers() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("lines.txt"), "aaa\nbbb\nccc\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "cat".into(),
            args: vec!["-n".into(), "lines.txt".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("1\taaa"));
        assert!(resp.stdout.contains("2\tbbb"));
        assert!(resp.stdout.contains("3\tccc"));
    }

    #[test]
    fn test_dispatch_cat_multiple_files() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.txt"), "file a\n").unwrap();
        fs::write(tmp.path().join("b.txt"), "file b\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "cat".into(),
            args: vec!["a.txt".into(), "b.txt".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout, "file a\nfile b\n");
    }

    #[test]
    fn test_dispatch_cat_no_files() {
        let tree = dummy_tree();
        let req = SearchRequest {
            command: "cat".into(),
            args: vec![],
            cwd: "/tmp".into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, FALLBACK_EXIT_CODE);
        assert!(resp.stderr.contains("no files specified"));
    }

    #[test]
    fn test_dispatch_cat_unsupported_flag() {
        let tree = dummy_tree();
        let req = SearchRequest {
            command: "cat".into(),
            args: vec!["-v".into(), "file.txt".into()],
            cwd: "/tmp".into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, FALLBACK_EXIT_CODE);
        assert!(resp.stderr.contains("unsupported cat flag"));
    }

    #[test]
    fn test_dispatch_cat_nonexistent_file() {
        let tmp = TempDir::new().unwrap();
        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "cat".into(),
            args: vec!["nonexistent.txt".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 1);
        assert!(resp.stderr.contains("nonexistent.txt"));
    }

    #[test]
    fn test_dispatch_cat_absolute_path() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("abs.txt");
        fs::write(&file_path, "absolute content\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "cat".into(),
            args: vec![file_path.to_str().unwrap().into()],
            cwd: "/tmp".into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout, "absolute content\n");
    }

    #[test]
    fn test_dispatch_cat_preserves_no_trailing_newline() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("no_newline.txt"), "no trailing newline").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "cat".into(),
            args: vec!["no_newline.txt".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        // Match real cat behavior: preserve exact content
        assert_eq!(resp.stdout, "no trailing newline");
    }

    #[test]
    fn test_dispatch_rg_json_output() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("j.txt"), "target_line\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "rg".into(),
            args: vec!["--json".into(), "target_line".into(), ".".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        // Each line should be valid JSON
        for line in resp.stdout.trim().lines() {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(v["type"], "match");
        }
    }

    // -----------------------------------------------------------------------
    // grep handler tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_dispatch_grep_basic_search() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("hello.txt"), "hello world\ngoodbye world\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "grep".into(),
            args: vec!["-r".into(), "hello".into(), ".".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("hello world"));
    }

    #[test]
    fn test_dispatch_grep_no_match() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("hello.txt"), "hello world\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "grep".into(),
            args: vec!["-r".into(), "nonexistent_xyz".into(), ".".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 1);
        assert!(resp.stdout.is_empty());
    }

    #[test]
    fn test_dispatch_grep_line_numbers() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("num.txt"), "aaa\nbbb\nccc\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "grep".into(),
            args: vec!["-rn".into(), "bbb".into(), ".".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        // Should contain filename:line_number:content
        assert!(resp.stdout.contains(":2:bbb"));
    }

    #[test]
    fn test_dispatch_grep_case_insensitive() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("case.txt"), "Hello World\nhello world\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "grep".into(),
            args: vec!["-ri".into(), "HELLO".into(), ".".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        // Both lines should match
        let lines: Vec<&str> = resp.stdout.trim().lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_dispatch_grep_files_only() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.txt"), "match line\n").unwrap();
        fs::write(tmp.path().join("b.txt"), "match line\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "grep".into(),
            args: vec!["-rl".into(), "match".into(), ".".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        let lines: Vec<&str> = resp.stdout.trim().lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_dispatch_grep_count() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("data.txt"), "foo\nbar\nfoo\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "grep".into(),
            args: vec!["-rc".into(), "foo".into(), ".".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains(":2"));
    }

    #[test]
    fn test_dispatch_grep_no_filename() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("test.txt"), "hello\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "grep".into(),
            args: vec!["-rh".into(), "hello".into(), ".".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        // Should not contain any path separator
        assert_eq!(resp.stdout.trim(), "hello");
    }

    #[test]
    fn test_dispatch_grep_include_glob() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("code.rs"), "fn main() {}\n").unwrap();
        fs::write(tmp.path().join("notes.txt"), "fn notes\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "grep".into(),
            args: vec![
                "-r".into(),
                "--include=*.rs".into(),
                "fn".into(),
                ".".into(),
            ],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        // Should only match the .rs file
        assert!(resp.stdout.contains("code.rs"));
        assert!(!resp.stdout.contains("notes.txt"));
    }

    #[test]
    fn test_dispatch_grep_default_output_has_filename() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("file.txt"), "target\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "grep".into(),
            args: vec!["-r".into(), "target".into(), ".".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        // Default output: filename:content
        assert!(resp.stdout.contains("file.txt:target"));
    }

    #[test]
    fn test_match_glob_star_ext() {
        assert!(match_glob("*.rs", Path::new("src/main.rs")));
        assert!(!match_glob("*.rs", Path::new("src/main.py")));
    }

    #[test]
    fn test_match_glob_prefix_star() {
        assert!(match_glob("test_*", Path::new("tests/test_foo.rs")));
        assert!(!match_glob("test_*", Path::new("tests/main.rs")));
    }

    #[test]
    fn test_match_glob_contains() {
        assert!(match_glob("*spec*", Path::new("tests/my_spec_file.rs")));
        assert!(!match_glob("*spec*", Path::new("tests/main.rs")));
    }

    #[test]
    fn test_match_glob_exact() {
        assert!(match_glob("Makefile", Path::new("project/Makefile")));
        assert!(!match_glob("Makefile", Path::new("project/makefile")));
    }

    // -----------------------------------------------------------------------
    // find handler tests
    // -----------------------------------------------------------------------

    fn create_find_test_tree(tmp: &TempDir) {
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::create_dir_all(tmp.path().join("tests")).unwrap();
        fs::write(tmp.path().join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(tmp.path().join("src/lib.rs"), "// lib").unwrap();
        fs::write(tmp.path().join("tests/test_one.rs"), "// test").unwrap();
        fs::write(tmp.path().join("README.md"), "# readme").unwrap();
        fs::write(tmp.path().join("data.json"), "{}").unwrap();
    }

    #[test]
    fn test_find_all_files() {
        let tmp = TempDir::new().unwrap();
        create_find_test_tree(&tmp);

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "find".into(),
            args: vec![".".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        // Should contain all files and dirs
        assert!(resp.stdout.contains("main.rs"));
        assert!(resp.stdout.contains("README.md"));
        assert!(resp.stdout.contains("src"));
    }

    #[test]
    fn test_find_name_glob() {
        let tmp = TempDir::new().unwrap();
        create_find_test_tree(&tmp);

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "find".into(),
            args: vec![".".into(), "-name".into(), "*.rs".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("main.rs"));
        assert!(resp.stdout.contains("lib.rs"));
        assert!(resp.stdout.contains("test_one.rs"));
        assert!(!resp.stdout.contains("README.md"));
        assert!(!resp.stdout.contains("data.json"));
    }

    #[test]
    fn test_find_iname_case_insensitive() {
        let tmp = TempDir::new().unwrap();
        create_find_test_tree(&tmp);

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "find".into(),
            args: vec![".".into(), "-iname".into(), "readme*".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("README.md"));
    }

    #[test]
    fn test_find_type_f() {
        let tmp = TempDir::new().unwrap();
        create_find_test_tree(&tmp);

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "find".into(),
            args: vec![".".into(), "-type".into(), "f".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        // Should not contain directory entries
        let lines: Vec<&str> = resp.stdout.trim().lines().collect();
        for line in &lines {
            assert!(!line.ends_with("/src") && !line.ends_with("/tests"),
                "found directory in -type f output: {}", line);
        }
        assert!(resp.stdout.contains("main.rs"));
    }

    #[test]
    fn test_find_type_d() {
        let tmp = TempDir::new().unwrap();
        create_find_test_tree(&tmp);

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "find".into(),
            args: vec![".".into(), "-type".into(), "d".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("src"));
        assert!(resp.stdout.contains("tests"));
        // Should not contain files
        assert!(!resp.stdout.contains("main.rs"));
    }

    #[test]
    fn test_find_combined_name_and_type() {
        let tmp = TempDir::new().unwrap();
        create_find_test_tree(&tmp);

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "find".into(),
            args: vec![".".into(), "-name".into(), "*.rs".into(), "-type".into(), "f".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("main.rs"));
        assert!(!resp.stdout.contains("README.md"));
    }

    #[test]
    fn test_find_unsupported_flag_fallback() {
        let tree = dummy_tree();
        let req = SearchRequest {
            command: "find".into(),
            args: vec![".".into(), "-maxdepth".into(), "2".into()],
            cwd: "/tmp".into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, FALLBACK_EXIT_CODE);
        assert!(resp.stderr.contains("unsupported find flags"));
    }

    #[test]
    fn test_find_nonexistent_dir() {
        let tmp = TempDir::new().unwrap();
        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "find".into(),
            args: vec!["nonexistent_dir_xyz".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 1);
    }

    // -----------------------------------------------------------------------
    // fd handler tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_fd_basic_pattern() {
        let tmp = TempDir::new().unwrap();
        create_find_test_tree(&tmp);

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "fd".into(),
            args: vec!["main".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("main.rs"));
        assert!(!resp.stdout.contains("lib.rs"));
    }

    #[test]
    fn test_fd_extension_filter() {
        let tmp = TempDir::new().unwrap();
        create_find_test_tree(&tmp);

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "fd".into(),
            args: vec!["-e".into(), "rs".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("main.rs"));
        assert!(resp.stdout.contains("lib.rs"));
        assert!(!resp.stdout.contains("README.md"));
        assert!(!resp.stdout.contains("data.json"));
    }

    #[test]
    fn test_fd_type_f() {
        let tmp = TempDir::new().unwrap();
        create_find_test_tree(&tmp);

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "fd".into(),
            args: vec!["-t".into(), "f".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("main.rs"));
        // Should not contain directories
        let lines: Vec<&str> = resp.stdout.trim().lines().collect();
        for line in &lines {
            assert!(line.contains('.'), "unexpected directory in output: {}", line);
        }
    }

    #[test]
    fn test_fd_type_d() {
        let tmp = TempDir::new().unwrap();
        create_find_test_tree(&tmp);

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "fd".into(),
            args: vec!["-t".into(), "d".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("src"));
        assert!(resp.stdout.contains("tests"));
        assert!(!resp.stdout.contains("main.rs"));
    }

    #[test]
    fn test_fd_ignore_case() {
        let tmp = TempDir::new().unwrap();
        create_find_test_tree(&tmp);

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "fd".into(),
            args: vec!["-i".into(), "readme".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("README.md"));
    }

    #[test]
    fn test_fd_no_match_returns_exit_1() {
        let tmp = TempDir::new().unwrap();
        create_find_test_tree(&tmp);

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "fd".into(),
            args: vec!["zzz_nonexistent".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 1);
        assert!(resp.stdout.is_empty());
    }

    #[test]
    fn test_fd_unsupported_flag_fallback() {
        let tree = dummy_tree();
        let req = SearchRequest {
            command: "fd".into(),
            args: vec!["--hidden".into(), "pattern".into()],
            cwd: "/tmp".into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, FALLBACK_EXIT_CODE);
        assert!(resp.stderr.contains("unsupported fd flags"));
    }

    #[test]
    fn test_fd_combined_extension_and_pattern() {
        let tmp = TempDir::new().unwrap();
        create_find_test_tree(&tmp);

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "fd".into(),
            args: vec!["-e".into(), "rs".into(), "test".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("test_one.rs"));
        assert!(!resp.stdout.contains("main.rs"));
    }

    #[test]
    fn test_fd_relative_output() {
        let tmp = TempDir::new().unwrap();
        create_find_test_tree(&tmp);

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "fd".into(),
            args: vec!["main".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        // Output should be relative path, not absolute
        let line = resp.stdout.trim();
        assert!(!line.starts_with('/'), "expected relative path, got: {}", line);
        assert!(line.contains("main.rs"));
    }

    #[test]
    fn test_rg_bm25_output_has_scores() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.rs"), "hello world\nhello again\n").unwrap();
        fs::write(tmp.path().join("b.rs"), "just hello\nsome other text\nmore padding\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "rg".into(),
            args: vec!["--bm25".into(), "hello".into(), ".".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        for line in resp.stdout.lines() {
            assert!(line.starts_with('['), "line should start with [score]: {}", line);
            assert!(line.contains(']'), "line should contain ]: {}", line);
        }
    }

    #[test]
    fn test_rg_without_bm25_no_scores() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.rs"), "hello world\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "rg".into(),
            args: vec!["hello".into(), ".".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        assert!(!resp.stdout.starts_with('['), "should not have [score] prefix without --bm25");
    }

    #[test]
    fn test_rg_bm25_json_has_score_field() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.rs"), "hello world\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "rg".into(),
            args: vec!["--json".into(), "--bm25".into(), "hello".into(), ".".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("bm25_score"), "JSON should contain bm25_score field");
    }

    #[test]
    fn test_grep_bm25_output_has_scores() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.txt"), "hello world\nhello again\n").unwrap();

        let tree = test_tree(tmp.path());
        let req = SearchRequest {
            command: "grep".into(),
            args: vec!["-r".into(), "--bm25".into(), "hello".into(), ".".into()],
            cwd: tmp.path().to_str().unwrap().into(),
        };
        let resp = dispatch(&req, &tree);
        assert_eq!(resp.exit_code, 0);
        for line in resp.stdout.lines() {
            assert!(line.starts_with('['), "line should start with [score]: {}", line);
        }
    }
}
