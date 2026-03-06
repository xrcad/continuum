//! `xrcad-server` — serves the WASM app files and hosts the WebSocket relay
//! on a single port so users need only run one binary.
//!
//! Usage:
//!   cargo run -p xrcad-server [-- --port 8080 --dir ./wasm]
//!
//! All WASM clients connect to `ws://<host>:<port>/relay/room/<id>` and are
//! automatically discovered from `window.location.origin` inside the WASM app.

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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PeerCameraState {
    pub peer_id: Uuid,
    pub target: [f32; 3],
    pub azimuth: f32,
    pub elevation: f32,
    pub distance: f32,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ClientMsg {
    Join { peer_id: Uuid },
    Camera(PeerCameraState),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ServerMsg {
    PeerJoined(Uuid),
    PeerLeft(Uuid),
    Camera(PeerCameraState),
}

// ── Shared state ─────────────────────────────────────────────────────────────

type PeerTx = mpsc::UnboundedSender<String>;

/// room_id → list of (peer_id, sender)
type Rooms = Arc<Mutex<HashMap<String, Vec<(Uuid, PeerTx)>>>>;

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
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let mut peer_id: Option<Uuid> = None;

    loop {
        tokio::select! {
            // Outbound: relay forwarded messages to this peer.
            Some(text) = rx.recv() => {
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            // Inbound: message from this peer.
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ClientMsg>(&text) {
                            Ok(ClientMsg::Join { peer_id: id }) => {
                                peer_id = Some(id);
                                let mut guard = rooms.lock().unwrap();
                                let room = guard.entry(room_id.clone()).or_default();
                                broadcast(room, id, &ServerMsg::PeerJoined(id));
                                room.push((id, tx.clone()));
                            }
                            Ok(ClientMsg::Camera(state)) => {
                                if let Some(id) = peer_id {
                                    let mut guard = rooms.lock().unwrap();
                                    if let Some(room) = guard.get_mut(&room_id) {
                                        broadcast(room, id, &ServerMsg::Camera(state));
                                    }
                                }
                            }
                            Err(e) => eprintln!("relay: bad message: {e}"),
                        }
                    }
                    _ => break,
                }
            }
        }
    }

    // Clean up on disconnect.
    if let Some(id) = peer_id {
        let mut guard = rooms.lock().unwrap();
        if let Some(room) = guard.get_mut(&room_id) {
            room.retain(|(pid, _)| *pid != id);
            broadcast(room, id, &ServerMsg::PeerLeft(id));
            if room.is_empty() {
                guard.remove(&room_id);
            }
        }
    }
}

/// Send `msg` to every peer in `room` except `except_id`.
fn broadcast(room: &[(Uuid, PeerTx)], except_id: Uuid, msg: &ServerMsg) {
    let Ok(text) = serde_json::to_string(msg) else {
        return;
    };
    for (id, tx) in room {
        if *id != except_id {
            let _ = tx.send(text.clone());
        }
    }
}

// ── CLI helpers ───────────────────────────────────────────────────────────────

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].as_str())
}
