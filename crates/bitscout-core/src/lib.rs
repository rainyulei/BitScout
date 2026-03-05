pub mod fs;
pub mod search;
pub mod extract;
pub mod cache;
pub mod protocol;
pub mod compat;
pub mod dispatch;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Search(String),
    #[error("{0}")]
    Extract(String),
}
