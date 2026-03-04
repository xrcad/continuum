//! Input roles — abstract device-agnostic actions.

use bevy::prelude::*;

/// An abstract input role, independent of device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InputRole(pub &'static str);

pub mod role {
    use super::InputRole;
    /// Single-finger orbit / camera navigation gesture.
    pub const NAVIGATE: InputRole = InputRole("navigate");
}

/// Resource mapping device matchers to roles.
#[derive(Resource, Default)]
pub struct InputRoleConfig;
