// Protocol definitions
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchRequest {
    pub command: String,       // "rg", "grep", "find", "fd"
    pub args: Vec<String>,     // original CLI args
    pub cwd: String,           // working directory
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResponse {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum DaemonRequest {
    Search(SearchRequest),
    Status,
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum DaemonResponse {
    SearchResult(SearchResponse),
    Status(StatusInfo),
    Ok,
    Error(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StatusInfo {
    pub pid: u32,
    pub uptime_secs: u64,
    pub files_indexed: usize,
    pub cache_size_bytes: u64,
}
