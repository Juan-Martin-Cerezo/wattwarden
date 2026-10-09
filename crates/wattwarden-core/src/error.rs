use std::io;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum WattWardenError {
    #[error("I/O failure on path '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("General I/O error: {0}")]
    GeneralIo(#[from] io::Error),

    #[error("Failed to parse integer value '{val}' from '{path}': {source}")]
    ParseInt {
        path: PathBuf,
        val: String,
        #[source]
        source: std::num::ParseIntError,
    },

    #[error("Failed to parse float value '{val}' from '{path}': {source}")]
    ParseFloat {
        path: PathBuf,
        val: String,
        #[source]
        source: std::num::ParseFloatError,
    },

    #[error("Hardware interface not found: {0}")]
    InterfaceNotFound(String),

    #[error("Permission denied: root/administrator privileges required for: {0}")]
    PermissionDenied(String),

    #[error("Value out of hardware bounds: {value} is not in range [{min}, {max}]")]
    OutOfBounds { value: u64, min: u64, max: u64 },

    #[error("Unsupported hardware feature: {0}")]
    Unsupported(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("IPC connection error: {0}")]
    Ipc(String),

    #[error("JSON serialization/deserialization error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, WattWardenError>;
