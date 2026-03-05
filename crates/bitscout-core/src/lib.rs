pub mod cache;
pub mod compat;
pub mod dispatch;
pub mod extract;
pub mod fs;
pub mod protocol;
pub mod search;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Search(String),
    #[error("{0}")]
    Extract(String),
}

/// Convert std::io::Error to a clean user-facing message (no OS error codes).
pub fn clean_io_error(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "No such file or directory".into(),
        std::io::ErrorKind::PermissionDenied => "Permission denied".into(),
        std::io::ErrorKind::AlreadyExists => "Already exists".into(),
        std::io::ErrorKind::NotADirectory => "Not a directory".into(),
        std::io::ErrorKind::IsADirectory => "Is a directory".into(),
        _ => {
            let s = e.to_string();
            // Strip trailing " (os error N)" if present
            if let Some(pos) = s.rfind(" (os error ") {
                s[..pos].to_string()
            } else {
                s
            }
        }
    }
}
