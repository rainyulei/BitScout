mod fallback;

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

// ---------------------------------------------------------------------------
// Inlined protocol types (kept minimal, no dependency on bitscout-core)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
struct SearchRequest {
    command: String,
    args: Vec<String>,
    cwd: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SearchResponse {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Serialize, Deserialize)]
enum DaemonRequest {
    Search(SearchRequest),
    Status,
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize)]
enum DaemonResponse {
    SearchResult(SearchResponse),
    Status(StatusInfo),
    Ok,
    Error(String),
}

#[derive(Debug, Serialize, Deserialize)]
struct StatusInfo {
    pid: u32,
    uptime_secs: u64,
    files_indexed: usize,
    cache_size_bytes: u64,
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // 1. Determine the command name from argv[0] / current_exe filename
    let cmd_name = command_name(&args);

    // Collect the remaining arguments (skip argv[0])
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    // 2. Try the daemon path; on ANY failure fall back to the original command
    match try_daemon(&cmd_name, &cmd_args) {
        Ok(response) => {
            // 4. Print stdout/stderr and exit with the daemon-provided exit code
            if !response.stdout.is_empty() {
                print!("{}", response.stdout);
            }
            if !response.stderr.is_empty() {
                eprint!("{}", response.stderr);
            }
            std::process::exit(response.exit_code);
        }
        Err(_) => {
            // 5. On ANY failure, fall back to the original command
            fallback::exec_original(&cmd_name, &cmd_args);
        }
    }
}

/// Extract command name from argv[0] or current_exe().
fn command_name(args: &[String]) -> String {
    // Try argv[0] first
    if let Some(arg0) = args.first() {
        if let Some(name) = std::path::Path::new(arg0).file_name() {
            if let Some(s) = name.to_str() {
                return s.to_string();
            }
        }
    }

    // Fall back to current_exe
    if let Ok(exe) = std::env::current_exe() {
        if let Some(name) = exe.file_name() {
            if let Some(s) = name.to_str() {
                return s.to_string();
            }
        }
    }

    // Last resort
    "unknown".to_string()
}

/// Try connecting to the daemon and performing the search.
fn try_daemon(cmd_name: &str, args: &[String]) -> Result<SearchResponse, Box<dyn std::error::Error>> {
    let home = std::env::var("HOME")?;
    let socket_path = std::path::PathBuf::from(&home)
        .join(".bitscout")
        .join("bitscout.sock");

    // 2. Connect to Unix socket
    let mut stream = UnixStream::connect(&socket_path)?;

    // Set a reasonable timeout so we don't hang forever
    stream.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(5)))?;

    let cwd = std::env::current_dir()?
        .to_str()
        .ok_or("non-utf8 cwd")?
        .to_string();

    // 3. Send DaemonRequest::Search
    let request = DaemonRequest::Search(SearchRequest {
        command: cmd_name.to_string(),
        args: args.to_vec(),
        cwd,
    });

    let mut payload = serde_json::to_vec(&request)?;
    payload.push(b'\n');
    stream.write_all(&payload)?;
    stream.flush()?;

    // 4. Read response (newline-delimited JSON)
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;

    let response: DaemonResponse = serde_json::from_str(line.trim())?;

    match response {
        DaemonResponse::SearchResult(sr) => Ok(sr),
        DaemonResponse::Error(e) => Err(e.into()),
        _ => Err("unexpected response from daemon".into()),
    }
}
