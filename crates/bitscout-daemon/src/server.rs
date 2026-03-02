use bitscout_core::protocol::*;
use std::path::PathBuf;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;

pub struct DaemonServer {
    socket_path: PathBuf,
    watch_root: PathBuf,
    start_time: Instant,
}

impl DaemonServer {
    pub fn new(socket_path: PathBuf, watch_root: PathBuf) -> Self {
        Self {
            socket_path,
            watch_root,
            start_time: Instant::now(),
        }
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Remove stale socket file if it exists
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path)?;
        }

        let listener = UnixListener::bind(&self.socket_path)?;
        eprintln!(
            "Daemon listening on {:?}, watching {:?}",
            self.socket_path, self.watch_root
        );

        loop {
            let (stream, _addr) = listener.accept().await?;
            let start_time = self.start_time;
            let watch_root = self.watch_root.clone();

            tokio::spawn(async move {
                if let Err(e) = handle_connection(stream, start_time, &watch_root).await {
                    eprintln!("Connection error: {}", e);
                }
            });
        }
    }
}

async fn handle_connection(
    mut stream: tokio::net::UnixStream,
    start_time: Instant,
    _watch_root: &PathBuf,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    loop {
        // Read 4-byte big-endian length prefix
        let len = match stream.read_u32().await {
            Ok(len) => len as usize,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                // Client disconnected
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };

        // Read the JSON payload
        let mut buf = vec![0u8; len];
        stream.read_exact(&mut buf).await?;

        let request: DaemonRequest = serde_json::from_slice(&buf)?;

        let response = match request {
            DaemonRequest::Status => {
                let uptime = start_time.elapsed();
                DaemonResponse::Status(StatusInfo {
                    pid: std::process::id(),
                    uptime_secs: uptime.as_secs(),
                    files_indexed: 0,
                    cache_size_bytes: 0,
                })
            }
            DaemonRequest::Search(_) => {
                DaemonResponse::Error("search not yet implemented".to_string())
            }
            DaemonRequest::Shutdown => {
                // Send Ok response before shutting down
                let resp_bytes = serde_json::to_vec(&DaemonResponse::Ok)?;
                stream.write_u32(resp_bytes.len() as u32).await?;
                stream.write_all(&resp_bytes).await?;
                stream.flush().await?;
                return Ok(());
            }
        };

        // Write length-prefixed JSON response
        let resp_bytes = serde_json::to_vec(&response)?;
        stream.write_u32(resp_bytes.len() as u32).await?;
        stream.write_all(&resp_bytes).await?;
        stream.flush().await?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::UnixStream;

    #[tokio::test]
    async fn test_daemon_responds_to_status() {
        // Use a temp file path for the socket
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");
        let watch_root = dir.path().to_path_buf();

        let server = DaemonServer::new(socket_path.clone(), watch_root);

        // Spawn the server
        let server_handle = tokio::spawn(async move {
            server.run().await.unwrap();
        });

        // Give the server a moment to bind
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Connect and send a Status request
        let mut stream = UnixStream::connect(&socket_path).await.unwrap();

        let request = DaemonRequest::Status;
        let req_bytes = serde_json::to_vec(&request).unwrap();
        stream.write_u32(req_bytes.len() as u32).await.unwrap();
        stream.write_all(&req_bytes).await.unwrap();
        stream.flush().await.unwrap();

        // Read the response
        let resp_len = stream.read_u32().await.unwrap() as usize;
        let mut resp_buf = vec![0u8; resp_len];
        stream.read_exact(&mut resp_buf).await.unwrap();

        let response: DaemonResponse = serde_json::from_slice(&resp_buf).unwrap();

        match response {
            DaemonResponse::Status(info) => {
                assert_eq!(info.pid, std::process::id());
                assert_eq!(info.files_indexed, 0);
                assert_eq!(info.cache_size_bytes, 0);
            }
            other => panic!("Expected Status response, got {:?}", other),
        }

        // Clean up
        server_handle.abort();
    }
}
