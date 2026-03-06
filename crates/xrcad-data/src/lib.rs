//! `xrcad-data` — persistence and B-Rep registry for xrcad.
//!
//! # Storage backend
//!
//! [`XrcadDataPlugin`] listens for [`xrcad_collab::OpApplied`] events and
//! batches them into git commits via the active [`backend::StorageBackend`]:
//!
//! - **Native**: [`backend::GixBackend`] — pure-Rust gix, no C dependencies.
//! - **WASM**: [`backend::IsomorphicGitBackend`] — calls `window.xrcadGit`
//!   (isomorphic-git + ZenFS OPFS) via wasm-bindgen.
//!
//! When gix gains `wasm32-unknown-unknown` support, `IsomorphicGitBackend` is
//! deleted and `GixBackend` is used on both targets. Nothing outside this
//! crate changes.

pub mod backend;
pub mod brep;

use std::time::Duration;

use bevy::prelude::*;
use xrcad_collab::OpApplied;

use backend::ActiveBackend;

#[allow(unused_imports)]
use backend::StorageError;

// ─────────────────────────────────────────────────────────────────────────────
// Resources
// ─────────────────────────────────────────────────────────────────────────────

/// Pending batch of ops waiting to be committed.
#[derive(Resource, Default)]
pub struct PendingBatch {
    /// Accumulated ops.log lines since the last commit.
    pub lines: Vec<String>,
    /// Wall-clock time when the first op in the current batch arrived.
    pub first_op_at: Option<std::time::Instant>,
    /// Content of ops.log committed so far (prefix for the next commit).
    pub committed_log: String,
}

/// Commit thresholds. Both are checked each frame; whichever fires first wins.
#[derive(Resource)]
pub struct CommitPolicy {
    /// Commit after this many ops accumulate. Default: 50.
    pub op_threshold: usize,
    /// Commit after this long with uncommitted ops. Default: 60 s.
    pub time_threshold: Duration,
}

impl Default for CommitPolicy {
    fn default() -> Self {
        Self {
            op_threshold:   50,
            time_threshold: Duration::from_secs(60),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Plugin
// ─────────────────────────────────────────────────────────────────────────────

/// Add to your Bevy [`App`] alongside [`xrcad_collab::XrcadCollabPlugin`].
///
/// Inserts the platform-appropriate [`ActiveBackend`] resource and registers
/// the op-accumulation + commit system.
pub struct XrcadDataPlugin;

impl Plugin for XrcadDataPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(not(target_arch = "wasm32"))]
        app.insert_resource(ActiveBackend::Gix(backend::GixBackend {
            repo_path: default_repo_path(),
        }));

        #[cfg(target_arch = "wasm32")]
        app.insert_resource(ActiveBackend::IsomorphicGit(
            backend::IsomorphicGitBackend {
                dir: "/xrcad/default".to_string(),
            },
        ));

        app.insert_resource(PendingBatch::default())
            .insert_resource(CommitPolicy::default())
            .add_systems(Update, accumulate_and_commit);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Systems
// ─────────────────────────────────────────────────────────────────────────────

fn accumulate_and_commit(
    backend:  Res<ActiveBackend>,
    policy:   Res<CommitPolicy>,
    mut batch: ResMut<PendingBatch>,
    mut ops:   MessageReader<OpApplied>,
) {
    // Drain incoming ops into the pending batch.
    for event in ops.read() {
        let line = format!(
            "{}/{:<6}  {}\n",
            event.envelope.peer_id,
            event.envelope.seq,
            event.envelope.op.summary(),
        );
        if batch.first_op_at.is_none() {
            batch.first_op_at = Some(std::time::Instant::now());
        }
        batch.lines.push(line);
    }

    if batch.lines.is_empty() {
        return;
    }

    // Check thresholds.
    let threshold_hit = batch.lines.len() >= policy.op_threshold;
    let interval_hit = batch
        .first_op_at
        .map(|t| t.elapsed() >= policy.time_threshold)
        .unwrap_or(false);

    if !threshold_hit && !interval_hit {
        return;
    }

    // Build the full ops.log content and commit message.
    let new_lines: String = batch.lines.join("");
    let ops_content = format!("{}{}", batch.committed_log, new_lines);
    let n = batch.lines.len();
    let message = format!("xrcad: {n} operations\n\n{new_lines}");

    // Commit. On native we block_on the future (acceptable for infrequent
    // commits). On WASM we spawn_local on the JS microtask queue using data
    // extracted from the backend so we avoid lifetime issues across the
    // async boundary.
    #[cfg(not(target_arch = "wasm32"))]
    bevy::tasks::block_on(async {
        commit_with_init(&backend, &message, &ops_content).await;
    });

    #[cfg(target_arch = "wasm32")]
    {
        if let ActiveBackend::IsomorphicGit(b) = &*backend {
            let dir     = b.dir.clone();
            let msg     = message.clone();
            let content = ops_content.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let b = backend::IsomorphicGitBackend { dir };
                commit_with_init(&b, &msg, &content).await;
            });
        }
    }

    batch.committed_log = ops_content;
    batch.lines.clear();
    batch.first_op_at = None;
}

#[cfg(not(target_arch = "wasm32"))]
async fn commit_with_init(backend: &ActiveBackend, message: &str, ops_content: &str) {
    if !backend.is_initialised().await {
        if let Err(e) = backend.init().await {
            tracing::error!("xrcad-data: init failed: {e}");
            return;
        }
    }
    if let Err(e) = backend.commit(message, ops_content).await {
        tracing::error!("xrcad-data: commit failed: {e}");
    }
}

#[cfg(target_arch = "wasm32")]
async fn commit_with_init(
    backend: &backend::IsomorphicGitBackend,
    message: &str,
    ops_content: &str,
) {
    use backend::StorageBackend as _;
    if !backend.is_initialised().await {
        if let Err(e) = backend.init().await {
            tracing::error!("xrcad-data: init failed: {e}");
            return;
        }
    }
    if let Err(e) = backend.commit(message, ops_content).await {
        tracing::error!("xrcad-data: commit failed: {e}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
fn default_repo_path() -> std::path::PathBuf {
    std::env::current_dir().expect("cannot determine working directory")
}
