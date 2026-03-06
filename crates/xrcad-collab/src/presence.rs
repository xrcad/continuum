//! Presence: cursor positions, viewport state, display names.
//!
//! Presence is broadcast on the unreliable channel at up to 30 Hz.
//! Last-write-wins; stale entries expire after [`PRESENCE_TIMEOUT_MS`] ms.

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use xrcad_net::{Channel, LocalPeer, NetCommand, PeerId, PeerMessageReceived};

const PRESENCE_TIMEOUT_MS: u64 = 3_000;

// ─────────────────────────────────────────────────────────────────────────────
// Wire type
// ─────────────────────────────────────────────────────────────────────────────

/// Presence message sent on the unreliable channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceMsg {
    pub peer_id: PeerId,
    pub display_name: String,
    /// Cursor world position, if the peer has an active cursor.
    pub cursor_pos: Option<[f32; 3]>,
    /// Camera eye position and look-at target.
    pub viewport: Option<Viewport>,
    /// Which tool is currently active on this peer.
    pub active_tool: Option<String>,
    /// RGB colour for this peer's cursor and selection highlight.
    pub peer_colour: [f32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Viewport {
    pub eye: [f32; 3],
    pub target: [f32; 3],
}

// ─────────────────────────────────────────────────────────────────────────────
// Resources
// ─────────────────────────────────────────────────────────────────────────────

/// The local peer's current camera viewport.
///
/// Write this resource from the app's camera system each frame.
/// `broadcast_presence` reads it and includes it in every outgoing
/// presence packet so remote peers can render a marker at our camera position.
///
/// ```rust,no_run
/// local_vp.0 = Some(xrcad_collab::presence::Viewport {
///     eye:    camera_transform.translation.into(),
///     target: orbit_target.into(),
/// });
/// ```
#[derive(Resource, Default)]
pub struct LocalViewport(pub Option<Viewport>);

/// Last-seen presence entry for a remote peer.
#[derive(Debug, Clone)]
pub struct PeerPresence {
    pub msg: PresenceMsg,
    /// Milliseconds since UNIX epoch when this was last received.
    pub last_seen_ms: u64,
}

/// All currently known remote peer presences. Stale entries expire.
#[derive(Resource, Default)]
pub struct PresenceState {
    pub peers: HashMap<PeerId, PeerPresence>,
}

impl PresenceState {
    pub fn update(&mut self, msg: PresenceMsg) {
        let now = now_ms();
        self.peers.insert(
            msg.peer_id,
            PeerPresence {
                msg,
                last_seen_ms: now,
            },
        );
    }

    /// Remove entries that haven't been heard from in [`PRESENCE_TIMEOUT_MS`].
    pub fn expire(&mut self) {
        let cutoff = now_ms().saturating_sub(PRESENCE_TIMEOUT_MS);
        self.peers.retain(|_, p| p.last_seen_ms >= cutoff);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Systems
// ─────────────────────────────────────────────────────────────────────────────

/// Broadcast our presence to all connected peers on the unreliable channel.
/// Run at ~10 Hz (configure via run conditions in the plugin).
pub fn broadcast_presence(
    local: Res<LocalPeer>,
    local_vp: Res<LocalViewport>,
    mut cmds: MessageWriter<NetCommand>,
) {
    let msg = PresenceMsg {
        peer_id: local.peer_id,
        display_name: local.display_name.clone(),
        cursor_pos: None,
        viewport: local_vp.0.clone(),
        active_tool: None,
        peer_colour: [0.4, 0.7, 1.0], // placeholder; should be per-peer persistent colour
    };

    let Ok(payload) = postcard::to_allocvec(&msg) else {
        tracing::warn!("failed to serialize PresenceMsg");
        return;
    };

    cmds.write(NetCommand::Broadcast {
        channel: Channel::Unreliable,
        payload,
    });
}

/// Receive presence messages from remote peers and update [`PresenceState`].
pub fn receive_presence(
    mut messages: MessageReader<PeerMessageReceived>,
    mut state: ResMut<PresenceState>,
) {
    for PeerMessageReceived(raw) in messages.read() {
        if raw.channel != Channel::Unreliable {
            continue;
        }
        match postcard::from_bytes::<PresenceMsg>(&raw.payload) {
            Ok(msg) => state.update(msg),
            Err(_) => { /* may be a different unreliable message type in future */ }
        }
    }
    state.expire();
}

// ─────────────────────────────────────────────────────────────────────────────

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
