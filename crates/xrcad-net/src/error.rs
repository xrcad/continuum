//! Error types for `xrcad-net`.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum NetError {
    #[error("session code error: {0}")]
    SessionCode(String),

    #[error("connection refused by peer: {0}")]
    ConnectionRefused(String),

    #[error("handshake failed: {0}")]
    Handshake(String),

    #[error("session ID mismatch (expected {expected}, got {got})")]
    SessionMismatch { expected: String, got: String },

    #[error("protocol version mismatch (local {local}, remote {remote})")]
    VersionMismatch { local: u8, remote: u8 },

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
