use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("Storage error: {message}")]
    StorageError { message: String },

    #[error("Storage critical (unrecoverable): {0}")]
    StorageCritical(String),

    #[error("Invalid vector: {0}")]
    InvalidVector(String),

    #[error("Internal error: {message}")]
    Internal { message: String },

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl EngineError {
    pub fn storage(msg: impl Into<String>) -> Self {
        Self::StorageError {
            message: msg.into(),
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal {
            message: msg.into(),
        }
    }
}

impl From<String> for EngineError {
    fn from(s: String) -> Self {
        Self::Internal { message: s }
    }
}
