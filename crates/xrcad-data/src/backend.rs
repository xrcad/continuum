//! [`StorageBackend`] trait and [`StorageError`] type.
//!
//! Two implementations exist:
//! - [`GixBackend`]  (`cfg(not(target_arch = "wasm32"))`) — pure-Rust gix
//! - [`IsomorphicGitBackend`] (`cfg(target_arch = "wasm32")`) — calls
//!   `window.xrcadGit` via wasm-bindgen
//!
//! Both are wrapped by [`ActiveBackend`], the type inserted as a Bevy resource.
//! When gix gains `wasm32-unknown-unknown` support, `IsomorphicGitBackend` is
//! deleted and `GixBackend` is used for both targets; nothing outside this
//! crate changes.

use std::future::Future;

pub mod gix_backend;
pub mod isomorphic_git_backend;

#[cfg(not(target_arch = "wasm32"))]
pub use gix_backend::GixBackend;

#[cfg(target_arch = "wasm32")]
pub use isomorphic_git_backend::IsomorphicGitBackend;

// ─────────────────────────────────────────────────────────────────────────────
// Error type
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("git operation failed: {0}")]
    Git(String),
    #[error("filesystem error: {0}")]
    Fs(String),
    #[error("serialization error: {0}")]
    Serialization(String),
}

// ─────────────────────────────────────────────────────────────────────────────
// Trait
// ─────────────────────────────────────────────────────────────────────────────

/// Persistence backend for xrcad document op logs.
///
/// Implementations must not expose platform-specific types outside this module.
/// All errors are mapped to [`StorageError`] at the implementation boundary.
pub trait StorageBackend: Send + Sync {
    /// Initialise a new repository if one does not already exist.
    fn init(&self) -> impl Future<Output = Result<(), StorageError>> + Send;

    /// Append `ops_content` to the ops log and create a git commit.
    ///
    /// `message` is the full commit message (structured format).
    /// `ops_content` is the complete text content of `ops.log` after appending.
    fn commit(
        &self,
        message: &str,
        ops_content: &str,
    ) -> impl Future<Output = Result<(), StorageError>> + Send;

    /// Returns `true` if a repository has already been initialised.
    fn is_initialised(&self) -> impl Future<Output = bool> + Send;
}

// ─────────────────────────────────────────────────────────────────────────────
// ActiveBackend — Bevy resource wrapping the platform-selected implementation
// ─────────────────────────────────────────────────────────────────────────────

/// The active storage backend, selected at compile time.
///
/// Inserted as a Bevy resource by [`crate::XrcadDataPlugin`]. Use
/// [`ActiveBackend::init`], [`ActiveBackend::commit`], and
/// [`ActiveBackend::is_initialised`] rather than the underlying backend type.
pub enum ActiveBackend {
    #[cfg(not(target_arch = "wasm32"))]
    Gix(GixBackend),

    #[cfg(target_arch = "wasm32")]
    IsomorphicGit(IsomorphicGitBackend),
}

impl ActiveBackend {
    pub async fn init(&self) -> Result<(), StorageError> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            ActiveBackend::Gix(b) => b.init().await,
            #[cfg(target_arch = "wasm32")]
            ActiveBackend::IsomorphicGit(b) => b.init().await,
        }
    }

    pub async fn commit(&self, message: &str, ops_content: &str) -> Result<(), StorageError> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            ActiveBackend::Gix(b) => b.commit(message, ops_content).await,
            #[cfg(target_arch = "wasm32")]
            ActiveBackend::IsomorphicGit(b) => b.commit(message, ops_content).await,
        }
    }

    pub async fn is_initialised(&self) -> bool {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            ActiveBackend::Gix(b) => b.is_initialised().await,
            #[cfg(target_arch = "wasm32")]
            ActiveBackend::IsomorphicGit(b) => b.is_initialised().await,
        }
    }
}

// SAFETY: ActiveBackend contains only a single variant at runtime (selected
// by cfg). Both GixBackend and IsomorphicGitBackend are Send+Sync.
unsafe impl Send for ActiveBackend {}
unsafe impl Sync for ActiveBackend {}

impl bevy::ecs::resource::Resource for ActiveBackend {}
