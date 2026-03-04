//! xrcad-input — input abstraction, role routing, and device adapters.

use bevy::app::{PluginGroup, PluginGroupBuilder};
use bevy::prelude::*;

mod keyboard;
mod mouse;
mod roles;
mod router;
mod touch;
mod voice;

pub use roles::{InputRole, InputRoleConfig};
pub use router::RoleMessage;
pub use touch::{OrbitDelta, PanDelta};

/// Core input plugin — role routing and event infrastructure.
/// Always required; added automatically by [`InputPlugins`].
pub struct InputCorePlugin;

impl Plugin for InputCorePlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<RoleMessage>()
            .init_resource::<InputRoleConfig>()
            .add_systems(PreUpdate, router::route_input);
    }
}

/// Builder-pattern plugin group for all input device adapters.
///
/// ```rust,no_run
/// app.add_plugins(
///     InputPlugins::new()
///         .with_mouse()
///         .with_keyboard()
///         .with_touch()
///         .with_voice()
/// );
/// ```
#[derive(Default)]
pub struct InputPlugins {
    mouse: bool,
    keyboard: bool,
    touch: bool,
    voice: bool,
}

impl InputPlugins {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_mouse(mut self) -> Self {
        self.mouse = true;
        self
    }

    pub fn with_keyboard(mut self) -> Self {
        self.keyboard = true;
        self
    }

    pub fn with_touch(mut self) -> Self {
        self.touch = true;
        self
    }

    pub fn with_voice(mut self) -> Self {
        self.voice = true;
        self
    }

    /// Convenience: mouse + keyboard (typical desktop).
    pub fn desktop() -> Self {
        Self::new().with_mouse().with_keyboard()
    }
}

impl PluginGroup for InputPlugins {
    fn build(self) -> PluginGroupBuilder {
        let mut group = PluginGroupBuilder::start::<Self>();
        group = group.add(InputCorePlugin);
        if self.mouse {
            group = group.add(mouse::MouseInputPlugin);
        }
        if self.keyboard {
            group = group.add(keyboard::KeyboardInputPlugin);
        }
        if self.touch {
            group = group.add(touch::TouchInputPlugin);
        }
        if self.voice {
            group = group.add(voice::VoiceInputPlugin);
        }
        group
    }
}
