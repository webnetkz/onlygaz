//! Road lamps placed every 500 metres behind the guard rail.

use bevy::gltf::GltfAssetLabel;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::world_serialization::WorldAssetRoot;

use super::PerformanceShadowExcluded;
use super::lighting::{LightingSettings, TimeOfDay};
use super::road::{RoadSpec, TrackPiece};

pub const STREET_LIGHT_SPACING: f32 = 500.0;
const POLE_HEIGHT: f32 = 9.0;
const ARM_LENGTH: f32 = 2.8;
const LAMP_RANGE: f32 = 95.0;
const LAMP_LUMENS: f32 = 220_000.0;
const STREET_LIGHT_ASSET: &str = "environment/roadside/bridge_street_light.glb";
const STREET_LIGHT_SOURCE_HEIGHT: f32 = 414.9825;
const STREET_LIGHT_MODEL_SCALE: f32 = POLE_HEIGHT / STREET_LIGHT_SOURCE_HEIGHT;

#[derive(Component)]
pub struct RoadLamp {
    peak: f32,
}

#[derive(Component)]
pub struct StreetLightModelRoot;

pub fn lamp_positions(spec: &RoadSpec) -> impl Iterator<Item = (f32, f32)> + '_ {
    let count = (spec.length / STREET_LIGHT_SPACING).floor() as usize;
    (0..count).map(move |index| {
        let distance = STREET_LIGHT_SPACING * (index as f32 + 0.5);
        let side = if index % 2 == 0 { -1.0 } else { 1.0 };
        (distance, side)
    })
}

pub fn spawn_street_lights(
    commands: &mut Commands,
    asset_server: &AssetServer,
    spec: &RoadSpec,
    settings: &LightingSettings,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let housing_mesh = meshes.add(Cuboid::new(0.52, 0.16, 0.34));
    let lamp_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.82, 0.86, 0.92),
        emissive: LinearRgba::rgb(3_500.0, 3_150.0, 2_650.0),
        emissive_exposure_weight: 1.0,
        perceptual_roughness: 0.25,
        ..default()
    });
    let model = asset_server.load(GltfAssetLabel::Scene(0).from_asset(STREET_LIGHT_ASSET));
    let active = settings.time_of_day == TimeOfDay::Night;

    for (index, (distance, side)) in lamp_positions(spec).enumerate() {
        let x = side * (spec.barrier_offset + 1.45);
        commands
            .spawn((
                Name::new(format!("RoadLamp{:03}", index + 1)),
                TrackPiece,
                Transform::from_xyz(x, spec.surface_y, distance),
                Visibility::default(),
            ))
            .with_children(|lamp| {
                lamp.spawn((
                    Name::new("BridgeStreetLightModel"),
                    StreetLightModelRoot,
                    WorldAssetRoot(model.clone()),
                    PerformanceShadowExcluded,
                    Transform::from_rotation(if side < 0.0 {
                        Quat::from_rotation_y(core::f32::consts::PI)
                    } else {
                        Quat::IDENTITY
                    })
                    .with_scale(Vec3::splat(STREET_LIGHT_MODEL_SCALE)),
                    Visibility::default(),
                ));
                let light_x = -side * (ARM_LENGTH * 0.45);
                lamp.spawn((
                    Mesh3d(housing_mesh.clone()),
                    MeshMaterial3d(lamp_material.clone()),
                    PerformanceShadowExcluded,
                    Transform::from_xyz(light_x, POLE_HEIGHT - 0.32, 0.0),
                ));
                let target = Vec3::new(-side * (spec.barrier_offset + 1.45), -POLE_HEIGHT, 0.0);
                lamp.spawn((
                    RoadLamp { peak: LAMP_LUMENS },
                    SpotLight {
                        color: Color::srgb(1.0, 0.90, 0.72),
                        intensity: if active { LAMP_LUMENS } else { 0.0 },
                        range: LAMP_RANGE,
                        radius: 0.18,
                        shadow_maps_enabled: false,
                        inner_angle: 34.0_f32.to_radians(),
                        outer_angle: 58.0_f32.to_radians(),
                        ..default()
                    },
                    Transform::from_xyz(light_x, POLE_HEIGHT - 0.42, 0.0)
                        .looking_to(target, Vec3::Z),
                ));
            });
    }
}

/// Imported glTF children appear asynchronously. Mark them once so 66 detailed
/// poles never multiply the cost of every cascaded sun-shadow pass.
pub fn configure_new_street_light_meshes(
    mut commands: Commands,
    new_meshes: Query<Entity, Added<Mesh3d>>,
    ancestry: Query<(Option<&ChildOf>, Option<&StreetLightModelRoot>)>,
) {
    for mesh in &new_meshes {
        let mut current = mesh;
        while let Ok((parent, root)) = ancestry.get(current) {
            if root.is_some() {
                commands
                    .entity(mesh)
                    .insert((PerformanceShadowExcluded, NotShadowCaster));
                break;
            }
            let Some(parent) = parent else {
                break;
            };
            current = parent.parent();
        }
    }
}

pub fn sync_street_lights(
    settings: Res<LightingSettings>,
    mut lamps: Query<(&RoadLamp, &mut SpotLight)>,
) {
    if !settings.is_changed() {
        return;
    }
    let strength = if settings.time_of_day == TimeOfDay::Night {
        1.0
    } else {
        0.0
    };
    for (lamp, mut light) in &mut lamps {
        light.intensity = lamp.peak * strength;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lamps_are_500_metres_apart_and_behind_alternating_guard_rails() {
        let spec = RoadSpec::default();
        let positions: Vec<_> = lamp_positions(&spec).collect();
        assert_eq!(positions.len(), 66);
        for pair in positions.windows(2) {
            assert!((pair[1].0 - pair[0].0 - STREET_LIGHT_SPACING).abs() < 0.001);
            assert_ne!(pair[0].1, pair[1].1);
        }
        for (_, side) in positions {
            let x = side * (spec.barrier_offset + 1.45);
            assert!(x.abs() > spec.barrier_offset);
        }
    }
}
