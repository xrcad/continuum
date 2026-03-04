//! Spawns and updates a cone entity for each connected peer, showing where
//! they are positioned and looking in the scene.

use bevy::prelude::*;
use uuid::Uuid;
use xrcad_net::PeerCameras;

use crate::camera::orbit_transform;

pub struct PeerMarkerPlugin;

impl Plugin for PeerMarkerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_peer_markers);
    }
}

/// Marker component — one per connected peer.
#[derive(Component)]
struct PeerMarker(Uuid);

fn sync_peer_markers(
    mut commands: Commands,
    peers: Res<PeerCameras>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut existing: Query<(Entity, &PeerMarker, &mut Transform)>,
) {
    // Update or despawn existing markers.
    for (entity, marker, mut transform) in &mut existing {
        match peers.0.get(&marker.0) {
            Some(state) => {
                *transform = orbit_transform(
                    state.target.into(),
                    state.azimuth,
                    state.elevation,
                    state.distance,
                );
            }
            None => {
                commands.entity(entity).despawn();
            }
        }
    }

    // Spawn markers for new peers.
    let existing_ids: Vec<Uuid> = existing.iter().map(|(_, m, _)| m.0).collect();
    for (peer_id, state) in &peers.0 {
        if existing_ids.contains(peer_id) {
            continue;
        }

        let color = peer_color(*peer_id);
        let transform = orbit_transform(
            state.target.into(),
            state.azimuth,
            state.elevation,
            state.distance,
        );

        commands.spawn((
            Mesh3d(meshes.add(Cone {
                radius: 0.15,
                height: 0.4,
            })),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                unlit: true,
                ..default()
            })),
            transform,
            PeerMarker(*peer_id),
        ));
    }
}

/// Derive a stable, visually distinct colour from a peer UUID.
fn peer_color(id: Uuid) -> Color {
    let bytes = id.as_bytes();
    let h = (bytes[0] as f32 / 255.0) * 360.0;
    Color::hsl(h, 0.9, 0.6)
}
