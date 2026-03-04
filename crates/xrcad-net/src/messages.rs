use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Snapshot of a peer's orbit camera state, broadcast every 100 ms.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PeerCameraState {
    pub peer_id: Uuid,
    /// World-space orbit target.
    pub target: [f32; 3],
    pub azimuth: f32,
    pub elevation: f32,
    pub distance: f32,
}

/// Messages sent from a client to the relay server.
#[derive(Serialize, Deserialize, Debug)]
pub enum ClientMsg {
    /// Sent once on connect to register with the room.
    Join { peer_id: Uuid },
    /// Periodic camera position broadcast.
    Camera(PeerCameraState),
}

/// Messages sent from the relay server to each client.
#[derive(Serialize, Deserialize, Debug)]
pub enum ServerMsg {
    /// Another peer joined the same room.
    PeerJoined(Uuid),
    /// A peer disconnected or left.
    PeerLeft(Uuid),
    /// A peer's latest camera state.
    Camera(PeerCameraState),
}
