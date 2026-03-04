//! Input roles — abstract device-agnostic actions.

use bevy::prelude::*;

/// An abstract input role, independent of device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InputRole(pub &'static str);

#[allow(dead_code)]
pub mod role {
    use super::InputRole;
    pub const NAVIGATE: InputRole = InputRole("navigate");
    pub const SELECT: InputRole = InputRole("select");
    pub const DRAW: InputRole = InputRole("draw");
    pub const ERASE: InputRole = InputRole("erase");
    pub const CONTEXT: InputRole = InputRole("context");
}

/// Resource mapping device matchers to roles.
#[derive(Resource, Default)]
pub struct InputRoleConfig;
