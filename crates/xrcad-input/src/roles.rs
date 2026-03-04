//! Input roles — abstract device-agnostic actions.

use bevy::prelude::*;

/// An abstract input role, independent of device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InputRole(pub &'static str);

/// Resource mapping device matchers to roles.
#[derive(Resource, Default)]
pub struct InputRoleConfig;
