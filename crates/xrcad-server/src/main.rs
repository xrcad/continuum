//! `xrcad-server` — serves the WASM app files and hosts the WebSocket relay
//! on a single port so users need only run one binary.
//!
//! Usage:
//!   cargo run -p xrcad-server [-- --port 8080 --dir ./wasm]
//!
//! Wire protocol:
//! - Text frame (Client→Server): `{"Join":{"peer_id":"<uuid-str>"}}` (once on open)
//! - Binary frame (Client→Server): postcard PresenceMsg bytes (opaque)
//! - Text frame (Server→Client): `{"PeerJoined":"<uuid-str>"}` / `{"PeerLeft":"<uuid-str>"}`
//! - Binary frame (Server→Client): [16-byte from-peer UUID][payload]

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tower_http::services::ServeDir;
use uuid::Uuid;

// ── Relay protocol types ──────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
enum ClientMsg {
    Join { peer_id: String },
}

#[derive(Serialize, Debug)]
enum ServerMsg {
    PeerJoined(String),
    PeerLeft(String),
}

// ── Shared state ─────────────────────────────────────────────────────────────

type PeerTx = mpsc::UnboundedSender<Message>;

/// room_id → list of (peer_id_str, uuid_bytes, sender)
type Rooms = Arc<Mutex<HashMap<String, Vec<(String, [u8; 16], PeerTx)>>>>;

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let port = arg_value(&args, "--port").unwrap_or("8080");
    let dir = arg_value(&args, "--dir").unwrap_or("./wasm");

    let rooms: Rooms = Arc::new(Mutex::new(HashMap::new()));

    let app = Router::new()
        .route("/relay/room/{room_id}", get(ws_handler))
        .fallback_service(ServeDir::new(dir))
        .with_state(rooms);

    let addr: SocketAddr = format!("0.0.0.0:{port}").parse().unwrap();
    println!("xrcad-server: listening on http://{addr}  (relay at /relay/room/:id)");
    println!("xrcad-server: serving static files from '{dir}'");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ── WebSocket relay handler ───────────────────────────────────────────────────

async fn ws_handler(
    ws: WebSocketUpgrade,
    Path(room_id): Path<String>,
    State(rooms): State<Rooms>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, room_id, rooms))
}

async fn handle_socket(mut socket: WebSocket, room_id: String, rooms: Rooms) {
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    // (peer_id_str, uuid_bytes)
    let mut peer: Option<(String, [u8; 16])> = None;

    loop {
        tokio::select! {
            // Outbound: relay forwarded messages to this peer.
            Some(msg) = rx.recv() => {
                if socket.send(msg).await.is_err() {
                    break;
                }
            }
            // Inbound: message from this peer.
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ClientMsg>(&text) {
                            Ok(ClientMsg::Join { peer_id: id_str }) => {
                                // Parse the UUID string to get raw bytes for binary relay.
                                let uuid_bytes = Uuid::parse_str(&id_str)
                                    .map(|u| *u.as_bytes())
                                    .unwrap_or([0u8; 16]);
                                peer = Some((id_str.clone(), uuid_bytes));
                                let mut guard = rooms.lock().unwrap();
                                let room = guard.entry(room_id.clone()).or_default();
                                broadcast_text(room, &id_str, &ServerMsg::PeerJoined(id_str.clone()));
                                room.push((id_str, uuid_bytes, tx.clone()));
                            }
                            Err(e) => eprintln!("relay: bad text message: {e}"),
                        }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        // Prepend the 16-byte sender UUID and relay to all others.
                        if let Some((ref id_str, uuid_bytes)) = peer {
                            let mut frame = Vec::with_capacity(16 + data.len());
                            frame.extend_from_slice(&uuid_bytes);
                            frame.extend_from_slice(&data);
                            let guard = rooms.lock().unwrap();
                            if let Some(room) = guard.get(&room_id) {
                                relay_binary(room, id_str, frame);
                            }
                        }
                    }
                    _ => break,
                }
            }
        }
    }

    // Clean up on disconnect.
    if let Some((id_str, _)) = peer {
        let mut guard = rooms.lock().unwrap();
        if let Some(room) = guard.get_mut(&room_id) {
            room.retain(|(pid, _, _)| pid != &id_str);
            broadcast_text(room, &id_str, &ServerMsg::PeerLeft(id_str.clone()));
            if room.is_empty() {
                guard.remove(&room_id);
            }
        }
    }
}

/// Broadcast a JSON control message to all peers in `room` except `except_id`.
fn broadcast_text(room: &[(String, [u8; 16], PeerTx)], except_id: &str, msg: &ServerMsg) {
    let Ok(text) = serde_json::to_string(msg) else {
        return;
    };
    for (id, _, tx) in room {
        if id != except_id {
            let _ = tx.send(Message::Text(text.clone().into()));
        }
    }
}

/// Relay an opaque binary frame to all peers in `room` except `except_id`.
fn relay_binary(room: &[(String, [u8; 16], PeerTx)], except_id: &str, frame: Vec<u8>) {
    for (id, _, tx) in room {
        if id != except_id {
            let _ = tx.send(Message::Binary(frame.clone().into()));
        }
    }
}

// ── CLI helpers ───────────────────────────────────────────────────────────────

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].as_str())
}
