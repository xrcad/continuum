mod camera;
mod peer_markers;
mod scene_plugin;

use bevy::{asset::AssetMetaCheck, prelude::*};
use scene_plugin::ScenePlugin;
use xrcad_collab::XrcadCollabPlugin;
use xrcad_data::XrcadDataPlugin;
use xrcad_net::{PeerId, XrcadNetPlugin};

/// Return a stable peer ID for this launch.
///
/// TODO: persist to disk so reconnecting peers are recognised across launches.
fn load_or_generate_peer_id() -> PeerId {
    PeerId::generate()
}

/// Return a human-readable display name for this peer.
fn whoami_fallback() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "Peer".to_string())
}

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    // Wasm builds will check for meta files (that don't exist) if this isn't set.
                    // This causes errors and even panics in web builds on itch.
                    // See https://github.com/bevyengine/bevy_github_ci_template/issues/48.
                    meta_check: AssetMetaCheck::Never,
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        // Fill the entire browser window for Wasm.
                        fit_canvas_to_parent: true,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(XrcadNetPlugin {
            local_peer_id: load_or_generate_peer_id(),
            display_name: whoami_fallback(),
        })
        .add_plugins(XrcadCollabPlugin::default())
        .add_plugins(XrcadDataPlugin)
        .add_plugins(ScenePlugin)
        .run();
}
