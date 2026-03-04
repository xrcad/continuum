//! Touch device adapter — single-finger orbit, two-finger pan.

use std::collections::HashMap;

use bevy::input::InputSystems;
use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::prelude::*;

use crate::roles::role;
use crate::router::RoleMessage;

/// Orbit (azimuth / elevation) delta emitted by a single-finger drag.
#[derive(Message, Debug, Clone, Copy)]
pub struct OrbitDelta {
    /// Azimuth change in radians — positive rotates the camera counter-clockwise around
    /// the scene when viewed from above.
    pub azimuth: f32,
    /// Elevation change in radians — positive tilts the camera up.
    pub elevation: f32,
}

/// Ground-plane pan delta emitted by a two-finger drag.
#[derive(Message, Debug, Clone, Copy)]
pub struct PanDelta {
    /// Displacement along the camera-right axis in world units.
    pub dx: f32,
    /// Displacement along the camera-forward axis in world units.
    pub dz: f32,
}

const ORBIT_SENSITIVITY: f32 = 0.008;
const PAN_SENSITIVITY: f32 = 0.04;

pub struct TouchInputPlugin;

impl Plugin for TouchInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreUpdate, process_touch.after(InputSystems));
    }
}

/// Tracks the last known screen position of each active touch, keyed by touch id.
#[derive(Default)]
struct TouchTracker {
    touches: HashMap<u64, Vec2>,
}

fn process_touch(
    mut events: MessageReader<TouchInput>,
    mut tracker: Local<TouchTracker>,
    mut orbit_writer: MessageWriter<OrbitDelta>,
    mut pan_writer: MessageWriter<PanDelta>,
    mut role_writer: MessageWriter<RoleMessage>,
) {
    let all: Vec<TouchInput> = events.read().cloned().collect();
    if all.is_empty() {
        return;
    }

    // Update the active-touch set and detect count changes.
    let mut count_changed = false;
    for ev in &all {
        match ev.phase {
            TouchPhase::Started => {
                tracker.touches.insert(ev.id, ev.position);
                count_changed = true;
            }
            TouchPhase::Ended | TouchPhase::Canceled => {
                tracker.touches.remove(&ev.id);
                count_changed = true;
            }
            _ => {}
        }
    }

    // Even when the finger count changes, keep stored positions current for
    // Moved events so the delta on the very next frame isn't inflated.
    if count_changed {
        for ev in &all {
            if ev.phase == TouchPhase::Moved && tracker.touches.contains_key(&ev.id) {
                tracker.touches.insert(ev.id, ev.position);
            }
        }
        return;
    }

    let n = tracker.touches.len();
    if n == 0 {
        return;
    }

    // Accumulate movement deltas from all Moved events this frame.
    // Positions are updated incrementally so multiple Moved events for the same
    // touch in one frame are summed correctly (total = final_pos - initial_pos).
    let mut total_delta = Vec2::ZERO;
    let mut moved = 0usize;
    for ev in &all {
        if ev.phase != TouchPhase::Moved {
            continue;
        }
        if let Some(&prev) = tracker.touches.get(&ev.id) {
            total_delta += ev.position - prev;
            moved += 1;
        }
        tracker.touches.insert(ev.id, ev.position);
    }

    if moved == 0 {
        return;
    }

    let avg = total_delta / moved as f32;

    match n {
        1 => {
            role_writer.write(RoleMessage {
                role: role::NAVIGATE,
            });
            orbit_writer.write(OrbitDelta {
                azimuth: -avg.x * ORBIT_SENSITIVITY,
                elevation: avg.y * ORBIT_SENSITIVITY,
            });
        }
        2 => {
            role_writer.write(RoleMessage { role: role::PAN });
            pan_writer.write(PanDelta {
                dx: avg.x * PAN_SENSITIVITY,
                dz: -avg.y * PAN_SENSITIVITY,
            });
        }
        _ => {}
    }
}
