//! `xrcad-collab` — collaboration protocol for xrcad.
//!
//! Builds on `xrcad-net`'s raw transport to provide:
//! - **Presence** — cursor positions, viewports, display names (unreliable, LWW)
//! - **OpLog** — causally ordered document operations with vector clock tracking
//! - **SessionManager** — peer registry, join/leave lifecycle
//!
//! Persistence (git commits) is handled by `xrcad-data`, which listens for
//! [`OpApplied`] events emitted by this crate.
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

pub mod doc_op;
pub mod op_log;
pub mod presence;
pub mod session;
pub mod time;
pub mod vector_clock;

pub use doc_op::{ConflictOutcome, DocOp};
pub use op_log::OpEnvelope;
pub use presence::{LocalViewport, PeerPresence, PresenceMsg, PresenceState, Viewport};
pub use session::SessionManager;
pub use time::now_ms;
pub use vector_clock::VectorClock;

// ─────────────────────────────────────────────────────────────────────────────
// Plugin
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for [`XrcadCollabPlugin`].
#[derive(Resource, Debug, Clone, Default)]
pub struct CollabConfig {}

/// Add to your Bevy [`App`] alongside [`xrcad_net::XrcadNetPlugin`].
#[derive(Default)]
pub struct XrcadCollabPlugin {
    pub config: CollabConfig,
}

impl Plugin for XrcadCollabPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.config.clone())
            .insert_resource(SessionManager::default())
            .insert_resource(op_log::OpLog::default())
            .insert_resource(presence::PresenceState::default())
            .insert_resource(presence::LocalViewport::default())
            .add_message::<OpApplied>()
            .add_message::<OpConflict>()
            .add_message::<SendDocOp>()
            .add_systems(
                Update,
                (
                    presence::broadcast_presence,
                    presence::receive_presence,
                    op_log::receive_ops,
                    op_log::apply_ready_ops,
                    session::handle_peer_connected,
                    session::handle_peer_disconnected,
                ),
            );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Events
// ─────────────────────────────────────────────────────────────────────────────

/// A [`DocOp`] has been applied to the local document. Other systems (the renderer,
/// the kernel) listen for this to update their state.
#[derive(Message, Debug, Clone)]
pub struct OpApplied {
    pub envelope: OpEnvelope,
}

/// Two concurrent ops are topologically incompatible. The op has been buffered but
/// not applied. The UI should surface this to the user.
#[derive(Message, Debug, Clone)]
pub struct OpConflict {
    pub local_op: DocOp,
    pub remote_op: DocOp,
}

/// Fire this event to send a [`DocOp`] to all connected peers (and apply it locally).
///
/// Example:
/// ```rust,no_run
/// # use bevy::prelude::*;
/// # use xrcad_collab::{SendDocOp, DocOp};
/// fn send_chat(mut writer: MessageWriter<SendDocOp>) {
///     writer.write(SendDocOp(DocOp::Chat { text: "hello!".into() }));
/// }
/// ```
#[derive(Message, Debug, Clone)]
pub struct SendDocOp(pub DocOp);
