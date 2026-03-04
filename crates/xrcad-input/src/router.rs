//! Routes raw input events to role messages.

use super::roles::InputRole;
use bevy::prelude::*;

/// A routed input event tagged with its role.
#[derive(Event, Debug, Clone)]
pub struct RoleMessage {
    pub role: InputRole,
}

pub fn route_input() {
    // TODO: implement routing
}
