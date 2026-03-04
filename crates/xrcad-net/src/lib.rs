//! Client-side networking plugin for xrcad.
//!
//! On startup the plugin derives the relay URL from the page origin (WASM) or
//! falls back to `ws://localhost:8080/relay/room/default` (native), then
//! attempts a non-blocking connection via `ewebsock`.
//!
//! The rest of the app only touches two resources:
//! - [`NetClient`] — connection state; press `N` to toggle
//! - [`PeerCameras`] — live map of every connected peer's camera state

pub mod messages;

use std::{collections::HashMap, sync::Mutex};

use bevy::prelude::*;
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
pub use messages::{ClientMsg, PeerCameraState, ServerMsg};
use uuid::Uuid;

const BROADCAST_INTERVAL_SECS: f32 = 0.1;

// ── Resources ────────────────────────────────────────────────────────────────

/// WebSocket connection state held as a Bevy [`Resource`].
///
/// `WsReceiver` is `!Sync` (it wraps `mpsc::Receiver`) so we wrap it in a
/// `Mutex` to satisfy Bevy's `Resource: Send + Sync` requirement. The Mutex is
/// only locked from the single `receive_msgs` system so there is no contention.
#[derive(Resource)]
pub struct NetClient {
    pub peer_id: Uuid,
    pub relay_url: String,
    state: NetState,
    broadcast_timer: f32,
}

enum NetState {
    Disconnected,
    Live {
        tx: WsSender,
        rx: Mutex<WsReceiver>,
    },
}

// SAFETY: WsSender is Send+Sync; WsReceiver is Send+!Sync.
// Wrapping rx in Mutex makes NetState Send+Sync.
unsafe impl Sync for NetState {}

impl NetClient {
    fn new(relay_url: String) -> Self {
        Self {
            peer_id: Uuid::new_v4(),
            relay_url,
            state: NetState::Disconnected,
            broadcast_timer: 0.0,
        }
    }

    pub fn connect(&mut self) {
        match ewebsock::connect(&self.relay_url, ewebsock::Options::default()) {
            Ok((tx, rx)) => {
                self.state = NetState::Live {
                    tx,
                    rx: Mutex::new(rx),
                };
            }
            Err(e) => {
                warn!("xrcad-net: could not connect to {}: {e}", self.relay_url);
            }
        }
    }

    pub fn disconnect(&mut self) {
        self.state = NetState::Disconnected;
    }

    pub fn is_connected(&self) -> bool {
        matches!(self.state, NetState::Live { .. })
    }

    fn send(&mut self, msg: &ClientMsg) {
        let NetState::Live { tx, .. } = &mut self.state else {
            return;
        };
        match serde_json::to_string(msg) {
            Ok(text) => tx.send(WsMessage::Text(text)),
            Err(e) => warn!("xrcad-net: serialisation error: {e}"),
        }
    }

    /// Drain all pending inbound events. Returns `None` if disconnected.
    fn drain(&mut self) -> Option<Vec<WsEvent>> {
        let NetState::Live { rx, .. } = &mut self.state else {
            return None;
        };
        let rx = rx.get_mut().unwrap();
        let mut events = Vec::new();
        loop {
            match rx.try_recv() {
                Some(ev) => events.push(ev),
                None => return Some(events),
            }
        }
    }
}

/// Live map of every peer currently in the room.
#[derive(Resource, Default)]
pub struct PeerCameras(pub HashMap<Uuid, PeerCameraState>);

// ── LocalCameraState resource ─────────────────────────────────────────────────

/// Written by `xrcad`'s camera system each frame; read here for broadcasting.
/// Avoids a circular crate dependency between xrcad and xrcad-net.
#[derive(Resource, Default)]
pub struct LocalCameraState {
    pub target: Vec3,
    pub azimuth: f32,
    pub elevation: f32,
    pub distance: f32,
}

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct NetPlugin;

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        let relay_url = derive_relay_url();
        info!("xrcad-net: relay URL = {relay_url}");

        let mut client = NetClient::new(relay_url);
        client.connect(); // best-effort; failure is silently ignored

        app.insert_resource(client)
            .insert_resource(PeerCameras::default())
            .add_systems(Update, receive_msgs)
            .add_systems(Update, broadcast_camera.after(receive_msgs))
            .add_systems(Update, reconnect_key.after(broadcast_camera));
    }
}

// ── Systems ────────────────────────────────────────────────────────────────────

fn receive_msgs(mut client: ResMut<NetClient>, mut peers: ResMut<PeerCameras>) {
    let Some(events) = client.drain() else { return };

    let peer_id = client.peer_id;
    let mut disconnected = false;

    for event in events {
        match event {
            WsEvent::Opened => {
                info!("xrcad-net: connected as {peer_id}");
                client.send(&ClientMsg::Join { peer_id });
            }
            WsEvent::Message(WsMessage::Text(text)) => {
                match serde_json::from_str::<ServerMsg>(&text) {
                    Ok(ServerMsg::PeerJoined(id)) => {
                        info!("xrcad-net: peer joined {id}");
                    }
                    Ok(ServerMsg::PeerLeft(id)) => {
                        peers.0.remove(&id);
                    }
                    Ok(ServerMsg::Camera(state)) => {
                        peers.0.insert(state.peer_id, state);
                    }
                    Err(e) => warn!("xrcad-net: bad message: {e}"),
                }
            }
            WsEvent::Error(e) => {
                warn!("xrcad-net: socket error: {e}");
                disconnected = true;
            }
            WsEvent::Closed => {
                info!("xrcad-net: disconnected");
                disconnected = true;
            }
            _ => {}
        }
    }

    if disconnected {
        client.disconnect();
        peers.0.clear();
    }
}

fn broadcast_camera(
    mut client: ResMut<NetClient>,
    camera_state: Option<Res<LocalCameraState>>,
    time: Res<Time>,
) {
    if !client.is_connected() {
        return;
    }
    client.broadcast_timer += time.delta_secs();
    if client.broadcast_timer < BROADCAST_INTERVAL_SECS {
        return;
    }
    client.broadcast_timer = 0.0;

    let Some(state) = camera_state else { return };
    let peer_id = client.peer_id;
    client.send(&ClientMsg::Camera(PeerCameraState {
        peer_id,
        target: state.target.into(),
        azimuth: state.azimuth,
        elevation: state.elevation,
        distance: state.distance,
    }));
}

fn reconnect_key(keys: Res<ButtonInput<KeyCode>>, mut client: ResMut<NetClient>) {
    if keys.just_pressed(KeyCode::KeyN) {
        if client.is_connected() {
            client.disconnect();
        } else {
            client.connect();
        }
    }
}

// ── URL derivation ─────────────────────────────────────────────────────────────

fn derive_relay_url() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        let origin = web_sys::window()
            .and_then(|w| w.location().origin().ok())
            .unwrap_or_else(|| "http://localhost:8080".to_string());
        let ws_origin = origin
            .replace("https://", "wss://")
            .replace("http://", "ws://");
        format!("{ws_origin}/relay/room/default")
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        "ws://localhost:8080/relay/room/default".to_string()
    }
}
