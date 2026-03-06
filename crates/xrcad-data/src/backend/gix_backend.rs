//! Native git backend.
//!
//! Uses the system `git` binary via `std::process::Command`. This avoids C
//! dependencies (no libgit2) while remaining simple and definitely correct.
//!
//! A pure-gix implementation (no subprocess, no git binary required) can
//! replace this once the gix index/tree/commit API stabilises across the
//! versions needed by this project. The [`StorageBackend`] trait isolates
//! the change to this file only.
//!
//! Compiled only for `not(target_arch = "wasm32")`.

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;
use std::process::Command;

use super::{StorageBackend, StorageError};

pub struct GixBackend {
    /// Path to the document's git repository root.
    pub repo_path: PathBuf,
}

impl StorageBackend for GixBackend {
    async fn init(&self) -> Result<(), StorageError> {
        if self.repo_path.join(".git").exists() {
            return Ok(());
        }
        let status = Command::new("git")
            .args(["init"])
            .current_dir(&self.repo_path)
            .status()
            .map_err(|e| StorageError::Git(format!("git init: {e}")))?;
        if !status.success() {
            return Err(StorageError::Git("git init failed".into()));
        }
        tracing::info!("xrcad-data: initialised git repo at {:?}", self.repo_path);
        Ok(())
    }

    async fn commit(&self, message: &str, ops_content: &str) -> Result<(), StorageError> {
        // 1. Write ops.log
        let ops_path = self.repo_path.join("ops.log");
        std::fs::write(&ops_path, ops_content)
            .map_err(|e| StorageError::Fs(e.to_string()))?;

        // 2. Stage ops.log
        let status = Command::new("git")
            .args(["add", "ops.log"])
            .current_dir(&self.repo_path)
            .status()
            .map_err(|e| StorageError::Git(format!("git add: {e}")))?;
        if !status.success() {
            return Err(StorageError::Git("git add failed".into()));
        }

        // 3. Commit
        let status = Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(&self.repo_path)
            .status()
            .map_err(|e| StorageError::Git(format!("git commit: {e}")))?;
        if !status.success() {
            return Err(StorageError::Git("git commit failed".into()));
        }

        tracing::info!(
            "xrcad-data: committed: {}",
            &message[..message.find('\n').unwrap_or(message.len())]
        );
        Ok(())
    }

    async fn is_initialised(&self) -> bool {
        self.repo_path.join(".git").exists()
    }
}
