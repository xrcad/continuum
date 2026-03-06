//! Session management: peer registry, join/leave lifecycle.

use std::collections::HashMap;

use bevy::prelude::*;
use xrcad_net::{PeerConnected, PeerDisconnected, PeerId, SessionId};

/// Information about a connected remote peer.
#[derive(Debug, Clone)]
pub struct RemotePeer {
    pub peer_id: PeerId,
    pub display_name: Option<String>,
    /// Monotonically increasing sequence number last seen from this peer.
    pub last_seq: u64,
}

/// Registry of all peers currently in the session.
#[derive(Resource, Default)]
pub struct SessionManager {
    pub peers: HashMap<PeerId, RemotePeer>,
    pub session_id: Option<SessionId>,
}

impl SessionManager {
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    pub fn add_peer(&mut self, peer_id: PeerId, display_name: Option<String>) {
        self.peers.insert(
            peer_id,
            RemotePeer {
                peer_id,
                display_name,
                last_seq: 0,
            },
        );
        tracing::info!("peer joined: {peer_id}");
    }

    pub fn remove_peer(&mut self, peer_id: PeerId) {
        self.peers.remove(&peer_id);
        tracing::info!("peer left: {peer_id}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Systems
// ─────────────────────────────────────────────────────────────────────────────

pub fn handle_peer_connected(
    mut events: MessageReader<PeerConnected>,
    mut manager: ResMut<SessionManager>,
) {
    for ev in events.read() {
        manager.session_id = Some(ev.session_id);
        manager.add_peer(ev.peer_id, ev.display_name.clone());
    }
}

pub fn handle_peer_disconnected(
    mut events: MessageReader<PeerDisconnected>,
    mut manager: ResMut<SessionManager>,
) {
    for ev in events.read() {
        manager.remove_peer(ev.peer_id);
    }
}
