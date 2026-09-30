//! Static world: road, guard rails, terrain, scenery and lighting.

pub mod barriers;
pub mod forest;
pub mod lighting;
pub mod mesh_builder;
pub mod road;
pub mod street_lights;
pub mod traffic;

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::game::GameState;
use crate::vehicle::physics::VehicleTag;
use bevy::light::atmosphere::ScatteringMedium;
use lighting::LightingSettings;
use road::{LOOP_VISUAL_EXTENSION, RoadMaterials, RoadSpec, TrackPiece};

/// How far to either side the ground extends [m].
const TERRAIN_HALF_WIDTH: f32 = 600.0;
/// Spacing of the hills on the horizon [m].
const HILL_SPACING: f32 = 620.0;
/// Spacing of the roadside marker posts [m].
const SIGN_SPACING: f32 = 500.0;

pub struct WorldPlugin;

/// Static scenery that does not need to enter the shadow pass in the fast
/// preset. The marker stays present so higher presets can restore shadow
/// casting without rebuilding the world.
#[derive(Component)]
pub struct PerformanceShadowExcluded;

/// Ordering point used by traffic and streamed scenery so they observe the
/// player's already-wrapped position in the same rendered frame.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RoadLoopSystems;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(traffic::TrafficPlugin)
            .register_type::<RoadSpec>()
            .init_resource::<RoadSpec>()
            .add_systems(Startup, (lighting::load_skybox_assets, setup_world))
            .add_systems(
                Update,
                wrap_player_on_infinite_road
                    .in_set(RoadLoopSystems)
                    .run_if(in_state(GameState::Driving)),
            )
            .add_systems(
                Update,
                (
                    street_lights::sync_street_lights,
                    street_lights::configure_new_street_light_meshes,
                    lighting::prepare_skyboxes,
                    forest::stream_cinematic_forest.after(RoadLoopSystems),
                    forest::configure_new_forest_meshes,
                ),
            );
    }
}

impl Default for WorldPlugin {
    fn default() -> Self {
        Self
    }
}

fn setup_world(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    spec: Res<RoadSpec>,
    settings: Res<LightingSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut mediums: ResMut<Assets<ScatteringMedium>>,
) {
    let palette = road::create_materials(&asset_server, &mut materials);
    lighting::spawn_lighting(&mut commands, &settings, &mut mediums);
    spawn_ground(&mut commands, &spec);
    spawn_road_chunks(&mut commands, &spec, &mut meshes, &palette);
    spawn_loop_extensions(&mut commands, &spec, &mut meshes, &palette);
    spawn_barriers(&mut commands, &spec, &mut meshes, &palette);
    spawn_distance_markers(&mut commands, &spec, &mut meshes, &palette);
    street_lights::spawn_street_lights(
        &mut commands,
        &asset_server,
        &spec,
        &settings,
        &mut meshes,
        &mut materials,
    );
    spawn_scenery(&mut commands, &spec, &mut meshes, &palette);

    commands.insert_resource(palette);

    info!(
        "straight road built: {:.0} m, {} lanes, {} chunks, lighting {}",
        spec.length,
        spec.lanes_per_direction * 2,
        spec.chunk_count(),
        settings.quality.label()
    );
}

/// Teleport only the longitudinal coordinate at the start/finish seam. All
/// dynamic state stays untouched, so speed, yaw, wheel rotation and gearbox
/// continue exactly as they were before the transition.
fn wrap_player_on_infinite_road(
    spec: Res<RoadSpec>,
    mut vehicles: Query<(&mut Position, &mut Transform), With<VehicleTag>>,
) {
    for (mut position, mut transform) in &mut vehicles {
        let wrapped = spec.wrap_distance(position.z);
        if (wrapped - position.z).abs() > 0.01 {
            position.z = wrapped;
            // A Transform write in Update is treated as a teleport by the
            // interpolation backend, avoiding a 33 km backwards lerp frame.
            transform.translation.z = wrapped;
        }
    }
}

fn spawn_ground(commands: &mut Commands, spec: &RoadSpec) {
    // The visible road remains chunked for culling, but its driving surface is
    // one continuous collider. Internal faces between adjacent static boxes
    // can be caught by swept CCD at extreme speed and kick a car upwards.
    let (road_collider, road_transform) = road::road_collider(
        spec,
        -LOOP_VISUAL_EXTENSION,
        spec.length + LOOP_VISUAL_EXTENSION,
    );
    commands.spawn((
        Name::new("RoadCollider"),
        TrackPiece,
        RigidBody::Static,
        road_collider,
        Friction::new(1.0),
        Restitution::new(0.0),
        road_transform,
    ));

    let (terrain_collider, terrain_transform) = road::terrain_collider(spec);
    commands.spawn((
        Name::new("TerrainCollider"),
        TrackPiece,
        RigidBody::Static,
        terrain_collider,
        Friction::new(0.7),
        Restitution::new(0.0),
        terrain_transform,
    ));

    // The authored W-beam remains the first physical contact. A seamless
    // vertical half-space directly behind it guarantees that even a violent
    // traffic collision cannot throw the player's car outside the road.
    let (collider, transform) = barriers::containment_collider(spec, 1.0);
    commands.spawn((
        Name::new("ContainmentCollider_L"),
        TrackPiece,
        RigidBody::Static,
        collider,
        Friction::new(0.25),
        Restitution::new(0.0),
        transform,
    ));
    let (collider, transform) = barriers::containment_collider(spec, -1.0);
    commands.spawn((
        Name::new("ContainmentCollider_R"),
        TrackPiece,
        RigidBody::Static,
        collider,
        Friction::new(0.25),
        Restitution::new(0.0),
        transform,
    ));
}

fn spawn_road_chunks(
    commands: &mut Commands,
    spec: &RoadSpec,
    meshes: &mut Assets<Mesh>,
    palette: &RoadMaterials,
) {
    for chunk in 0..spec.chunk_count() {
        let z0 = chunk as f32 * spec.chunk_length;
        let z1 = (z0 + spec.chunk_length).min(spec.length);

        if let Some(terrain) = road::build_terrain_chunk(spec, z0, z1, TERRAIN_HALF_WIDTH) {
            commands.spawn((
                Name::new(format!("Terrain{chunk}")),
                TrackPiece,
                Mesh3d(meshes.add(terrain)),
                MeshMaterial3d(palette.grass.clone()),
                // Flat world surfaces cannot cast a useful shadow, but
                // submitting them to every cascade still costs a draw call.
                PerformanceShadowExcluded,
                Transform::IDENTITY,
            ));
        }

        if let Some(surface) = road::build_surface_chunk(spec, z0, z1) {
            commands.spawn((
                Name::new(format!("RoadSurface{chunk}")),
                TrackPiece,
                Mesh3d(meshes.add(surface)),
                MeshMaterial3d(palette.asphalt.clone()),
                PerformanceShadowExcluded,
                Transform::IDENTITY,
            ));
        }

        if let Some(markings) = road::build_markings_chunk(spec, z0, z1) {
            commands.spawn((
                Name::new(format!("RoadMarkings{chunk}")),
                TrackPiece,
                Mesh3d(meshes.add(markings)),
                MeshMaterial3d(palette.marking.clone()),
                PerformanceShadowExcluded,
                Transform::IDENTITY,
            ));
        }
    }
}

/// Duplicate inexpensive visual geometry outside the nominal loop. The main
/// road keeps its diagnostic entity counts, while a 4 km camera or mirror can
/// never see the start/finish edge during a transition.
fn spawn_loop_extensions(
    commands: &mut Commands,
    spec: &RoadSpec,
    meshes: &mut Assets<Mesh>,
    palette: &RoadMaterials,
) {
    let extension_chunks = (LOOP_VISUAL_EXTENSION / spec.chunk_length).ceil() as i32;
    for side in [-1i32, 1] {
        for index in 0..extension_chunks {
            let (z0, z1) = if side < 0 {
                let end = -(index as f32) * spec.chunk_length;
                ((end - spec.chunk_length).max(-LOOP_VISUAL_EXTENSION), end)
            } else {
                let start = spec.length + index as f32 * spec.chunk_length;
                (
                    start,
                    (start + spec.chunk_length).min(spec.length + LOOP_VISUAL_EXTENSION),
                )
            };
            if z1 <= z0 {
                continue;
            }

            let suffix = if side < 0 { "Before" } else { "After" };
            if let Some(terrain) = road::build_terrain_chunk(spec, z0, z1, TERRAIN_HALF_WIDTH) {
                commands.spawn((
                    Name::new(format!("LoopExtensionTerrain{suffix}{index}")),
                    TrackPiece,
                    Mesh3d(meshes.add(terrain)),
                    MeshMaterial3d(palette.grass.clone()),
                    PerformanceShadowExcluded,
                    Transform::IDENTITY,
                ));
            }
            if let Some(surface) = road::build_surface_chunk(spec, z0, z1) {
                commands.spawn((
                    Name::new(format!("LoopExtensionSurface{suffix}{index}")),
                    TrackPiece,
                    Mesh3d(meshes.add(surface)),
                    MeshMaterial3d(palette.asphalt.clone()),
                    PerformanceShadowExcluded,
                    Transform::IDENTITY,
                ));
            }
            if let Some(markings) = road::build_markings_chunk(spec, z0, z1) {
                commands.spawn((
                    Name::new(format!("LoopExtensionMarkings{suffix}{index}")),
                    TrackPiece,
                    Mesh3d(meshes.add(markings)),
                    MeshMaterial3d(palette.marking.clone()),
                    PerformanceShadowExcluded,
                    Transform::IDENTITY,
                ));
            }
            for rail_side in [-1.0f32, 1.0] {
                if let Some(mesh) = barriers::build_barrier_chunk(spec, rail_side, z0, z1) {
                    commands.spawn((
                        Name::new(format!(
                            "LoopExtensionRail{suffix}{index}_{}",
                            if rail_side < 0.0 { "R" } else { "L" }
                        )),
                        TrackPiece,
                        Mesh3d(meshes.add(mesh)),
                        MeshMaterial3d(palette.metal.clone()),
                        PerformanceShadowExcluded,
                        Transform::IDENTITY,
                    ));
                }
                let (collider, transform) = barriers::barrier_collider(spec, rail_side, z0, z1);
                commands.spawn((
                    Name::new(format!(
                        "LoopExtensionRailCollider{suffix}{index}_{}",
                        if rail_side < 0.0 { "R" } else { "L" }
                    )),
                    TrackPiece,
                    barriers::RoadRail { side: rail_side },
                    RigidBody::Static,
                    collider,
                    barriers::rail_friction(),
                    barriers::rail_restitution(),
                    transform,
                ));
            }
        }
    }
}

fn spawn_barriers(
    commands: &mut Commands,
    spec: &RoadSpec,
    meshes: &mut Assets<Mesh>,
    palette: &RoadMaterials,
) {
    for chunk in 0..spec.chunk_count() {
        let z0 = chunk as f32 * spec.chunk_length;
        let z1 = (z0 + spec.chunk_length).min(spec.length);

        for side in [-1.0f32, 1.0] {
            if let Some(mesh) = barriers::build_barrier_chunk(spec, side, z0, z1) {
                commands.spawn((
                    Name::new(format!(
                        "Barrier{chunk}_{}",
                        if side < 0.0 { "R" } else { "L" }
                    )),
                    TrackPiece,
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(palette.metal.clone()),
                    PerformanceShadowExcluded,
                    Transform::IDENTITY,
                ));
            }

            let (collider, transform) = barriers::barrier_collider(spec, side, z0, z1);
            commands.spawn((
                Name::new(format!(
                    "BarrierCollider{chunk}_{}",
                    if side < 0.0 { "R" } else { "L" }
                )),
                TrackPiece,
                barriers::RoadRail { side },
                RigidBody::Static,
                collider,
                barriers::rail_friction(),
                barriers::rail_restitution(),
                transform,
            ));
        }
    }
}

fn spawn_distance_markers(
    commands: &mut Commands,
    spec: &RoadSpec,
    meshes: &mut Assets<Mesh>,
    palette: &RoadMaterials,
) {
    let count = (spec.length / spec.marker_spacing).floor() as u32;
    let lateral = -(spec.barrier_offset + 1.6);
    for kilometre in 1..=count {
        let z = kilometre as f32 * spec.marker_spacing;
        let Some((structure, digits)) = road::build_distance_marker(kilometre) else {
            warn!("could not build the board for kilometre {kilometre}");
            continue;
        };
        // The digit faces back towards approaching traffic.
        let facing = Transform::from_translation(spec.point_at(z, lateral, 0.0))
            .with_rotation(spec.frame_rotation(z));
        commands
            .spawn((
                Name::new(format!("DistanceMarker{kilometre}")),
                TrackPiece,
                Mesh3d(meshes.add(structure)),
                MeshMaterial3d(palette.sign_face.clone()),
                PerformanceShadowExcluded,
                facing,
            ))
            .with_child((
                Mesh3d(meshes.add(digits)),
                MeshMaterial3d(palette.sign_text.clone()),
                Transform::from_xyz(0.0, 0.0, -0.045),
            ));
    }
}

fn spawn_scenery(
    commands: &mut Commands,
    spec: &RoadSpec,
    meshes: &mut Assets<Mesh>,
    palette: &RoadMaterials,
) {
    // Roadside marker posts: cheap, and they give a strong sense of speed.
    let post = meshes.add(Cuboid::new(0.09, 1.0, 0.09));
    let count = (spec.length / SIGN_SPACING).floor() as u32;
    for index in 0..=count {
        let distance = index as f32 * SIGN_SPACING;
        for side in [-1.0f32, 1.0] {
            commands.spawn((
                TrackPiece,
                Mesh3d(post.clone()),
                MeshMaterial3d(palette.sign_text.clone()),
                PerformanceShadowExcluded,
                Transform::from_translation(spec.point_at(
                    distance,
                    side * (spec.barrier_offset + 0.9),
                    0.5,
                ))
                .with_rotation(spec.frame_rotation(distance)),
            ));
            for copy in loop_scenery_copies(distance, spec).into_iter().flatten() {
                commands.spawn((
                    Name::new("LoopExtensionRoadsidePost"),
                    TrackPiece,
                    Mesh3d(post.clone()),
                    MeshMaterial3d(palette.sign_text.clone()),
                    PerformanceShadowExcluded,
                    Transform::from_translation(spec.point_at(
                        copy,
                        side * (spec.barrier_offset + 0.9),
                        0.5,
                    )),
                ));
            }
        }
    }

    // Low polygon hills on the horizon. One shared mesh, many transforms.
    let hill = meshes.add(Sphere::new(1.0).mesh().uv(14, 8));
    let hill_count = (spec.length / HILL_SPACING).ceil() as i32 + 2;
    for index in -1..hill_count {
        let distance = index as f32 * HILL_SPACING;
        for (side, scale, offset) in [
            (-1.0f32, Vec3::new(260.0, 62.0, 200.0), 430.0),
            (1.0, Vec3::new(300.0, 48.0, 240.0), 480.0),
        ] {
            commands.spawn((
                TrackPiece,
                Mesh3d(hill.clone()),
                MeshMaterial3d(palette.hill.clone()),
                PerformanceShadowExcluded,
                Transform::from_translation(spec.point_at(
                    distance + side * 140.0,
                    side * offset,
                    -18.0,
                ))
                .with_scale(scale),
            ));
            for copy in loop_scenery_copies(distance + side * 140.0, spec)
                .into_iter()
                .flatten()
            {
                commands.spawn((
                    Name::new("LoopExtensionHill"),
                    TrackPiece,
                    Mesh3d(hill.clone()),
                    MeshMaterial3d(palette.hill.clone()),
                    PerformanceShadowExcluded,
                    Transform::from_translation(spec.point_at(copy, side * offset, -18.0))
                        .with_scale(scale),
                ));
            }
        }
    }
}

/// Place identical hills and roadside posts on both visual continuations.
/// Their world-space geometry repeats at precisely the same longitudinal
/// offset as the road, rather than disappearing when the player wraps.
fn loop_scenery_copies(distance: f32, road: &RoadSpec) -> [Option<f32>; 2] {
    [
        (distance <= LOOP_VISUAL_EXTENSION).then_some(distance + road.length),
        (distance >= road.length - LOOP_VISUAL_EXTENSION).then_some(distance - road.length),
    ]
}
