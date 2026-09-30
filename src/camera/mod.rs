//! Camera rig: six modes, smoothing and speed-dependent field of view.

pub mod controller;
pub mod mirrors;
pub mod modes;
pub mod speed_effect;

use bevy::light::Skybox;
use bevy::prelude::*;

use crate::vehicle::physics::VehicleTag;
use crate::world::lighting::{self, LightingSettings, LitCamera};
use controller::{CameraSettings, CockpitCameraSettings, GameCamera};

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(speed_effect::SpeedEffectPlugin)
            .add_plugins(mirrors::MirrorPlugin)
            .init_resource::<CameraSettings>()
            .init_resource::<CockpitCameraSettings>()
            .register_type::<CockpitCameraSettings>()
            .add_systems(PostStartup, spawn_camera)
            .add_systems(
                PostUpdate,
                controller::follow_vehicle
                    // The system reads the car's interpolated local Transform,
                    // then propagation publishes both car and camera poses to
                    // rendering in the same frame.
                    .before(TransformSystems::Propagate),
            );
    }
}

fn spawn_camera(
    mut commands: Commands,
    lighting_settings: Res<LightingSettings>,
    vehicles: Query<Entity, With<VehicleTag>>,
) {
    let Ok(vehicle) = vehicles.single() else {
        warn!("no vehicle to follow, camera not spawned");
        return;
    };

    let camera = commands
        .spawn((
            Name::new("GameCamera"),
            Camera3d::default(),
            Projection::Perspective(PerspectiveProjection {
                fov: 65.0_f32.to_radians(),
                near: 0.08,
                far: 4_000.0,
                ..default()
            }),
            Transform::from_xyz(0.0, 3.0, 12.0).looking_at(Vec3::ZERO, Vec3::Y),
            // There is only one camera, but saying so explicitly means the HUD
            // is never left looking for a target.
            IsDefaultUiCamera,
            // Material surfaces use multi-layer elevation shadows. A modest
            // sample count keeps their soft edges clean without affecting the
            // 3D render path.
            BoxShadowSamples(8),
            LitCamera,
            Skybox {
                image: None,
                brightness: lighting_settings.time_of_day.skybox_brightness(),
                ..default()
            },
            GameCamera::new(vehicle),
            speed_effect::SpeedTunnelEffect::default(),
        ))
        .id();

    lighting::apply_to_camera(&mut commands.entity(camera), &lighting_settings);
}
