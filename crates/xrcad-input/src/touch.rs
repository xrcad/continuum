//! Touch device adapter — single-finger orbit, two-finger pan.

use std::collections::HashMap;

use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::input::InputSystem;
use bevy::prelude::*;

/// Orbit (azimuth / elevation) delta emitted by a single-finger drag.
#[derive(Event, Debug, Clone, Copy)]
pub struct OrbitDelta {
    /// Azimuth change in radians — positive rotates the camera counter-clockwise around
    /// the scene when viewed from above.
    pub azimuth: f32,
    /// Elevation change in radians — positive tilts the camera up.
    pub elevation: f32,
}

/// Ground-plane pan delta emitted by a two-finger drag.
#[derive(Event, Debug, Clone, Copy)]
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
        app.add_event::<OrbitDelta>()
            .add_event::<PanDelta>()
            .add_systems(PreUpdate, process_touch.after(InputSystem));
    }
}

/// Tracks the last known screen position of each active touch, keyed by touch id.
#[derive(Default)]
struct TouchTracker {
    touches: HashMap<u64, Vec2>,
}

fn process_touch(
    mut events: EventReader<TouchInput>,
    mut tracker: Local<TouchTracker>,
    mut orbit_writer: EventWriter<OrbitDelta>,
    mut pan_writer: EventWriter<PanDelta>,
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

    // Skip gesture output on the frame the finger count changes to avoid position jumps.
    if count_changed {
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
        1 => orbit_writer.send(OrbitDelta {
            azimuth: -avg.x * ORBIT_SENSITIVITY,
            elevation: -avg.y * ORBIT_SENSITIVITY,
        }),
        2 => pan_writer.send(PanDelta {
            dx: avg.x * PAN_SENSITIVITY,
            dz: -avg.y * PAN_SENSITIVITY,
        }),
        _ => {}
    }
}
