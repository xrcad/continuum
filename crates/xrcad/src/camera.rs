//! Orbit camera — spherical-coordinate state, inertia, and update system.

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;
use xrcad_input::{OrbitDelta, PanDelta};
use xrcad_net::LocalCameraState;

/// Fraction of velocity remaining after one second of coasting (no touch input).
/// 0.01 means ~1 % remains after 1 s, giving a natural ~0.5 s deceleration.
const INERTIA_DECAY: f32 = 0.01;

/// Spherical-coordinate orbit state attached to the camera entity.
///
/// Camera position is computed as:
/// ```text
/// x = target.x + distance * cos(elevation) * sin(azimuth)
/// y = target.y + distance * sin(elevation)
/// z = target.z + distance * cos(elevation) * cos(azimuth)
/// ```
#[derive(Component)]
pub struct OrbitCamera {
    /// Point the camera orbits around and looks at.
    pub target: Vec3,
    /// Rotation around the world Y-axis in radians.
    pub azimuth: f32,
    /// Angle above the horizontal plane in radians. Clamped away from 0 and ±90°.
    pub elevation: f32,
    /// Distance from target to camera in world units.
    pub distance: f32,
    /// Inertia velocity — azimuth (x) and elevation (y) in radians/frame.
    pub orbit_vel: Vec2,
    /// Inertia velocity — camera-right (x) and camera-forward (y) in world units/frame.
    pub pan_vel: Vec2,
}

impl OrbitCamera {
    pub fn compute_transform(&self) -> Transform {
        orbit_transform(self.target, self.azimuth, self.elevation, self.distance)
    }
}

/// Compute a camera [`Transform`] from spherical orbit parameters.
///
/// The camera is placed at the spherical position and oriented to look at
/// `target`. Shared between the live camera and the peer marker system.
pub fn orbit_transform(target: Vec3, azimuth: f32, elevation: f32, distance: f32) -> Transform {
    let (sin_az, cos_az) = azimuth.sin_cos();
    let (sin_el, cos_el) = elevation.sin_cos();
    let pos = target
        + Vec3::new(
            distance * cos_el * sin_az,
            distance * sin_el,
            distance * cos_el * cos_az,
        );
    Transform::from_translation(pos).looking_at(target, Vec3::Y)
}

pub struct OrbitCameraPlugin;

impl Plugin for OrbitCameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (update_camera, publish_local_camera_state).chain());
    }
}

/// Writes the local camera state into [`LocalCameraState`] so that
/// `xrcad-net` can broadcast it without creating a circular crate dependency.
fn publish_local_camera_state(
    cameras: Query<&OrbitCamera>,
    mut local: ResMut<LocalCameraState>,
) {
    let Ok(cam) = cameras.single() else { return };
    local.target = cam.target;
    local.azimuth = cam.azimuth;
    local.elevation = cam.elevation;
    local.distance = cam.distance;
}

/// Reads `OrbitDelta` and `PanDelta` events, applies them as velocity, and
/// coasts with exponential decay when no input arrives.
fn update_camera(
    mut orbit_events: MessageReader<OrbitDelta>,
    mut pan_events: MessageReader<PanDelta>,
    mut cameras: Query<(&mut OrbitCamera, &mut Transform)>,
    time: Res<Time>,
) {
    let Ok((mut cam, mut transform)) = cameras.single_mut() else {
        return;
    };

    let decay = INERTIA_DECAY.powf(time.delta_secs());

    // Orbit — accumulate deltas; if any arrived this frame, they become the new velocity.
    let mut az = 0.0_f32;
    let mut el = 0.0_f32;
    let mut had_orbit = false;
    for ev in orbit_events.read() {
        az += ev.azimuth;
        el += ev.elevation;
        had_orbit = true;
    }
    if had_orbit {
        cam.orbit_vel = Vec2::new(az, el);
    } else {
        cam.orbit_vel *= decay;
    }

    // Pan — same pattern.
    let mut dx = 0.0_f32;
    let mut dz = 0.0_f32;
    let mut had_pan = false;
    for ev in pan_events.read() {
        dx += ev.dx;
        dz += ev.dz;
        had_pan = true;
    }
    if had_pan {
        cam.pan_vel = Vec2::new(dx, dz);
    } else {
        cam.pan_vel *= decay;
    }

    // Apply orbit velocity.
    cam.azimuth += cam.orbit_vel.x;
    cam.elevation = (cam.elevation + cam.orbit_vel.y).clamp(0.05, FRAC_PI_2 - 0.05);

    // Apply pan velocity — camera-relative axes projected onto the ground plane.
    // Copy pan_vel first to satisfy the borrow checker.
    let pan_vel = cam.pan_vel;
    if pan_vel != Vec2::ZERO {
        let (sin_az, cos_az) = cam.azimuth.sin_cos();
        let right = Vec3::new(cos_az, 0.0, -sin_az);
        let forward = Vec3::new(-sin_az, 0.0, -cos_az);
        cam.target += right * pan_vel.x + forward * pan_vel.y;
    }

    *transform = cam.compute_transform();
}
