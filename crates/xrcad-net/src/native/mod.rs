//! Native transport: tokio + mDNS peer discovery + TCP connections.
//!
//! Registers three Bevy systems:
//! - `startup_net` (Startup) — spawns the tokio runtime and coordinator task.
//! - `flush_inbound` (Update) — drains the inbound channel → Bevy events.
//! - `flush_outbound` (Update) — reads `NetCommand` events → outbound channel.

use std::sync::Mutex;

use bevy::prelude::*;
use tokio::sync::mpsc;

use crate::{
    Channel, LocalPeer, NetCommand, PeerConnected, PeerDisconnected, PeerDiscovered, PeerLost,
    PeerMessageReceived, RawMessage, SessionId,
};

mod coordinator;
mod framing;
mod peer;

// ─────────────────────────────────────────────────────────────────────────────
// Internal channel message types (tokio ↔ Bevy bridge)
// ─────────────────────────────────────────────────────────────────────────────

pub(super) enum NetInbound {
    PeerConnected {
        peer_id: crate::PeerId,
        display_name: Option<String>,
        session_id: SessionId,
    },
    PeerDisconnected {
        peer_id: crate::PeerId,
        graceful: bool,
    },
    Message {
        from: crate::PeerId,
        channel: Channel,
        payload: Vec<u8>,
    },
    PeerDiscovered {
        peer_id: crate::PeerId,
        display_name: Option<String>,
        session_id: Option<SessionId>,
    },
    PeerLost {
        peer_id: crate::PeerId,
    },
}

pub(super) enum NetOutbound {
    Broadcast {
        channel: Channel,
        payload: Vec<u8>,
    },
    SendTo {
        peer_id: crate::PeerId,
        channel: Channel,
        payload: Vec<u8>,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// Bevy resources
// ─────────────────────────────────────────────────────────────────────────────

/// Keeps the tokio runtime alive for the lifetime of the Bevy app.
#[derive(Resource)]
struct TokioRuntime(#[allow(dead_code)] tokio::runtime::Runtime);

/// Channels bridging the tokio coordinator task and Bevy systems.
#[derive(Resource)]
struct NetBridge {
    inbound_rx: Mutex<mpsc::UnboundedReceiver<NetInbound>>,
    outbound_tx: mpsc::UnboundedSender<NetOutbound>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Plugin registration
// ─────────────────────────────────────────────────────────────────────────────

pub fn register(app: &mut App) {
    app.add_systems(Startup, startup_net)
        .add_systems(Update, (flush_inbound, flush_outbound));
}

// ─────────────────────────────────────────────────────────────────────────────
// Systems
// ─────────────────────────────────────────────────────────────────────────────

fn startup_net(mut commands: Commands, local_peer: Res<LocalPeer>) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("xrcad-net: failed to build tokio runtime");

    let (inbound_tx, inbound_rx) = mpsc::unbounded_channel::<NetInbound>();
    let (outbound_tx, outbound_rx) = mpsc::unbounded_channel::<NetOutbound>();

    let local_peer_id = local_peer.peer_id;
    let display_name = local_peer.display_name.clone();
    let session_id = SessionId::generate();

    rt.spawn(coordinator::run_coordinator(
        local_peer_id,
        display_name,
        session_id,
        inbound_tx,
        outbound_rx,
    ));

    commands.insert_resource(TokioRuntime(rt));
    commands.insert_resource(NetBridge {
        inbound_rx: Mutex::new(inbound_rx),
        outbound_tx,
    });
}

fn flush_inbound(
    bridge: Res<NetBridge>,
    mut peer_connected: MessageWriter<PeerConnected>,
    mut peer_disconnected: MessageWriter<PeerDisconnected>,
    mut peer_discovered: MessageWriter<PeerDiscovered>,
    mut peer_lost: MessageWriter<PeerLost>,
    mut peer_msg: MessageWriter<PeerMessageReceived>,
) {
    let Ok(mut rx) = bridge.inbound_rx.try_lock() else {
        return;
    };
    while let Ok(event) = rx.try_recv() {
        match event {
            NetInbound::PeerConnected { peer_id, display_name, session_id } => {
                peer_connected.write(PeerConnected { peer_id, display_name, session_id });
            }
            NetInbound::PeerDisconnected { peer_id, graceful } => {
                peer_disconnected.write(PeerDisconnected { peer_id, graceful });
            }
            NetInbound::Message { from, channel, payload } => {
                peer_msg.write(PeerMessageReceived(RawMessage { from, channel, payload }));
            }
            NetInbound::PeerDiscovered { peer_id, display_name, session_id } => {
                peer_discovered.write(PeerDiscovered {
                    peer_id,
                    display_name,
                    has_session: session_id.is_some(),
                    session_id,
                });
            }
            NetInbound::PeerLost { peer_id } => {
                peer_lost.write(PeerLost { peer_id });
            }
        }
    }
}

fn flush_outbound(bridge: Res<NetBridge>, mut commands: MessageReader<NetCommand>) {
    for cmd in commands.read() {
        let outbound = match cmd {
            NetCommand::Broadcast { channel, payload } => NetOutbound::Broadcast {
                channel: *channel,
                payload: payload.clone(),
            },
            NetCommand::SendTo { peer_id, channel, payload } => NetOutbound::SendTo {
                peer_id: *peer_id,
                channel: *channel,
                payload: payload.clone(),
            },
            // StartSession / JoinSession / LeaveSession: auto-managed for now.
            _ => continue,
        };
        bridge.outbound_tx.send(outbound).ok();
    }
}
