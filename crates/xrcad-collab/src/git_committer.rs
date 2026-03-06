//! Periodic git commits of accumulated [`OpEnvelope`]s.
//!
//! This module is compiled only for native targets (`not(target_arch = "wasm32")`).
//!
//! Operations accumulate in [`OpLog::applied`]. When the commit threshold or time
//! interval is reached, the pending batch is committed to the git repository with a
//! structured commit message that is human-readable with standard git tools.
//!
//! # Commit message format
//!
//! ```text
//! xrcad: <N> operations
//!
//! Peers: alice <uuid> (12 ops), bob <uuid> (11 ops)
//! Session: <session-uuid>
//! Clock: {"<alice-uuid>": 120, "<bob-uuid>": 98}
//!
//! alice/108  MoveVertex { id: v42, delta: [0.5, 0.0, -1.2] }
//! alice/109  AddConstraint { ... }
//! bob/91     ExtrudeFace { face: f7, distance: 10.0 }
//! ```

use std::{collections::HashMap, path::PathBuf, time::Instant};

use bevy::prelude::*;
use git2::Repository;

use crate::{
    op_log::{OpEnvelope, OpLog},
    session::SessionManager,
    CollabConfig,
};

// ─────────────────────────────────────────────────────────────────────────────
// Resource
// ─────────────────────────────────────────────────────────────────────────────

/// State for the git committer. Inserted by [`register`].
#[derive(Resource)]
pub struct GitCommitter {
    pub repo_path:      PathBuf,
    pub config:         CollabConfig,
    pub last_commit_at: Instant,
    /// Index into `OpLog::applied` of the first op not yet committed.
    pub committed_up_to: usize,
}

impl GitCommitter {
    pub fn new(repo_path: PathBuf, config: CollabConfig) -> Self {
        Self {
            repo_path,
            config,
            last_commit_at: Instant::now(),
            committed_up_to: 0,
        }
    }

    /// True if a commit should be made now.
    pub fn should_commit(&self, log: &OpLog) -> bool {
        let pending = log.applied.len() - self.committed_up_to;
        if pending == 0 {
            return false;
        }
        let threshold_reached = pending >= self.config.git_commit_threshold;
        let interval_elapsed  = self.last_commit_at.elapsed().as_secs()
                                >= self.config.git_commit_interval_secs;
        threshold_reached || interval_elapsed
    }

    /// Commit all applied ops since `committed_up_to` to the git repository.
    pub fn commit_pending(
        &mut self,
        log:     &mut OpLog,
        session: &SessionManager,
    ) -> Result<(), git2::Error> {
        let batch: Vec<&OpEnvelope> = log.applied[self.committed_up_to..].iter().collect();
        if batch.is_empty() {
            return Ok(());
        }

        let repo = Repository::open(&self.repo_path)?;
        let sig  = repo.signature()?;

        // Build commit message
        let message = build_commit_message(&batch, session, &log.clock);

        // Write to the ops log file (a flat append-only file tracked in git)
        let ops_path = self.repo_path.join("ops.log");
        let mut ops_content = std::fs::read_to_string(&ops_path).unwrap_or_default();
        for env in &batch {
            ops_content.push_str(&format!("{}\n", env.summary()));
        }
        std::fs::write(&ops_path, &ops_content)?;

        // Stage and commit
        let mut index = repo.index()?;
        index.add_path(std::path::Path::new("ops.log"))?;
        index.write()?;

        let tree_id  = index.write_tree()?;
        let tree     = repo.find_tree(tree_id)?;
        let parent   = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        let parents: Vec<&git2::Commit> = parent.iter().collect();

        repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &parents)?;

        self.committed_up_to = log.applied.len();
        self.last_commit_at  = Instant::now();

        tracing::info!("committed {} ops to git", batch.len());
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Bevy registration
// ─────────────────────────────────────────────────────────────────────────────

/// Called from [`crate::XrcadCollabPlugin::build`] on native targets.
pub fn register(app: &mut App, config: &CollabConfig) {
    // The repo path defaults to the current directory. In production this should be
    // derived from the open document path.
    let repo_path = std::env::current_dir().expect("cannot determine working directory");

    app.insert_resource(GitCommitter::new(repo_path, config.clone()))
       .add_systems(Update, commit_if_ready);
}

/// System: commit pending ops when the threshold or interval is reached.
pub fn commit_if_ready(
    mut committer: ResMut<GitCommitter>,
    mut log:       ResMut<OpLog>,
    session:       Res<SessionManager>,
) {
    if committer.should_commit(&log) {
        if let Err(e) = committer.commit_pending(&mut log, &session) {
            tracing::error!("git commit failed: {e}");
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Commit message builder
// ─────────────────────────────────────────────────────────────────────────────

fn build_commit_message(
    batch:   &[&OpEnvelope],
    session: &SessionManager,
    clock:   &crate::VectorClock,
) -> String {
    let mut per_peer: HashMap<String, usize> = HashMap::new();
    for env in batch {
        *per_peer.entry(env.peer_id.to_string()).or_default() += 1;
    }

    let peer_summary = per_peer
        .iter()
        .map(|(id, n)| format!("{id} ({n} ops)"))
        .collect::<Vec<_>>()
        .join(", ");

    let session_str = session
        .session_id
        .map(|s| s.0.to_string())
        .unwrap_or_else(|| "none".into());

    let clock_json = {
        let entries: Vec<String> = clock.0.iter()
            .map(|(p, s)| format!("{p:?}: {s}"))
            .collect();
        format!("{{{}}}", entries.join(", "))
    };

    let op_lines: String = batch
        .iter()
        .map(|e| format!("  {}/{:<6}  {}", e.peer_id, e.seq, e.op.summary()))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "xrcad: {} operations\n\nPeers: {}\nSession: {}\nClock: {}\n\n{}",
        batch.len(),
        peer_summary,
        session_str,
        clock_json,
        op_lines,
    )
}
