use std::f32::consts::FRAC_PI_2;
use std::f32::consts::PI;

use bevy::{
    asset::RenderAssetUsages,
    color::palettes::css::SILVER,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use xrcad_input::{InputPlugins, OrbitDelta, PanDelta};

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(InputPlugins::new().with_touch())
            .add_systems(Startup, setup)
            .add_systems(Update, (rotate, update_camera));
    }
}

/// Marker component used by the `rotate` system to identify shapes that should continuously rotate.
#[derive(Component)]
struct Shape;

/// Spherical-coordinate orbit state attached to the camera entity.
///
/// Camera position is computed as:
/// ```
/// x = target.x + distance * cos(elevation) * sin(azimuth)
/// y = target.y + distance * sin(elevation)
/// z = target.z + distance * cos(elevation) * cos(azimuth)
/// ```
#[derive(Component)]
struct OrbitCamera {
    /// Point the camera orbits around and looks at.
    target: Vec3,
    /// Rotation around the world Y-axis in radians.
    azimuth: f32,
    /// Angle above the horizontal plane in radians. Clamped away from 0 and ±90°.
    elevation: f32,
    /// Distance from target to camera in world units.
    distance: f32,
}

impl OrbitCamera {
    fn compute_transform(&self) -> Transform {
        let (sin_az, cos_az) = self.azimuth.sin_cos();
        let (sin_el, cos_el) = self.elevation.sin_cos();
        let pos = self.target
            + Vec3::new(
                self.distance * cos_el * sin_az,
                self.distance * sin_el,
                self.distance * cos_el * cos_az,
            );
        Transform::from_translation(pos).looking_at(self.target, Vec3::Y)
    }
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let debug_material = materials.add(StandardMaterial {
        base_color_texture: Some(images.add(uv_debug_texture())),
        ..default()
    });

    let shapes = [
        meshes.add(Cuboid::default()),
        meshes.add(Capsule3d::default()),
        meshes.add(Torus::default()),
        meshes.add(Cylinder::default()),
        meshes.add(Sphere::default().mesh().ico(5).unwrap()),
        meshes.add(Sphere::default().mesh().uv(32, 18)),
    ];
    let num_shapes = shapes.len();
    let x_extent = 14.0_f32;

    for (i, shape) in shapes.into_iter().enumerate() {
        commands.spawn((
            Mesh3d(shape),
            MeshMaterial3d(debug_material.clone()),
            Transform::from_xyz(
                -x_extent / 2. + i as f32 / (num_shapes - 1) as f32 * x_extent,
                2.0,
                0.0,
            )
            .with_rotation(Quat::from_rotation_x(-PI / 4.)),
            Shape,
        ));
    }

    commands.spawn((
        PointLight {
            shadows_enabled: true,
            intensity: 10_000_000.,
            range: 100.0,
            shadow_depth_bias: 0.2,
            ..default()
        },
        Transform::from_xyz(8.0, 16.0, 8.0),
    ));

    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(50., 50.))),
        MeshMaterial3d(materials.add(Color::from(SILVER))),
    ));

    // Orbit state that matches the original camera position:
    //   camera (0, 6, 12) → target (0, 1, 0)
    //   distance = 13, azimuth = 0, elevation = asin(5/13) ≈ 0.395 rad
    let orbit = OrbitCamera {
        target: Vec3::new(0.0, 1.0, 0.0),
        azimuth: 0.0,
        elevation: (5.0_f32 / 13.0_f32).asin(),
        distance: 13.0,
    };
    let transform = orbit.compute_transform();
    commands.spawn((Camera3d::default(), transform, orbit));
}

fn rotate(mut query: Query<&mut Transform, With<Shape>>, time: Res<Time>) {
    for mut transform in &mut query {
        transform.rotate_y(time.delta_secs() / 2.);
    }
}

/// Reads `OrbitDelta` and `PanDelta` events from the touch adapter and
/// updates the camera transform each frame.
fn update_camera(
    mut orbit_events: MessageReader<OrbitDelta>,
    mut pan_events: MessageReader<PanDelta>,
    mut cameras: Query<(&mut OrbitCamera, &mut Transform)>,
) {
    let Ok((mut cam, mut transform)) = cameras.get_single_mut() else {
        return;
    };

    for ev in orbit_events.read() {
        cam.azimuth += ev.azimuth;
        cam.elevation = (cam.elevation + ev.elevation).clamp(0.05, FRAC_PI_2 - 0.05);
    }

    for ev in pan_events.read() {
        // Camera-relative right and forward axes projected onto the ground plane.
        let (sin_az, cos_az) = cam.azimuth.sin_cos();
        let right = Vec3::new(cos_az, 0.0, -sin_az);
        let forward = Vec3::new(-sin_az, 0.0, -cos_az);
        cam.target += right * ev.dx + forward * ev.dz;
    }

    *transform = cam.compute_transform();
}

fn uv_debug_texture() -> Image {
    const TEXTURE_SIZE: usize = 8;

    let mut palette: [u8; 32] = [
        255, 102, 159, 255, 255, 159, 102, 255, 236, 255, 102, 255, 121, 255, 102, 255, 102, 255,
        198, 255, 102, 198, 255, 255, 121, 102, 255, 255, 236, 102, 255, 255,
    ];

    let mut texture_data = [0; TEXTURE_SIZE * TEXTURE_SIZE * 4];
    for y in 0..TEXTURE_SIZE {
        let offset = TEXTURE_SIZE * y * 4;
        texture_data[offset..(offset + TEXTURE_SIZE * 4)].copy_from_slice(&palette);
        palette.rotate_right(4);
    }

    Image::new_fill(
        Extent3d {
            width: TEXTURE_SIZE as u32,
            height: TEXTURE_SIZE as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &texture_data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}
