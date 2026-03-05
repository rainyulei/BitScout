pub mod fs;
pub mod search;
pub mod extract;
pub mod cache;
pub mod protocol;
pub mod compat;
pub mod dispatch;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(String),
    #[error("Search error: {0}")]
    Search(String),
    #[error("Extract error: {0}")]
    Extract(String),
}
