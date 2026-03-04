//! Input roles — abstract device-agnostic actions.

use bevy::prelude::*;

/// An abstract input role, independent of device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InputRole(pub &'static str);

pub mod role {
    use super::InputRole;
    /// Single-finger orbit / camera navigation gesture.
    pub const NAVIGATE: InputRole = InputRole("navigate");
    /// Two-finger ground-plane pan gesture.
    pub const PAN: InputRole = InputRole("pan");
    // pub const SELECT: InputRole = InputRole("select");
    // pub const DRAW: InputRole   = InputRole("draw");
    // pub const ERASE: InputRole  = InputRole("erase");
    // pub const CONTEXT: InputRole = InputRole("context");
}

/// Resource mapping device matchers to roles.
#[derive(Resource, Default)]
pub struct InputRoleConfig;
