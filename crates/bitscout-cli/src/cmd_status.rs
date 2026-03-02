use bitscout_core::protocol::{DaemonRequest, DaemonResponse};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

/// Returns the ~/.bitscout directory path.
fn bitscout_dir() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    PathBuf::from(home).join(".bitscout")
}

pub fn execute() {
    let socket_path = bitscout_dir().join("bitscout.sock");

    let mut stream = match UnixStream::connect(&socket_path) {
        Ok(s) => s,
        Err(_) => {
            println!("BitScout daemon is not running.");
            return;
        }
    };

    // Set timeouts so we don't hang forever
    let timeout = std::time::Duration::from_secs(5);
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    // Send Status request with length-prefixed JSON (matching daemon protocol)
    let request = DaemonRequest::Status;
    let req_bytes = match serde_json::to_vec(&request) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Failed to serialize request: {}", e);
            return;
        }
    };

    let len = req_bytes.len() as u32;
    if stream.write_all(&len.to_be_bytes()).is_err()
        || stream.write_all(&req_bytes).is_err()
        || stream.flush().is_err()
    {
        eprintln!("Failed to send status request to daemon.");
        return;
    }

    // Read 4-byte big-endian length prefix
    let mut len_buf = [0u8; 4];
    if stream.read_exact(&mut len_buf).is_err() {
        eprintln!("Failed to read response from daemon.");
        return;
    }
    let resp_len = u32::from_be_bytes(len_buf) as usize;

    // Read the JSON payload
    let mut resp_buf = vec![0u8; resp_len];
    if stream.read_exact(&mut resp_buf).is_err() {
        eprintln!("Failed to read response payload from daemon.");
        return;
    }

    let response: DaemonResponse = match serde_json::from_slice(&resp_buf) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to parse daemon response: {}", e);
            return;
        }
    };

    match response {
        DaemonResponse::Status(info) => {
            let hours = info.uptime_secs / 3600;
            let mins = (info.uptime_secs % 3600) / 60;
            let secs = info.uptime_secs % 60;

            println!("BitScout daemon status:");
            println!("  PID:            {}", info.pid);
            println!("  Uptime:         {}h {}m {}s", hours, mins, secs);
            println!("  Files indexed:  {}", info.files_indexed);
            println!("  Cache size:     {} bytes", info.cache_size_bytes);
        }
        DaemonResponse::Error(e) => {
            eprintln!("Daemon returned error: {}", e);
        }
        _ => {
            eprintln!("Unexpected response from daemon.");
        }
    }
}
