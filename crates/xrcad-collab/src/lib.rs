//! `xrcad-collab` — collaboration protocol for xrcad.
//!
//! Builds on `xrcad-net`'s raw transport to provide:
//! - **Presence** — cursor positions, viewports, display names (unreliable, LWW)
//! - **OpLog** — causally ordered document operations with vector clock tracking
//! - **SessionManager** — peer registry, join/leave lifecycle
//! - **GitCommitter** — periodic commit of op batches to a git repository (native only)
//!
//! # Usage
//!
//! Add [`XrcadCollabPlugin`] alongside [`xrcad_net::XrcadNetPlugin`]:
//!
//! ```rust,no_run
//! use bevy::prelude::*;
//! use xrcad_net::{XrcadNetPlugin, PeerId};
//! use xrcad_collab::XrcadCollabPlugin;
//!
//! fn main() {
//!     App::new()
//!         .add_plugins(DefaultPlugins)
//!         .add_plugins(XrcadNetPlugin {
//!             local_peer_id: PeerId::generate(),
//!             display_name:  "Alice".to_string(),
//!         })
//!         .add_plugins(XrcadCollabPlugin::default())
//!         .run();
//! }
//! ```

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use xrcad_net::PeerId;

pub mod doc_op;
pub mod op_log;
pub mod presence;
pub mod session;
pub mod vector_clock;

#[cfg(not(target_arch = "wasm32"))]
pub mod git_committer;

pub use doc_op::{ConflictOutcome, DocOp};
pub use op_log::OpEnvelope;
pub use presence::{PresenceMsg, PeerPresence};
pub use session::SessionManager;
pub use vector_clock::VectorClock;

// ─────────────────────────────────────────────────────────────────────────────
// Plugin
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for [`XrcadCollabPlugin`].
#[derive(Debug, Clone)]
pub struct CollabConfig {
    /// Commit a batch to git after this many ops accumulate. Default: 50.
    pub git_commit_threshold: usize,
    /// Also commit if this many seconds pass with uncommitted ops. Default: 60.
    pub git_commit_interval_secs: u64,
}

impl Default for CollabConfig {
    fn default() -> Self {
        Self {
            git_commit_threshold:     50,
            git_commit_interval_secs: 60,
        }
    }
}

/// Add to your Bevy [`App`] alongside [`xrcad_net::XrcadNetPlugin`].
pub struct XrcadCollabPlugin {
    pub config: CollabConfig,
}

impl Default for XrcadCollabPlugin {
    fn default() -> Self {
        Self { config: CollabConfig::default() }
    }
}

impl Plugin for XrcadCollabPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(self.config.clone())
            .insert_resource(SessionManager::default())
            .insert_resource(op_log::OpLog::default())
            .insert_resource(presence::PresenceState::default())
            .add_event::<OpApplied>()
            .add_event::<OpConflict>()
            .add_event::<SendDocOp>()
            .add_systems(Update, (
                presence::broadcast_presence,
                presence::receive_presence,
                op_log::receive_ops,
                op_log::apply_ready_ops,
                session::handle_peer_connected,
                session::handle_peer_disconnected,
            ));

        #[cfg(not(target_arch = "wasm32"))]
        git_committer::register(app, &self.config);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Events
// ─────────────────────────────────────────────────────────────────────────────

/// A [`DocOp`] has been applied to the local document. Other systems (the renderer,
/// the kernel) listen for this to update their state.
#[derive(Event, Debug, Clone)]
pub struct OpApplied {
    pub envelope: OpEnvelope,
}

/// Two concurrent ops are topologically incompatible. The op has been buffered but
/// not applied. The UI should surface this to the user.
#[derive(Event, Debug, Clone)]
pub struct OpConflict {
    pub local_op:  DocOp,
    pub remote_op: DocOp,
}

/// Fire this event to send a [`DocOp`] to all connected peers (and apply it locally).
///
/// Example:
/// ```rust,no_run
/// # use bevy::prelude::*;
/// # use xrcad_collab::{SendDocOp, DocOp};
/// fn send_chat(mut writer: EventWriter<SendDocOp>) {
///     writer.send(SendDocOp(DocOp::Chat { text: "hello!".into() }));
/// }
/// ```
#[derive(Event, Debug, Clone)]
pub struct SendDocOp(pub DocOp);
