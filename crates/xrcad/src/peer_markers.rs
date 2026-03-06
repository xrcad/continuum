//! Spawns and updates a cone entity for each remote peer that has a viewport,
//! showing where they are positioned and looking in the scene.

use bevy::prelude::*;
use xrcad_collab::presence::PresenceState;
use xrcad_net::PeerId;

pub struct PeerMarkerPlugin;

impl Plugin for PeerMarkerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_peer_markers);
    }
}

/// Marker component — one entity per remote peer.
#[derive(Component)]
struct PeerMarker(PeerId);

fn sync_peer_markers(
    presence: Res<PresenceState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut existing: Query<(Entity, &PeerMarker, &mut Transform)>,
) {
    // Update transforms or despawn markers for peers that have left / lost viewport.
    for (entity, marker, mut transform) in &mut existing {
        match presence.peers.get(&marker.0).and_then(|p| p.msg.viewport.as_ref()) {
            Some(vp) => {
                let eye = Vec3::from(vp.eye);
                let target = Vec3::from(vp.target);
                if (target - eye).length_squared() > 1e-6 {
                    *transform = Transform::from_translation(eye).looking_at(target, Vec3::Y);
                }
            }
            None => {
                commands.entity(entity).despawn();
            }
        }
    }

    // Spawn markers for new peers.
    let existing_ids: Vec<PeerId> = existing.iter().map(|(_, m, _)| m.0).collect();
    for (peer_id, peer) in &presence.peers {
        if existing_ids.contains(peer_id) {
            continue;
        }
        let Some(vp) = &peer.msg.viewport else { continue };

        let eye = Vec3::from(vp.eye);
        let target = Vec3::from(vp.target);
        let transform = if (target - eye).length_squared() > 1e-6 {
            Transform::from_translation(eye).looking_at(target, Vec3::Y)
        } else {
            Transform::from_translation(eye)
        };

        let [r, g, b] = peer.msg.peer_colour;
        commands.spawn((
            Mesh3d(meshes.add(Cone { radius: 0.15, height: 0.4 })),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(r, g, b),
                unlit: true,
                ..default()
            })),
            transform,
            PeerMarker(*peer_id),
        ));
    }
}
