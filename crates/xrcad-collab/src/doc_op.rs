//! [`DocOp`] — the typed document operation enum.
//!
//! Every change to an xrcad document is expressed as a `DocOp`. Operations are:
//! - **Self-contained**: carry all data needed to apply or reverse them
//! - **Typed**: no raw diffs or opaque blobs
//! - **Reversible**: every op has a well-defined inverse for undo
//!
//! New variants are added here as `xrcad-kernel` features are implemented.
//! Existing variants must not be removed or reordered — doing so would break
//! deserialization of existing git history.

use serde::{Deserialize, Serialize};

/// A document operation. Grows incrementally as the kernel grows.
///
/// # Adding new variants
///
/// 1. Add the variant here with full documentation
/// 2. Add a corresponding inverse in `inverse()`
/// 3. Add kernel application logic in `xrcad-kernel`
/// 4. Add a `DocOp::YourVariant { .. }` arm to the UI conflict resolution view
///
/// # Serialization stability
///
/// Variants are serialized by postcard using their discriminant index. **Never reorder or
/// remove variants** — append only. Bump the protocol version in `xrcad-net` if a breaking
/// change is unavoidable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DocOp {
    // ── Phase 1 — bootstrapping ─────────────────────────────────────────────

    /// Text chat message. Carried on the reliable channel so it appears in git history.
    Chat { text: String },

    /// A peer updates their human-readable display name.
    SetPeerName { name: String },

    // ── Phase 2 — FeRx script ───────────────────────────────────────────────
    //
    // Uncomment and flesh out when xrcad-script is implemented.
    // These ops model text editing of the FeRx DSL source.
    //
    // /// Insert `text` starting at byte offset `pos` in the script.
    // ScriptInsert { pos: usize, text: String },
    //
    // /// Delete the byte range `start..end` from the script.
    // ScriptDelete { start: usize, end: usize },

    // ── Phase 3 — B-rep geometry ────────────────────────────────────────────
    //
    // One variant per kernel operation. Each maps 1:1 to a half-edge mesh mutation.
    // Add these as the kernel ops are implemented in xrcad-kernel.
    //
    // /// Add a new isolated vertex at `pos`.
    // AddVertex { id: EntityId, pos: [f64; 3] },
    //
    // /// Translate a vertex by `delta`.
    // MoveVertex { id: EntityId, delta: [f64; 3] },
    //
    // /// Remove a vertex (must have no incident edges).
    // DeleteVertex { id: EntityId },
    //
    // /// Split an edge at parameter `t` ∈ (0, 1), creating a new midpoint vertex.
    // SplitEdge { edge: EntityId, t: f64 },
    //
    // /// Extrude a face by `distance` along its normal.
    // ExtrudeFace { face: EntityId, distance: f64 },
    //
    // /// Add a geometric constraint.
    // AddConstraint { constraint: Constraint },
    //
    // /// Remove a constraint by ID.
    // RemoveConstraint { id: ConstraintId },

    // ── Conflict bookkeeping ────────────────────────────────────────────────

    /// Records a user's resolution of a topology conflict.
    ///
    /// Both conflicting operations are embedded so the history is auditable in git.
    ConflictResolution {
        /// The peer who made the resolution decision.
        resolved_by: xrcad_net::PeerId,
        /// The local operation involved in the conflict.
        local_op:    Box<DocOp>,
        /// The remote operation involved in the conflict.
        remote_op:   Box<DocOp>,
        /// What the user chose.
        resolution:  ConflictOutcome,
    },
}

/// The outcome of a user-resolved topology conflict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictOutcome {
    /// The local operation is kept; the remote is discarded.
    AcceptLocal,
    /// The remote operation is kept; the local is discarded.
    AcceptRemote,
    /// Both operations are kept — only valid if the kernel confirms the combined
    /// state passes the topology validity check.
    AcceptBoth,
    /// The user manually edited the model to resolve the conflict; the resolution
    /// is expressed as subsequent `DocOp`s rather than by accepting either conflicting op.
    ManualEdit,
}

impl DocOp {
    /// Returns a human-readable summary of this operation for commit messages and logging.
    pub fn summary(&self) -> String {
        match self {
            DocOp::Chat { text }        => format!("Chat({:?})", &text[..text.len().min(40)]),
            DocOp::SetPeerName { name } => format!("SetPeerName({name:?})"),
            DocOp::ConflictResolution { resolution, .. } => {
                format!("ConflictResolution({resolution:?})")
            }
        }
    }
}
