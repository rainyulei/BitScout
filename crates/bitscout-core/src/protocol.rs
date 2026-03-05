use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResponse {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}
