//! Per-peer TCP connection: handshake, writer task, reader loop.

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use super::framing::{read_frame, write_frame};
use crate::{Channel, PeerId, SessionId};

// ─────────────────────────────────────────────────────────────────────────────
// Wire types
// ─────────────────────────────────────────────────────────────────────────────

/// Message sent on the wire (postcard-encoded, length-prefixed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) enum WireMsg {
    /// First message exchanged after TCP connect — establishes identity.
    Handshake {
        peer_id: PeerId,
        display_name: String,
        session_id: SessionId,
    },
    /// A data payload forwarded from the collaboration layer.
    Payload { channel: Channel, payload: Vec<u8> },
}

// ─────────────────────────────────────────────────────────────────────────────
// Peer ↔ coordinator channel types
// ─────────────────────────────────────────────────────────────────────────────

/// Command sent from the coordinator to a peer's writer task.
pub(super) enum PeerCmd {
    /// A pre-assembled frame (length-prefix + postcard WireMsg::Payload bytes).
    Send(Vec<u8>),
}

/// Events a peer task sends back to the coordinator.
pub(super) enum PeerEvent {
    Connected {
        peer_id: PeerId,
        display_name: String,
        session_id: SessionId,
        /// Channel to write frames to this peer's TCP socket.
        writer: mpsc::UnboundedSender<PeerCmd>,
    },
    Disconnected {
        peer_id: PeerId,
        graceful: bool,
    },
    Message {
        from: PeerId,
        channel: Channel,
        payload: Vec<u8>,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// Connection lifecycle
// ─────────────────────────────────────────────────────────────────────────────

/// Run the full lifecycle of one TCP peer connection:
/// 1. Exchange handshakes.
/// 2. Spawn a writer task that drains `PeerCmd::Send` frames to the socket.
/// 3. Loop reading frames from the socket and forwarding to the coordinator.
///
/// Returns when the connection closes (gracefully or by error).
pub(super) async fn run_peer(
    stream: TcpStream,
    local_peer_id: PeerId,
    local_display_name: String,
    local_session_id: SessionId,
    events_tx: mpsc::UnboundedSender<PeerEvent>,
) {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // ── Send our handshake ────────────────────────────────────────────────────
    let hs = WireMsg::Handshake {
        peer_id: local_peer_id,
        display_name: local_display_name,
        session_id: local_session_id,
    };
    let hs_bytes = match postcard::to_allocvec(&hs) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("xrcad-net: handshake serialize: {e}");
            return;
        }
    };
    if write_frame(&mut write_half, &hs_bytes).await.is_err() {
        return;
    }

    // ── Receive remote handshake ──────────────────────────────────────────────
    let frame = match read_frame(&mut reader).await {
        Ok(f) => f,
        Err(_) => return,
    };
    let (remote_peer_id, remote_display_name, session_id) =
        match postcard::from_bytes::<WireMsg>(&frame) {
            Ok(WireMsg::Handshake {
                peer_id,
                display_name,
                session_id,
            }) => (peer_id, display_name, session_id),
            _ => return, // protocol error
        };

    // Guard against self-connections (mDNS can resolve our own address).
    if remote_peer_id == local_peer_id {
        return;
    }

    // ── Notify coordinator of successful connection ───────────────────────────
    let (writer_tx, mut writer_rx) = mpsc::unbounded_channel::<PeerCmd>();
    events_tx
        .send(PeerEvent::Connected {
            peer_id: remote_peer_id,
            display_name: remote_display_name,
            session_id,
            writer: writer_tx,
        })
        .ok();

    // ── Spawn writer task ─────────────────────────────────────────────────────
    tokio::spawn(async move {
        while let Some(PeerCmd::Send(frame)) = writer_rx.recv().await {
            if write_half.write_all(&frame).await.is_err() {
                break;
            }
        }
    });

    // ── Reader loop ───────────────────────────────────────────────────────────
    loop {
        match read_frame(&mut reader).await {
            Ok(frame) => {
                if let Ok(WireMsg::Payload { channel, payload }) =
                    postcard::from_bytes::<WireMsg>(&frame)
                {
                    events_tx
                        .send(PeerEvent::Message {
                            from: remote_peer_id,
                            channel,
                            payload,
                        })
                        .ok();
                }
                // Other WireMsg variants are silently ignored (future-compat).
            }
            Err(_) => {
                events_tx
                    .send(PeerEvent::Disconnected {
                        peer_id: remote_peer_id,
                        graceful: false,
                    })
                    .ok();
                return;
            }
        }
    }
}
