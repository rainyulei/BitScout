use bitscout_core::fs::tree::FileTree;
use bitscout_core::fs::watcher::FileWatcher;
use bitscout_core::protocol::*;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;

pub struct DaemonServer {
    socket_path: PathBuf,
    watch_root: PathBuf,
    start_time: Instant,
    tree: Arc<RwLock<FileTree>>,
    _watcher: Option<FileWatcher>,
}

impl DaemonServer {
    pub fn new(socket_path: PathBuf, watch_root: PathBuf) -> Self {
        // Build the initial hot index
        let tree = match FileTree::scan(&watch_root) {
            Ok(t) => {
                eprintln!("Hot index: {} files indexed", t.file_count());
                t
            }
            Err(e) => {
                eprintln!("Warning: failed to scan {}: {}", watch_root.display(), e);
                // Fallback: empty tree — handlers will still work via per-request scan
                FileTree::scan(std::env::temp_dir().as_path()).unwrap_or_else(|_| {
                    panic!("cannot create fallback FileTree")
                })
            }
        };

        let tree = Arc::new(RwLock::new(tree));

        // Start FileWatcher for incremental updates
        let watcher = {
            let tree_clone = Arc::clone(&tree);
            match FileWatcher::start(&watch_root, move |event| {
                if let Ok(mut t) = tree_clone.write() {
                    t.apply_event(&event);
                }
            }) {
                Ok(w) => {
                    eprintln!("FileWatcher started for {:?}", watch_root);
                    Some(w)
                }
                Err(e) => {
                    eprintln!("Warning: FileWatcher failed to start: {}", e);
                    None
                }
            }
        };

        Self {
            socket_path,
            watch_root,
            start_time: Instant::now(),
            tree,
            _watcher: watcher,
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
            let tree = Arc::clone(&self.tree);

            tokio::spawn(async move {
                if let Err(e) = handle_connection(stream, start_time, &tree).await {
                    eprintln!("Connection error: {}", e);
                }
            });
        }
    }
}

async fn handle_connection(
    mut stream: tokio::net::UnixStream,
    start_time: Instant,
    tree: &Arc<RwLock<FileTree>>,
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
                let files_indexed = tree.read().map(|t| t.file_count()).unwrap_or(0);
                let uptime = start_time.elapsed();
                DaemonResponse::Status(StatusInfo {
                    pid: std::process::id(),
                    uptime_secs: uptime.as_secs(),
                    files_indexed,
                    cache_size_bytes: 0,
                })
            }
            DaemonRequest::Search(req) => {
                let response = bitscout_daemon::dispatch::dispatch(&req, tree);
                DaemonResponse::SearchResult(response)
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
                // files_indexed reflects the hot index (may include socket file)
                assert!(info.uptime_secs < 10);
                assert_eq!(info.cache_size_bytes, 0);
            }
            other => panic!("Expected Status response, got {:?}", other),
        }

        // Clean up
        server_handle.abort();
    }
}
