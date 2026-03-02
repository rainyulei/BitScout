use notify::{recommended_watcher, Event, EventKind, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum WatchEvent {
    Created(PathBuf),
    Modified(PathBuf),
    Removed(PathBuf),
}

pub struct FileWatcher {
    _inner: notify::RecommendedWatcher,
}

impl FileWatcher {
    pub fn start<F>(root: &Path, callback: F) -> Result<Self, crate::Error>
    where
        F: Fn(WatchEvent) + Send + 'static,
    {
        let mut watcher = recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                for path in event.paths {
                    let watch_event = match event.kind {
                        EventKind::Create(_) => Some(WatchEvent::Created(path)),
                        EventKind::Modify(_) => Some(WatchEvent::Modified(path)),
                        EventKind::Remove(_) => Some(WatchEvent::Removed(path)),
                        _ => None,
                    };
                    if let Some(ev) = watch_event {
                        callback(ev);
                    }
                }
            }
        })
        .map_err(|e| crate::Error::Io(e.to_string()))?;

        watcher
            .watch(root, RecursiveMode::Recursive)
            .map_err(|e| crate::Error::Io(e.to_string()))?;

        Ok(FileWatcher { _inner: watcher })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn test_watcher_detects_new_file() {
        let dir = tempfile::tempdir().unwrap();
        // Canonicalize to resolve symlinks (e.g. /var -> /private/var on macOS)
        let dir_path = dir.path().canonicalize().unwrap();
        let (tx, rx) = mpsc::channel();

        let _watcher = FileWatcher::start(&dir_path, move |ev| {
            tx.send(ev).ok();
        })
        .unwrap();

        // Give the watcher time to initialize
        std::thread::sleep(Duration::from_millis(200));

        // Create a new file
        let file_path = dir_path.join("new_file.txt");
        std::fs::write(&file_path, "hello").unwrap();

        // Wait for event — on macOS, file creation may arrive as Created or Modified
        let mut found = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(WatchEvent::Created(p)) | Ok(WatchEvent::Modified(p)) => {
                    if p == file_path {
                        found = true;
                        break;
                    }
                }
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => break,
            }
        }

        assert!(found, "Expected a Created or Modified event for the new file");
    }

    #[test]
    fn test_watcher_detects_modification() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().canonicalize().unwrap();

        // Create the file before starting the watcher
        let file_path = dir_path.join("existing.txt");
        std::fs::write(&file_path, "initial").unwrap();

        let (tx, rx) = mpsc::channel();

        let _watcher = FileWatcher::start(&dir_path, move |ev| {
            tx.send(ev).ok();
        })
        .unwrap();

        // Give the watcher time to initialize
        std::thread::sleep(Duration::from_millis(200));

        // Modify the file
        std::fs::write(&file_path, "modified content").unwrap();

        // Wait for event
        let mut found_modified = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(WatchEvent::Modified(p)) => {
                    if p == file_path {
                        found_modified = true;
                        break;
                    }
                }
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => break,
            }
        }

        assert!(
            found_modified,
            "Expected a Modified event for the existing file"
        );
    }
}
