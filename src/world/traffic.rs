//! Lightweight motorway traffic.
//!
//! NPC vehicles use finite-mass rigid bodies, but do not run eleven more
//! tyre, suspension and drivetrain solvers. They cruise in both directions,
//! keep a safe gap, and are recycled outside the visible streaming radius.

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::world_serialization::WorldAssetRoot;
use serde::{Deserialize, Serialize};

use crate::game::GameState;
use crate::vehicle::physics::VehicleTag;

use super::RoadLoopSystems;
use super::lighting::LightingSettings;
use super::road::RoadSpec;

pub const MAX_TRAFFIC_SPEED_KMH: f32 = 160.0;
const MAX_TRAFFIC_SPEED_MPS: f32 = MAX_TRAFFIC_SPEED_KMH / 3.6;
/// Maximum comfortable emergency deceleration used by the traffic controller.
const TRAFFIC_BRAKE_DECEL_MPS2: f32 = 8.0;
const TRAFFIC_ACCEL_MPS2: f32 = 2.4;
/// Clear space left between the NPC's front bumper and a stopped obstacle.
const TRAFFIC_STOP_GAP: f32 = 8.0;
const TRAFFIC_REACTION_TIME: f32 = 0.7;
const TRAFFIC_SENSOR_WIDTH: f32 = 2.7;
const TRAFFIC_SENSOR_HEIGHT: f32 = 1.0;
const TRAFFIC_SENSOR_DEPTH: f32 = 0.2;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum TrafficDensity {
    Off,
    Low,
    #[default]
    Medium,
    High,
}

impl TrafficDensity {
    pub const ALL: [Self; 4] = [Self::Off, Self::Low, Self::Medium, Self::High];

    pub fn next(self) -> Self {
        Self::ALL[(self as usize + 1) % Self::ALL.len()]
    }

    pub fn previous(self) -> Self {
        Self::ALL[(self as usize + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
        }
    }

    pub const fn vehicle_count(self) -> usize {
        match self {
            Self::Off => 0,
            Self::Low => 6,
            Self::Medium => 12,
            Self::High => 22,
        }
    }
}

#[derive(Debug, Clone, Resource, Reflect, Serialize, Deserialize)]
#[reflect(Resource)]
#[serde(default)]
pub struct TrafficSettings {
    pub density: TrafficDensity,
}

impl Default for TrafficSettings {
    fn default() -> Self {
        Self {
            density: TrafficDensity::Medium,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrafficVisual {
    Bus,
    MercedesMClass2011,
    KiaPicantoGtLine2017,
}

impl TrafficVisual {
    const fn display_name(self) -> &'static str {
        match self {
            Self::Bus => "Town Bus",
            Self::MercedesMClass2011 => "2011 Mercedes M-Class",
            Self::KiaPicantoGtLine2017 => "2017 Kia Picanto GT-Line",
        }
    }
}

#[derive(Debug, Clone, Copy, Component)]
pub struct TrafficVehicle {
    pub slot: usize,
    pub lane: usize,
    pub oncoming: bool,
    pub cruise_speed_mps: f32,
    current_speed_mps: f32,
    crashed: bool,
}

#[derive(Component)]
struct TrafficVisualRoot;

/// Low beam of a traffic car. Lit only when
/// [`LightingSettings::traffic_beams_enabled`]: by day a beam adds well under
/// one percent to the sunlit road but still costs every pixel it covers.
#[derive(Component)]
pub struct TrafficBeam;

const TRAFFIC_BEAM_LUMENS: f32 = 45_000.0;

fn sync_traffic_beams(
    settings: Res<LightingSettings>,
    mut beams: Query<(Ref<TrafficBeam>, &mut SpotLight)>,
) {
    let intensity = if settings.traffic_beams_enabled() {
        TRAFFIC_BEAM_LUMENS
    } else {
        0.0
    };
    for (beam, mut light) in &mut beams {
        if (settings.is_changed() || beam.is_added()) && light.intensity != intensity {
            light.intensity = intensity;
        }
    }
}

/// Shared emissive lenses and shadow-free beams, independent of time of day.
#[derive(Resource)]
struct TrafficLampAssets {
    lens: Handle<Mesh>,
    front: Handle<StandardMaterial>,
    rear: Handle<StandardMaterial>,
}

fn setup_traffic_lamps(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(TrafficLampAssets {
        lens: meshes.add(Cuboid::new(0.22, 0.075, 0.035)),
        front: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.94, 0.82),
            emissive: LinearRgba::rgb(35_000.0, 30_000.0, 22_000.0),
            ..default()
        }),
        rear: materials.add(StandardMaterial {
            base_color: Color::srgb(0.8, 0.015, 0.005),
            emissive: LinearRgba::rgb(8_000.0, 100.0, 25.0),
            ..default()
        }),
    });
}

fn spawn_traffic_lamps(
    commands: &mut Commands,
    parent: Entity,
    size: Vec3,
    assets: &TrafficLampAssets,
) {
    for side in [-1.0, 1.0] {
        let x = side * size.x * 0.37;
        let y = -size.y * 0.5 + 0.65;
        for (z, material) in [
            (-size.z * 0.5 - 0.025, &assets.front),
            (size.z * 0.5 + 0.025, &assets.rear),
        ] {
            commands.spawn((
                Name::new("Traffic running lamp"),
                Mesh3d(assets.lens.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(x, y, z),
                ChildOf(parent),
            ));
        }
        commands.spawn((
            Name::new("Traffic low beam"),
            TrafficBeam,
            SpotLight {
                color: Color::srgb(1.0, 0.94, 0.82),
                // Set by `sync_traffic_beams`; dark lights are not rendered.
                intensity: 0.0,
                range: 85.0,
                inner_angle: 0.18,
                outer_angle: 0.38,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_xyz(x, y, -size.z * 0.5 - 0.08)
                .with_rotation(Quat::from_rotation_x(-0.04)),
            ChildOf(parent),
        ));
    }
}

/// A low, lane-width shape swept ahead of every NPC. Keeping one shared shape
/// avoids rebuilding a collider for every vehicle on every frame.
#[derive(Resource)]
struct TrafficObstacleSensor(Collider);

impl Default for TrafficObstacleSensor {
    fn default() -> Self {
        Self(Collider::cuboid(
            TRAFFIC_SENSOR_WIDTH,
            TRAFFIC_SENSOR_HEIGHT,
            TRAFFIC_SENSOR_DEPTH,
        ))
    }
}

pub struct TrafficPlugin;

impl Plugin for TrafficPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TrafficSettings>()
            .init_resource::<TrafficObstacleSensor>()
            .add_systems(Startup, setup_traffic_lamps)
            .register_type::<TrafficSettings>()
            .register_type::<TrafficDensity>()
            .add_systems(Update, maintain_traffic.after(RoadLoopSystems))
            .add_systems(Update, sync_traffic_beams.after(maintain_traffic))
            .add_systems(
                Update,
                drive_traffic
                    .after(maintain_traffic)
                    .run_if(in_state(GameState::Driving)),
            );
    }
}

fn visual_for_slot(slot: usize) -> TrafficVisual {
    // Keep every NPC type in the six-car Low preset. The player's Bugatti is
    // intentionally excluded from traffic.
    match slot % 3 {
        0 => TrafficVisual::Bus,
        1 => TrafficVisual::MercedesMClass2011,
        _ => TrafficVisual::KiaPicantoGtLine2017,
    }
}

fn dimensions(visual: TrafficVisual) -> Vec3 {
    match visual {
        // Measured from the supplied glTF after its 0.092 scale conversion.
        TrafficVisual::Bus => Vec3::new(2.95, 3.0, 12.05),
        // Full scene bounds after the source centimetres-to-metres conversion.
        TrafficVisual::MercedesMClass2011 => Vec3::new(2.143_282, 1.837_95, 4.800_46),
        // The Kia export is authored at 1/100 real-world size.
        TrafficVisual::KiaPicantoGtLine2017 => Vec3::new(1.842_404, 1.620_134, 3.606_163),
    }
}

fn slot_layout(slot: usize, lanes: usize) -> (usize, bool, f32) {
    let lanes = lanes.max(1);
    let lane = slot % lanes;
    let row_index = slot / lanes;
    // Alternate complete rows so both carriageways use every available lane.
    let oncoming = row_index.is_multiple_of(2);
    let row = row_index as f32;
    // Nearby traffic is still far enough away not to interfere with a launch.
    let offset = if oncoming {
        300.0 + row * 155.0
    } else {
        220.0 + row * 125.0
    };
    (lane, oncoming, offset)
}

fn spawn_distance(player_distance: f32, offset: f32) -> f32 {
    // Visual and physical road extensions allow traffic to occupy the nearest
    // periodic image beyond either end of the nominal 33 km coordinate range.
    player_distance + offset
}

fn cruise_speed(slot: usize, visual: TrafficVisual) -> f32 {
    let kmh = match visual {
        TrafficVisual::Bus => 86.0 + (slot % 3) as f32 * 7.0,
        TrafficVisual::MercedesMClass2011 | TrafficVisual::KiaPicantoGtLine2017 => {
            104.0 + (slot * 13 % 55) as f32
        }
    };
    kmh.min(MAX_TRAFFIC_SPEED_KMH) / 3.6
}

fn spawn_traffic_vehicle(
    commands: &mut Commands,
    asset_server: &AssetServer,
    road: &RoadSpec,
    player_distance: f32,
    slot: usize,
    lamps: &TrafficLampAssets,
) {
    let visual = visual_for_slot(slot);
    let size = dimensions(visual);
    let (lane, oncoming, offset) = slot_layout(slot, road.lanes_per_direction);
    let distance = spawn_distance(player_distance, offset);
    let lateral = road.lane_center_x(lane, oncoming);
    let position = road.point_at(distance, lateral, road.surface_y + size.y * 0.5);
    let rotation = if oncoming {
        Quat::IDENTITY
    } else {
        road.vehicle_rotation(distance)
    };
    let speed = cruise_speed(slot, visual).min(MAX_TRAFFIC_SPEED_MPS);
    let direction = if oncoming { -Vec3::Z } else { Vec3::Z };

    let entity = commands
        .spawn((
            Name::new(format!("Traffic {slot}")),
            TrafficVehicle {
                slot,
                lane,
                oncoming,
                cruise_speed_mps: speed,
                current_speed_mps: speed,
                crashed: false,
            },
            RigidBody::Dynamic,
            Collider::cuboid(size.x, size.y, size.z),
            Friction::new(0.65),
            Restitution::new(0.02),
            SweptCcd::default(),
            GravityScale(0.0),
            LinearVelocity(direction * speed),
            AngularVelocity::ZERO,
            Position(position),
            Rotation(rotation),
            Transform::from_translation(position).with_rotation(rotation),
            Visibility::default(),
        ))
        .id();
    let mass = match visual {
        TrafficVisual::Bus => 12_000.0,
        TrafficVisual::MercedesMClass2011 => 2_150.0,
        TrafficVisual::KiaPicantoGtLine2017 => 1_050.0,
    };
    commands.entity(entity).insert((
        super::blob_shadows::BlobShadowCaster {
            half_extents: Vec2::new(size.x * 0.5, size.z * 0.5),
        },
        Mass(mass),
        NoAutoMass,
        AngularInertia::new(
            Vec3::new(
                size.y * size.y + size.z * size.z,
                size.x * size.x + size.z * size.z,
                size.x * size.x + size.y * size.y,
            ) * (mass / 12.0),
        ),
        NoAutoAngularInertia,
        AngularDamping(0.2),
    ));
    spawn_traffic_lamps(commands, entity, size, lamps);

    let (path, model_transform) = visual_model(visual, size.y);
    commands.spawn((
        Name::new(format!("TrafficVisual: {}", visual.display_name())),
        TrafficVisualRoot,
        WorldAssetRoot(asset_server.load(GltfAssetLabel::Scene(0).from_asset(path))),
        model_transform,
        Visibility::default(),
        ChildOf(entity),
    ));
}

fn visual_model(visual: TrafficVisual, collider_height: f32) -> (&'static str, Transform) {
    match visual {
        TrafficVisual::Bus => {
            const SCALE: f32 = 0.092;
            // The glTF root matrix already converts the source's Z-up axes to
            // Bevy's Y-up space. Only turn its remaining X length axis towards
            // the simulator's canonical -Z vehicle-forward direction.
            let rotation = Quat::from_rotation_y(core::f32::consts::FRAC_PI_2);
            let source_centre_x = 1.053;
            let source_min_z = -1.456;
            let translation = Vec3::new(
                0.0,
                -collider_height * 0.5 - source_min_z * SCALE,
                source_centre_x * SCALE,
            );
            (
                "traffic/town_bus/scene.gltf",
                Transform::from_translation(translation)
                    .with_rotation(rotation)
                    .with_scale(Vec3::splat(SCALE)),
            )
        }
        TrafficVisual::MercedesMClass2011 => {
            const SCALE: f32 = 0.01;
            const SOURCE_MIN: Vec3 = Vec3::new(-107.152_76, 0.0, -240.023);
            const SOURCE_MAX: Vec3 = Vec3::new(107.175_44, 183.795, 240.023);
            (
                "traffic/mercedes_m_class_2011/scene.gltf",
                imported_car_transform(SCALE, SOURCE_MIN, SOURCE_MAX, collider_height),
            )
        }
        TrafficVisual::KiaPicantoGtLine2017 => {
            const SCALE: f32 = 100.0;
            const SOURCE_MIN: Vec3 = Vec3::new(-0.009_212_017, -0.000_046_979, -0.016_500_484);
            const SOURCE_MAX: Vec3 = Vec3::new(0.009_212_019, 0.016_154_362, 0.019_561_144);
            (
                "traffic/kia_picanto_gt_line_2017/scene.gltf",
                imported_car_transform(SCALE, SOURCE_MIN, SOURCE_MAX, collider_height),
            )
        }
    }
}

fn imported_car_transform(
    scale: f32,
    source_min: Vec3,
    source_max: Vec3,
    collider_height: f32,
) -> Transform {
    // Both supplied cars face +Z. Traffic chassis use -Z as their local front,
    // so this yaw aligns the visible nose with the kinematic body's direction.
    let rotation = Quat::from_rotation_y(core::f32::consts::PI);
    let source_origin = Vec3::new(
        (source_min.x + source_max.x) * 0.5,
        source_min.y,
        (source_min.z + source_max.z) * 0.5,
    );
    let mut transform = Transform::from_translation(-(rotation * (source_origin * scale)))
        .with_rotation(rotation)
        .with_scale(Vec3::splat(scale));
    transform.translation.y -= collider_height * 0.5;
    transform
}

fn maintain_traffic(
    mut commands: Commands,
    state: Res<State<GameState>>,
    settings: Res<TrafficSettings>,
    road: Res<RoadSpec>,
    asset_server: Res<AssetServer>,
    lamps: Res<TrafficLampAssets>,
    player: Query<&Transform, (With<VehicleTag>, Without<TrafficVehicle>)>,
    mut traffic: Query<(
        Entity,
        &mut TrafficVehicle,
        &mut Position,
        &mut Transform,
        &mut Rotation,
        &mut LinearVelocity,
        &mut AngularVelocity,
        &mut GravityScale,
    )>,
) {
    let active = matches!(*state.get(), GameState::Driving | GameState::Paused);
    let desired = if active {
        settings.density.vehicle_count()
    } else {
        0
    };
    let Ok(player_transform) = player.single() else {
        return;
    };
    let player_distance = road.distance_along(player_transform.translation);

    let mut existing = Vec::new();
    for (entity, vehicle, ..) in &traffic {
        existing.push((vehicle.slot, entity));
    }
    existing.sort_by_key(|(slot, _)| *slot);

    for &(slot, entity) in &existing {
        if slot >= desired {
            commands.entity(entity).despawn();
        }
    }
    for slot in 0..desired {
        if !existing
            .iter()
            .any(|(existing_slot, _)| *existing_slot == slot)
        {
            spawn_traffic_vehicle(
                &mut commands,
                &asset_server,
                &road,
                player_distance,
                slot,
                &lamps,
            );
        }
    }

    // Keep every NPC in the periodic image nearest the player. Near the finish
    // line this deliberately places traffic beyond 33 km on the visual road
    // extension; after the player wraps, the same NPC shifts to its matching
    // coordinate near zero without changing speed or lane.
    for (
        _,
        mut vehicle,
        mut position,
        mut transform,
        mut rotation,
        mut velocity,
        mut angular,
        mut gravity,
    ) in &mut traffic
    {
        if vehicle.slot >= desired {
            continue;
        }
        let canonical = road.wrap_distance(position.z);
        let relative = road.signed_loop_delta(player_distance, canonical);
        let nearest_image = player_distance + relative;
        if (position.z - nearest_image).abs() > road.length * 0.5 {
            position.z = nearest_image;
            transform.translation.z = nearest_image;
        }

        let passed = relative < -420.0;
        let too_far = relative > 1_800.0;
        if passed || too_far {
            let (_, _, offset) = slot_layout(vehicle.slot, road.lanes_per_direction);
            position.z = spawn_distance(player_distance, 700.0 + offset * 0.55);
            position.x = road.lane_center_x(vehicle.lane, vehicle.oncoming);
            position.y = road.surface_y + dimensions(visual_for_slot(vehicle.slot)).y * 0.5;
            transform.translation = position.0;
            rotation.0 = if vehicle.oncoming {
                Quat::IDENTITY
            } else {
                road.vehicle_rotation(position.z)
            };
            transform.rotation = rotation.0;
            vehicle.crashed = false;
            vehicle.current_speed_mps = vehicle.cruise_speed_mps;
            velocity.0 = Vec3::Z
                * if vehicle.oncoming {
                    -vehicle.current_speed_mps
                } else {
                    vehicle.current_speed_mps
                };
            angular.0 = Vec3::ZERO;
            gravity.0 = 0.0;
        }
    }
}

fn move_towards(current: f32, target: f32, max_delta: f32) -> f32 {
    current + (target - current).clamp(-max_delta, max_delta)
}

/// Distance that must be visible for the NPC to react and stop from `speed`.
fn obstacle_scan_distance(speed: f32) -> f32 {
    let speed = speed.max(0.0);
    TRAFFIC_STOP_GAP
        + speed * TRAFFIC_REACTION_TIME
        + speed * speed / (2.0 * TRAFFIC_BRAKE_DECEL_MPS2)
}

/// Highest safe speed for the available bumper-to-obstacle gap. For a moving
/// vehicle ahead, only the closing speed has to be shed.
fn safe_speed_for_gap(gap: f32, obstacle_speed: f32) -> f32 {
    let braking_gap = (gap - TRAFFIC_STOP_GAP).max(0.0);
    obstacle_speed.max(0.0) + (2.0 * TRAFFIC_BRAKE_DECEL_MPS2 * braking_gap).sqrt()
}

fn drive_traffic(
    time: Res<Time>,
    road: Res<RoadSpec>,
    sensor: Res<TrafficObstacleSensor>,
    spatial_query: SpatialQuery,
    obstacle_velocities: Query<&LinearVelocity, Without<TrafficVehicle>>,
    mut traffic: Query<(
        Entity,
        &Position,
        &mut TrafficVehicle,
        &mut LinearVelocity,
        &AngularVelocity,
        &mut GravityScale,
    )>,
) {
    let snapshots: Vec<(Entity, usize, bool, Vec3, f32, f32)> = traffic
        .iter()
        .map(|(entity, position, vehicle, ..)| {
            (
                entity,
                vehicle.lane,
                vehicle.oncoming,
                position.0,
                vehicle.current_speed_mps,
                dimensions(visual_for_slot(vehicle.slot)).z * 0.5,
            )
        })
        .collect();

    for (entity, position, mut vehicle, mut velocity, angular, mut gravity) in &mut traffic {
        let direction_sign = if vehicle.oncoming { -1.0 } else { 1.0 };
        let expected = Vec3::Z * direction_sign * vehicle.current_speed_mps;
        if (velocity.0 - expected).length() > 2.0 || angular.0.length() > 0.15 {
            vehicle.crashed = true;
        }
        if vehicle.crashed {
            gravity.0 = 1.0;
            vehicle.current_speed_mps = (velocity.z * direction_sign).max(0.0);
            continue;
        }
        let direction = if vehicle.oncoming {
            Dir3::NEG_Z
        } else {
            Dir3::Z
        };
        let size = dimensions(visual_for_slot(vehicle.slot));
        let half_length = size.z * 0.5;
        let mut target_speed = vehicle.cruise_speed_mps.min(MAX_TRAFFIC_SPEED_MPS);

        // Preserve orderly following across the periodic road seam. The shape
        // cast below handles the nearest physical image; this longitudinal
        // check also covers a leader immediately across the 33 km wrap.
        for (other, lane, oncoming, other_position, other_speed, other_half_length) in &snapshots {
            if *other == entity || *lane != vehicle.lane || *oncoming != vehicle.oncoming {
                continue;
            }
            let centre_gap = road.forward_loop_gap(position.z, other_position.z, direction_sign);
            if centre_gap > 0.0 {
                let bumper_gap = centre_gap - half_length - other_half_length;
                target_speed = target_speed.min(safe_speed_for_gap(bumper_gap, *other_speed));
            }
        }

        // Sweep from just beyond the front bumper. The sensor stays above the
        // road surface, spans most of one lane, and detects the player, other
        // traffic, and any static or dynamic collider blocking the lane.
        let sensor_origin = Vec3::new(
            position.x,
            road.surface_y + 0.75,
            position.z + direction_sign * (half_length + TRAFFIC_SENSOR_DEPTH * 0.5 + 0.1),
        );
        let scan_distance = obstacle_scan_distance(vehicle.current_speed_mps);
        let filter = SpatialQueryFilter::from_excluded_entities([entity]);
        if let Some(hit) = spatial_query.cast_shape(
            &sensor.0,
            sensor_origin,
            Quat::IDENTITY,
            direction,
            &ShapeCastConfig::from_max_distance(scan_distance),
            &filter,
        ) {
            let obstacle_speed = snapshots
                .iter()
                .find(|(candidate, ..)| *candidate == hit.entity)
                .map(|(_, _, oncoming, _, speed, _)| {
                    let obstacle_direction_sign = if *oncoming { -1.0 } else { 1.0 };
                    (obstacle_direction_sign * speed * direction_sign).max(0.0)
                })
                .or_else(|| {
                    obstacle_velocities
                        .get(hit.entity)
                        .ok()
                        .map(|velocity| (velocity.0.z * direction_sign).max(0.0))
                })
                .unwrap_or(0.0);
            target_speed = target_speed.min(safe_speed_for_gap(hit.distance, obstacle_speed));
        }

        let response = if target_speed < vehicle.current_speed_mps {
            TRAFFIC_BRAKE_DECEL_MPS2
        } else {
            TRAFFIC_ACCEL_MPS2
        };
        vehicle.current_speed_mps = move_towards(
            vehicle.current_speed_mps,
            target_speed,
            response * time.delta_secs(),
        )
        .clamp(0.0, MAX_TRAFFIC_SPEED_MPS);
        velocity.0 = Vec3::Z * direction_sign * vehicle.current_speed_mps;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_traffic_is_thrown_by_a_500_kmh_collision() {
        use bevy::time::TimeUpdateStrategy;
        use std::time::Duration;
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            AssetPlugin::default(),
            PhysicsPlugins::default(),
        ))
        .init_asset::<Mesh>()
        .init_resource::<RoadSpec>()
        .init_resource::<TrafficObstacleSensor>()
        .add_systems(Update, drive_traffic)
        .insert_resource(Gravity(Vec3::ZERO))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 120.0,
        )));
        app.finish();
        app.cleanup();
        let target = app
            .world_mut()
            .spawn((
                RigidBody::Dynamic,
                Collider::cuboid(2.0, 1.5, 4.0),
                Mass(1_050.0),
                NoAutoMass,
                Transform::from_xyz(0.0, 0.0, 0.0),
                SweptCcd::default(),
                TrafficVehicle {
                    slot: 2,
                    lane: 0,
                    oncoming: true,
                    cruise_speed_mps: 0.0,
                    current_speed_mps: 0.0,
                    crashed: false,
                },
                GravityScale(0.0),
            ))
            .id();
        app.world_mut().spawn((
            RigidBody::Dynamic,
            Collider::cuboid(2.0, 1.5, 4.0),
            Mass(1_900.0),
            NoAutoMass,
            Transform::from_xyz(0.0, 0.0, 10.0),
            LinearVelocity(Vec3::new(0.0, 0.0, -520.0 / 3.6)),
            SweptCcd::default(),
        ));
        for _ in 0..60 {
            app.update();
        }
        let velocity = app.world().get::<LinearVelocity>(target).unwrap();
        let position = app.world().get::<Position>(target).unwrap();
        assert!(velocity.z < -20.0, "impact velocity: {velocity:?}");
        assert!(position.z < -5.0, "impact position: {position:?}");
        assert!(app.world().get::<TrafficVehicle>(target).unwrap().crashed);
    }

    #[test]
    fn density_has_off_and_progressively_more_traffic() {
        assert_eq!(TrafficDensity::Off.vehicle_count(), 0);
        assert!(TrafficDensity::Low.vehicle_count() < TrafficDensity::Medium.vehicle_count());
        assert!(TrafficDensity::Medium.vehicle_count() < TrafficDensity::High.vehicle_count());
        assert_eq!(TrafficDensity::Off.previous(), TrafficDensity::High);
    }

    #[test]
    fn every_traffic_vehicle_obeys_the_160_kmh_limit() {
        for slot in 0..TrafficDensity::High.vehicle_count() {
            let speed = cruise_speed(slot, visual_for_slot(slot));
            assert!(speed <= MAX_TRAFFIC_SPEED_MPS);
        }
    }

    #[test]
    fn obstacle_sensor_looks_far_enough_to_stop_from_the_speed_limit() {
        let stopping_distance =
            MAX_TRAFFIC_SPEED_MPS.powi(2) / (2.0 * TRAFFIC_BRAKE_DECEL_MPS2) + TRAFFIC_STOP_GAP;
        assert!(obstacle_scan_distance(MAX_TRAFFIC_SPEED_MPS) > stopping_distance);
        assert!(obstacle_scan_distance(MAX_TRAFFIC_SPEED_MPS) > 150.0);
    }

    #[test]
    fn blocked_traffic_stops_with_a_standstill_gap_and_follows_moving_leaders() {
        assert_eq!(safe_speed_for_gap(TRAFFIC_STOP_GAP, 0.0), 0.0);
        assert_eq!(safe_speed_for_gap(0.0, 0.0), 0.0);
        assert_eq!(safe_speed_for_gap(TRAFFIC_STOP_GAP, 18.0), 18.0);
        assert!(safe_speed_for_gap(60.0, 0.0) < MAX_TRAFFIC_SPEED_MPS);
        assert!(safe_speed_for_gap(60.0, 18.0) > safe_speed_for_gap(60.0, 0.0));
    }

    #[test]
    fn traffic_uses_both_directions_and_every_lane() {
        let road = RoadSpec::default();
        let layouts: Vec<_> = (0..TrafficDensity::High.vehicle_count())
            .map(|slot| slot_layout(slot, road.lanes_per_direction))
            .collect();
        assert!(layouts.iter().any(|(_, oncoming, _)| *oncoming));
        assert!(layouts.iter().any(|(_, oncoming, _)| !*oncoming));
        for lane in 0..road.lanes_per_direction {
            assert!(
                layouts
                    .iter()
                    .any(|(candidate, oncoming, _)| *candidate == lane && *oncoming)
            );
            assert!(
                layouts
                    .iter()
                    .any(|(candidate, oncoming, _)| *candidate == lane && !*oncoming)
            );
        }
    }

    #[test]
    fn traffic_spacing_is_continuous_across_the_start_finish_seam() {
        let road = RoadSpec::default();
        assert!((spawn_distance(road.length - 20.0, 80.0) - (road.length + 60.0)).abs() < 0.01);
        assert!((road.forward_loop_gap(road.length - 20.0, 60.0, 1.0) - 80.0).abs() < 0.01);
        assert!((road.forward_loop_gap(60.0, road.length - 20.0, -1.0) - 80.0).abs() < 0.01);
    }

    #[test]
    fn bus_conversion_matches_real_world_dimensions() {
        let bus = dimensions(TrafficVisual::Bus);
        assert!((11.0..=13.0).contains(&bus.z));
        assert!((2.4..=3.2).contains(&bus.x));
        assert!((2.6..=3.5).contains(&bus.y));

        let (_, transform) = visual_model(TrafficVisual::Bus, bus.y);
        let transformed_up = transform.rotation * Vec3::Y;
        let transformed_length = transform.rotation * Vec3::X;
        assert!(transformed_up.dot(Vec3::Y) > 0.999);
        assert!(transformed_length.dot(Vec3::NEG_Z) > 0.999);
    }

    #[test]
    fn low_density_contains_both_imported_cars() {
        let visuals: Vec<_> = (0..TrafficDensity::Low.vehicle_count())
            .map(visual_for_slot)
            .collect();
        assert!(visuals.contains(&TrafficVisual::MercedesMClass2011));
        assert!(visuals.contains(&TrafficVisual::KiaPicantoGtLine2017));
    }

    #[test]
    fn traffic_never_uses_the_players_bugatti_asset() {
        for slot in 0..TrafficDensity::High.vehicle_count() {
            let visual = visual_for_slot(slot);
            let size = dimensions(visual);
            let (path, _) = visual_model(visual, size.y);
            assert_ne!(path, "models/bugatti/scene.gltf");
        }
    }

    #[test]
    fn imported_car_conversions_match_real_world_dimensions_and_heading() {
        for (visual, expected_length) in [
            (TrafficVisual::MercedesMClass2011, 4.80),
            (TrafficVisual::KiaPicantoGtLine2017, 3.61),
        ] {
            let size = dimensions(visual);
            assert!((size.z - expected_length).abs() < 0.02);
            assert!((1.6..=2.2).contains(&size.x));
            assert!((1.5..=1.9).contains(&size.y));

            let (_, transform) = visual_model(visual, size.y);
            assert!((transform.rotation * Vec3::Z).dot(Vec3::NEG_Z) > 0.999);
        }
    }

    #[test]
    fn imported_traffic_assets_and_licenses_are_present() {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/traffic");
        for folder in ["mercedes_m_class_2011", "kia_picanto_gt_line_2017"] {
            assert!(assets.join(folder).join("scene.gltf").is_file());
            assert!(assets.join(folder).join("scene.bin").is_file());
            assert!(assets.join(folder).join("license.txt").is_file());
        }
    }
}
