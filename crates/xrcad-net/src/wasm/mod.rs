//! WASM transport: WebSocket relay.
//!
//! All browsers connect to `ws[s]://<relay>/relay/room/default`.
//! The relay URL is read from the `?relay=` query parameter at startup;
//! if absent it defaults to the same origin (works when served by xrcad-server).
//!
//! Wire protocol:
//! - Text frame (Client→Server): `{"Join":{"peer_id":"<uuid>"}}`  (once, on open)
//! - Binary frame (Client→Server): postcard-encoded `PresenceMsg` bytes
//! - Text frame (Server→Client): `{"PeerJoined":"<uuid>"}` / `{"PeerLeft":"<uuid>"}`
//! - Binary frame (Server→Client): `[16-byte from-peer UUID][postcard PresenceMsg]`

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use bevy::prelude::*;
use js_sys::{ArrayBuffer, Uint8Array};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{BinaryType, CloseEvent, MessageEvent, WebSocket};

use crate::{
    Channel, LocalPeer, NetCommand, PeerConnected, PeerDisconnected, PeerDiscovered, PeerLost,
    PeerMessageReceived, RawMessage, SessionId,
};

// ─────────────────────────────────────────────────────────────────────────────
// Wire types (text-frame JSON control messages)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
enum ClientCtrl {
    Join { peer_id: String },
}

#[derive(Deserialize)]
enum ServerCtrl {
    PeerJoined(String),
    PeerLeft(String),
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal event queue (shared between JS callbacks and Bevy systems)
// ─────────────────────────────────────────────────────────────────────────────

enum WsInbound {
    PeerJoined(crate::PeerId),
    PeerLeft(crate::PeerId),
    /// Binary relay frame decoded into from-peer + postcard payload.
    Message {
        from: crate::PeerId,
        payload: Vec<u8>,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// NonSend resource (Rc is not Send — fine on single-threaded WASM)
// ─────────────────────────────────────────────────────────────────────────────

struct WasmWs {
    ws: WebSocket,
    events: Rc<RefCell<VecDeque<WsInbound>>>,
    session_id: SessionId,
    /// Set to true once onopen fires and we've sent the Join message.
    joined: Rc<RefCell<bool>>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Registration
// ─────────────────────────────────────────────────────────────────────────────

pub fn register(app: &mut App) {
    app.add_systems(Startup, startup_wasm)
        .add_systems(Update, (flush_inbound, flush_outbound));
}

// ─────────────────────────────────────────────────────────────────────────────
// Systems
// ─────────────────────────────────────────────────────────────────────────────

fn startup_wasm(world: &mut World) {
    let url = relay_url();
    tracing::info!("xrcad-net (wasm): connecting to {url}");

    let peer_id_str = world.resource::<LocalPeer>().peer_id.0.to_string();

    let ws = match WebSocket::new(&url) {
        Ok(w) => w,
        Err(e) => {
            tracing::error!("xrcad-net (wasm): WebSocket::new failed: {:?}", e);
            return;
        }
    };
    ws.set_binary_type(BinaryType::Arraybuffer);

    let events: Rc<RefCell<VecDeque<WsInbound>>> = Rc::new(RefCell::new(VecDeque::new()));
    let joined: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
    let session_id = SessionId::generate();

    // onopen — send Join
    {
        let ws_c = ws.clone();
        let joined_c = joined.clone();
        let cb = Closure::<dyn FnMut()>::new(move || {
            let msg = serde_json::to_string(&ClientCtrl::Join {
                peer_id: peer_id_str.clone(),
            })
            .unwrap_or_default();
            let _ = ws_c.send_with_str(&msg);
            *joined_c.borrow_mut() = true;
            tracing::info!("xrcad-net (wasm): relay connected, sent Join");
        });
        ws.set_onopen(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
    }

    // onmessage — parse text (PeerJoined/Left) or binary ([16-byte uuid][payload])
    {
        let events_c = events.clone();
        let cb = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            if let Ok(js_str) = e.data().dyn_into::<js_sys::JsString>() {
                let s = String::from(js_str);
                if let Ok(ctrl) = serde_json::from_str::<ServerCtrl>(&s) {
                    match ctrl {
                        ServerCtrl::PeerJoined(uuid_str) => {
                            if let Ok(uuid) = Uuid::parse_str(&uuid_str) {
                                events_c
                                    .borrow_mut()
                                    .push_back(WsInbound::PeerJoined(crate::PeerId(uuid)));
                            }
                        }
                        ServerCtrl::PeerLeft(uuid_str) => {
                            if let Ok(uuid) = Uuid::parse_str(&uuid_str) {
                                events_c
                                    .borrow_mut()
                                    .push_back(WsInbound::PeerLeft(crate::PeerId(uuid)));
                            }
                        }
                    }
                }
            } else if let Ok(ab) = e.data().dyn_into::<ArrayBuffer>() {
                let bytes = Uint8Array::new(&ab).to_vec();
                if bytes.len() >= 16 {
                    let from_bytes: [u8; 16] = bytes[..16].try_into().unwrap();
                    let from = crate::PeerId(Uuid::from_bytes(from_bytes));
                    let payload = bytes[16..].to_vec();
                    events_c
                        .borrow_mut()
                        .push_back(WsInbound::Message { from, payload });
                }
            }
        });
        ws.set_onmessage(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
    }

    // onclose — log; reconnect not implemented for MVP
    {
        let cb = Closure::<dyn FnMut(CloseEvent)>::new(|e: CloseEvent| {
            tracing::warn!("xrcad-net (wasm): relay closed (code {})", e.code());
        });
        ws.set_onclose(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
    }

    world.insert_non_send_resource(WasmWs {
        ws,
        events,
        session_id,
        joined,
    });
}

fn flush_inbound(
    ws: Option<NonSend<WasmWs>>,
    mut peer_connected: MessageWriter<PeerConnected>,
    mut peer_disconnected: MessageWriter<PeerDisconnected>,
    mut peer_discovered: MessageWriter<PeerDiscovered>,
    mut peer_lost: MessageWriter<PeerLost>,
    mut peer_msg: MessageWriter<PeerMessageReceived>,
) {
    let Some(ws) = ws else { return };
    let session_id = ws.session_id;

    let mut queue = ws.events.borrow_mut();
    while let Some(event) = queue.pop_front() {
        match event {
            WsInbound::PeerJoined(peer_id) => {
                peer_discovered.write(PeerDiscovered {
                    peer_id,
                    display_name: None,
                    has_session: true,
                    session_id: Some(session_id),
                });
                peer_connected.write(PeerConnected {
                    peer_id,
                    display_name: None,
                    session_id,
                });
            }
            WsInbound::PeerLeft(peer_id) => {
                peer_disconnected.write(PeerDisconnected {
                    peer_id,
                    graceful: true,
                });
                peer_lost.write(PeerLost { peer_id });
            }
            WsInbound::Message { from, payload } => {
                peer_msg.write(PeerMessageReceived(RawMessage {
                    from,
                    channel: Channel::Unreliable,
                    payload,
                }));
            }
        }
    }
}

fn flush_outbound(ws: Option<NonSend<WasmWs>>, mut commands: MessageReader<NetCommand>) {
    let Some(ws) = ws else { return };
    if !*ws.joined.borrow() {
        return; // not connected yet
    }
    for cmd in commands.read() {
        if let NetCommand::Broadcast { payload, .. } = cmd {
            let _ = ws.ws.send_with_u8_array(payload);
        }
        // SendTo is unsupported on a broadcast relay; silently drop.
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Relay URL resolution
// ─────────────────────────────────────────────────────────────────────────────

/// Build the relay WebSocket URL.
///
/// Checks `?relay=<url>` in the page query string first.
/// Falls back to the same origin so it works zero-config when served by
/// `xrcad-server` directly on the LAN.
fn relay_url() -> String {
    let window = web_sys::window().expect("no window");
    let location = window.location();

    if let Ok(search) = location.search() {
        for part in search.trim_start_matches('?').split('&') {
            if let Some(encoded) = part.strip_prefix("relay=") {
                let decoded = js_sys::decode_uri_component(encoded)
                    .ok()
                    .and_then(|v| v.as_string())
                    .unwrap_or_else(|| encoded.to_string());
                return format!("{}/relay/room/default", decoded.trim_end_matches('/'));
            }
        }
    }

    let protocol = location.protocol().unwrap_or_default();
    let host = location.host().unwrap_or_default();
    let ws_proto = if protocol == "https:" { "wss:" } else { "ws:" };
    format!("{ws_proto}//{host}/relay/room/default")
}
