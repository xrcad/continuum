//! Mouse device adapter — left-drag to orbit, right-drag to pan.

use bevy::input::InputSystems;
use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;

use crate::roles::role;
use crate::router::RoleMessage;
use crate::touch::{OrbitDelta, PanDelta};

const ORBIT_SENSITIVITY: f32 = 0.008;
const PAN_SENSITIVITY: f32 = 0.04;

pub struct MouseInputPlugin;

impl Plugin for MouseInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreUpdate, process_mouse.after(InputSystems));
    }
}

fn process_mouse(
    mut motion: MessageReader<MouseMotion>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut orbit_writer: MessageWriter<OrbitDelta>,
    mut pan_writer: MessageWriter<PanDelta>,
    mut role_writer: MessageWriter<RoleMessage>,
) {
    let mut delta = Vec2::ZERO;
    for ev in motion.read() {
        delta += ev.delta;
    }
    if delta == Vec2::ZERO {
        return;
    }

    if buttons.pressed(MouseButton::Left) {
        role_writer.write(RoleMessage {
            role: role::NAVIGATE,
        });
        orbit_writer.write(OrbitDelta {
            azimuth: -delta.x * ORBIT_SENSITIVITY,
            elevation: delta.y * ORBIT_SENSITIVITY,
        });
    } else if buttons.pressed(MouseButton::Right) {
        role_writer.write(RoleMessage { role: role::PAN });
        pan_writer.write(PanDelta {
            dx: delta.x * PAN_SENSITIVITY,
            dz: -delta.y * PAN_SENSITIVITY,
        });
    }
}
