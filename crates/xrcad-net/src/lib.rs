//! `xrcad-net` — transport layer for xrcad peer-to-peer collaboration.
//!
//! Responsibilities:
//! - mDNS autodiscovery on LAN and Tailscale networks (`native` feature)
//! - Session code exchange for internet peers
//! - Direct UDP + TCP connections (`native` feature)
//! - WebRTC DataChannel connections (`wasm` feature)
//! - Bevy plugin exposing typed events and a command channel
//!
//! This crate handles **transport only**. It does not interpret message payloads.
//! `xrcad-collab` builds the collaboration protocol on top of the events this crate emits.
//!
//! # Feature flags
//! - `native` (default): tokio sockets + mdns-sd
//! - `wasm`: WebRTC DataChannel via web-sys

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod error;
pub mod session_code;

#[cfg(feature = "native")]
pub mod native;

#[cfg(feature = "wasm")]
pub mod wasm;

// ─────────────────────────────────────────────────────────────────────────────
// Identity types
// ─────────────────────────────────────────────────────────────────────────────

/// Stable identity for a peer. Generated once at first launch; persisted locally.
///
/// Peers identify each other by this ID across sessions. It is not a cryptographic
/// identifier — it is just a UUID used to correlate messages and presence entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PeerId(pub Uuid);

impl PeerId {
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
}

impl std::fmt::Display for PeerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.0.to_string()[..8])
    }
}

/// Unique identifier for a collaboration session.
///
/// A session ID is chosen by the peer that creates the session. All peers in the same
/// session share the same session ID. It is embedded in mDNS TXT records and session codes
/// so that joining peers can confirm they are connecting to the right session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub Uuid);

impl SessionId {
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
}

// ─────────────────────────────────────────────────────────────────────────────
// Raw message envelope
// ─────────────────────────────────────────────────────────────────────────────

/// The logical channel on which a message was sent or received.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Channel {
    /// Unreliable, unordered. For presence / cursor / viewport. May be lost.
    Unreliable,
    /// Reliable, ordered. For document operations. Guaranteed delivery and order.
    Reliable,
}

/// A raw inbound message before deserialization by `xrcad-collab`.
///
/// The `payload` field is a `postcard`-encoded byte slice. `xrcad-collab` systems
/// subscribe to `PeerMessageReceived` events and decode the payload into their own types.
#[derive(Debug, Clone)]
pub struct RawMessage {
    pub from:    PeerId,
    pub channel: Channel,
    /// postcard-encoded payload; deserialized by xrcad-collab.
    pub payload: Vec<u8>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Bevy events — outward signals from xrcad-net to the rest of the app
// ─────────────────────────────────────────────────────────────────────────────

/// A peer has connected and completed the session handshake.
#[derive(Event, Debug, Clone)]
pub struct PeerConnected {
    pub peer_id:     PeerId,
    /// Display name as declared in the peer's handshake packet. May be updated later
    /// via `DocOp::SetPeerName` in xrcad-collab.
    pub display_name: Option<String>,
    pub session_id:  SessionId,
}

/// A peer has disconnected, either gracefully or by timeout.
#[derive(Event, Debug, Clone)]
pub struct PeerDisconnected {
    pub peer_id: PeerId,
    /// `true` if the peer sent an explicit close; `false` if the connection timed out.
    pub graceful: bool,
}

/// A raw message has arrived from a peer. Consumed by `xrcad-collab`.
#[derive(Event, Debug, Clone)]
pub struct PeerMessageReceived(pub RawMessage);

/// mDNS has found a new xrcad instance on the local network or tailnet.
///
/// The application should show this peer in the "available sessions" list.
/// The user explicitly chooses whether to join.
#[derive(Event, Debug, Clone)]
pub struct PeerDiscovered {
    pub peer_id:      PeerId,
    pub display_name: Option<String>,
    /// Whether this peer is currently hosting a joinable session.
    pub has_session:  bool,
    pub session_id:   Option<SessionId>,
}

/// A previously discovered peer has left the local network (mDNS goodbye packet or timeout).
#[derive(Event, Debug, Clone)]
pub struct PeerLost {
    pub peer_id: PeerId,
}

// ─────────────────────────────────────────────────────────────────────────────
// Commands — inward signals to xrcad-net from the rest of the app
// ─────────────────────────────────────────────────────────────────────────────

/// Commands sent to the net layer. Fire these as Bevy events to control the session.
///
/// `xrcad-collab` and the UI send these; `xrcad-net` systems consume them.
#[derive(Event, Debug)]
pub enum NetCommand {
    /// Start a new session. Begins mDNS advertisement.
    StartSession { session_id: SessionId },

    /// Join an existing session — either a discovered LAN peer or an internet session code.
    JoinSession { target: JoinTarget },

    /// Broadcast a payload to all connected peers on the given channel.
    Broadcast { channel: Channel, payload: Vec<u8> },

    /// Send a payload to a specific peer.
    SendTo { peer_id: PeerId, channel: Channel, payload: Vec<u8> },

    /// Leave the session and close all peer connections.
    LeaveSession,
}

/// How the joining peer will reach the target.
#[derive(Debug, Clone)]
pub enum JoinTarget {
    /// A peer discovered via mDNS — `xrcad-net` already knows the address.
    DiscoveredPeer(PeerId),
    /// A session code produced by `session_code::encode`.
    SessionCode(String),
}

// ─────────────────────────────────────────────────────────────────────────────
// Resources
// ─────────────────────────────────────────────────────────────────────────────

/// Current session state of the local peer. Readable by any system.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub enum SessionState {
    #[default]
    /// Not in any session.
    Idle,
    /// Hosting a session; peers may join.
    Hosting { session_id: SessionId },
    /// Joined another peer's session.
    Joined { session_id: SessionId },
}

/// This peer's identity and display name.
#[derive(Resource, Debug, Clone)]
pub struct LocalPeer {
    pub peer_id:      PeerId,
    pub display_name: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Plugin
// ─────────────────────────────────────────────────────────────────────────────

/// Add to your Bevy [`App`] to enable xrcad networking.
///
/// The local peer's identity should be generated once at first launch and persisted
/// (e.g. in the app config directory). Generating a new UUID on every launch means
/// other peers cannot recognise returning users across reconnects.
///
/// # Example
///
/// ```rust,no_run
/// use bevy::prelude::*;
/// use xrcad_net::{XrcadNetPlugin, PeerId};
///
/// fn main() {
///     App::new()
///         .add_plugins(DefaultPlugins)
///         .add_plugins(XrcadNetPlugin {
///             local_peer_id: PeerId::generate(), // persist this!
///             display_name:  "Alice".to_string(),
///         })
///         .run();
/// }
/// ```
pub struct XrcadNetPlugin {
    /// This peer's stable identity. Generate once; persist across launches.
    pub local_peer_id: PeerId,
    /// Human-readable name shown to other peers in the session.
    pub display_name: String,
}

impl Plugin for XrcadNetPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(LocalPeer {
                peer_id:      self.local_peer_id,
                display_name: self.display_name.clone(),
            })
            .insert_resource(SessionState::default())
            .add_event::<PeerConnected>()
            .add_event::<PeerDisconnected>()
            .add_event::<PeerMessageReceived>()
            .add_event::<PeerDiscovered>()
            .add_event::<PeerLost>()
            .add_event::<NetCommand>();

        #[cfg(feature = "native")]
        native::register(app);

        #[cfg(feature = "wasm")]
        wasm::register(app);
    }
}
