//! Textured tyre marks and expanding smoke laid down by individual contact
//! patches under emergency braking or power wheelspin.
//!
//! The decision to draw is based on the same wheel slip that drives ABS and
//! tyre forces.  This keeps ordinary slowing, reverse acceleration and a car
//! standing on the brake visually clean.

use std::collections::HashMap;

use avian3d::prelude::LinearVelocity;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::config::WheelId;
use super::physics::Vehicle;
use super::solver::ControlInput;
use super::wheels::WheelState;
use crate::world::lighting::LightingSettings;

/// A keyboard brake has to be essentially fully depressed before rubber is
/// deposited.  The slip check below then distinguishes actual emergency
/// braking from simply holding the pedal at low speed.
const EMERGENCY_BRAKE_INPUT: f32 = 0.90;
/// Below 30 km/h a locked tyre does not leave the long, dark path this effect
/// represents.
const MIN_MARK_SPEED_MPS: f32 = 30.0 / 3.6;
/// Negative longitudinal slip means the road is moving faster than the tyre.
const MIN_BRAKING_SLIP: f32 = 0.07;
const MIN_POWER_SLIP: f32 = 0.11;
const MIN_WHEELSPIN_THROTTLE: f32 = 0.45;
const MIN_NORMAL_FORCE: f32 = 100.0;

/// Sampling limits keep the trail continuous without creating tiny entities,
/// and prevent a reset/teleport from drawing a line across the entire road.
const MIN_SEGMENT_LENGTH: f32 = 0.16;
const MAX_SEGMENT_LENGTH: f32 = 6.0;
const SURFACE_OFFSET: f32 = 0.008;
const MAX_ACTIVE_SEGMENTS: usize = 3_200;

/// The generated alpha texture has wide transparent side gutters: measured at
/// the first non-transparent texel, rubber occupies 567 of its 1254 columns.
/// Scaling the quad by their inverse makes the *visible rubber* equal the
/// configured tyre width instead of compressing every difference between cars.
const VISIBLE_RUBBER_FRACTION: f32 = 567.0 / 1_254.0;

const SOLID_SECONDS: f32 = 14.0;
const FADE_SECONDS: f32 = 10.0;
const FADE_STEPS: usize = 8;
const SMOKE_FADE_STEPS: usize = 16;
/// Spatial sampling keeps smoke continuous when a wheel travels several
/// metres between rendered frames at extreme speed.
const MIN_SMOKE_SPACING: f32 = 0.30;
const MAX_SMOKE_SAMPLES_PER_WHEEL_FRAME: usize = 12;
const MAX_SMOKE_BRIDGE_DISTANCE: f32 = 18.0;
const MAX_SMOKE_PARTICLES: usize = 1_400;

#[derive(Resource)]
pub struct SkidMarkAssets {
    unit_plane: Handle<Mesh>,
    fade_materials: [Handle<StandardMaterial>; FADE_STEPS],
    smoke_mesh: Handle<Mesh>,
    smoke_materials: [Handle<StandardMaterial>; SMOKE_FADE_STEPS],
}

#[derive(Debug, Clone, Copy)]
struct TrailPoint {
    position: Vec3,
    normal: Vec3,
}

#[derive(Resource, Default)]
pub struct SkidTrailState {
    last_contacts: HashMap<(Entity, WheelId), TrailPoint>,
    active_segments: usize,
}

#[derive(Resource, Default)]
pub struct TireSmokeState {
    emitters: HashMap<(Entity, WheelId), SmokeEmitterPoint>,
    sequence: u64,
    active_particles: usize,
}

#[derive(Debug, Clone, Copy)]
struct SmokeEmitterPoint {
    position: Vec3,
    normal: Vec3,
    emitted_at: f32,
}

#[derive(Component)]
pub struct EmergencySkidMark {
    born_at: f32,
}

#[derive(Component)]
pub struct TireSmokeParticle {
    age: f32,
    lifetime: f32,
    velocity: Vec3,
    expansion: f32,
    material_stage: usize,
}

#[derive(Component)]
pub(crate) struct TireSmokeLayer;

/// Load one shared texture/mesh and a small material palette. Every segment
/// reuses them, so even a four-wheel stop does not continually allocate GPU
/// assets.
pub fn setup_skid_mark_assets(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let texture = asset_server.load("effects/tire_skid.png");
    let unit_plane = meshes.add(Plane3d::default().mesh().size(1.0, 1.0));
    let fade_materials = core::array::from_fn(|step| {
        let remaining = 1.0 - step as f32 / FADE_STEPS as f32;
        materials.add(StandardMaterial {
            base_color: Color::srgba(0.52, 0.50, 0.48, 0.88 * remaining),
            base_color_texture: Some(texture.clone()),
            perceptual_roughness: 0.98,
            metallic: 0.0,
            alpha_mode: AlphaMode::Blend,
            // Keep the transparent decal just above both asphalt and paint
            // without requiring a physical slab or collider.
            depth_bias: 2.0,
            ..default()
        })
    });
    let smoke_texture = asset_server.load("effects/tire_smoke_v3.png");
    let smoke_mesh = meshes.add(Rectangle::new(1.0, 1.0));
    let smoke_materials = core::array::from_fn(|step| {
        let life = 1.0 - step as f32 / SMOKE_FADE_STEPS as f32;
        materials.add(StandardMaterial {
            base_color: Color::srgba(0.76, 0.77, 0.78, 0.23 * life * life),
            base_color_texture: Some(smoke_texture.clone()),
            perceptual_roughness: 1.0,
            metallic: 0.0,
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            double_sided: true,
            cull_mode: None,
            ..default()
        })
    });

    commands.insert_resource(SkidMarkAssets {
        unit_plane,
        fade_materials,
        smoke_mesh,
        smoke_materials,
    });
}

/// Lay down one independently sized strip per slipping wheel.
pub fn spawn_emergency_skid_marks(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<SkidMarkAssets>,
    settings: Res<LightingSettings>,
    vehicles: Query<(Entity, &Vehicle)>,
    mut trails: ResMut<SkidTrailState>,
) {
    if !settings.tire_marks_enabled() {
        trails.last_contacts.clear();
        return;
    }
    trails
        .last_contacts
        .retain(|(entity, _), _| vehicles.contains(*entity));

    for (vehicle_entity, vehicle) in &vehicles {
        for id in WheelId::ALL {
            let wheel = &vehicle.dynamics.wheels[id.index()];
            let key = (vehicle_entity, id);
            if !leaves_tire_mark(
                &vehicle.input,
                vehicle.dynamics.transmission.gear,
                vehicle.dynamics.forward_speed,
                wheel,
            ) {
                // Breaking continuity here is important: a second braking
                // event must never be joined to the previous one by a diagonal.
                trails.last_contacts.remove(&key);
                continue;
            }

            let current = TrailPoint {
                position: wheel.contact_point,
                normal: wheel.contact_normal.normalize_or(Vec3::Y),
            };
            let Some(previous) = trails.last_contacts.insert(key, current) else {
                continue;
            };

            let displacement = current.position - previous.position;
            let distance = displacement.length();
            if !(MIN_SEGMENT_LENGTH..=MAX_SEGMENT_LENGTH).contains(&distance)
                || trails.active_segments >= MAX_ACTIVE_SEGMENTS
            {
                continue;
            }

            let normal = (previous.normal + current.normal).normalize_or(Vec3::Y);
            let along =
                (displacement - normal * displacement.dot(normal)).normalize_or(Vec3::NEG_Z);
            let right = normal.cross(along).normalize_or(Vec3::X);
            let rotation = Quat::from_mat3(&Mat3::from_cols(right, normal, along));
            let tire_width = vehicle.config.wheel(id).tire.width;

            commands.spawn((
                Name::new(format!("EmergencySkidMark{}", id.short_name())),
                EmergencySkidMark {
                    born_at: time.elapsed_secs(),
                },
                Mesh3d(assets.unit_plane.clone()),
                MeshMaterial3d(assets.fade_materials[0].clone()),
                Transform {
                    translation: (previous.position + current.position) * 0.5
                        + normal * SURFACE_OFFSET,
                    rotation,
                    scale: Vec3::new(skid_quad_width(tire_width), 1.0, distance),
                },
                NotShadowCaster,
            ));
            trails.active_segments += 1;
        }
    }
}

/// Emit translucent volumes from the real wheel contact patches. Samples are
/// distributed over the distance travelled since the previous emission, so a
/// 1,000 km/h wheel cannot leave one isolated puff per frame.
pub fn spawn_tire_smoke(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<SkidMarkAssets>,
    settings: Res<LightingSettings>,
    vehicles: Query<(Entity, &Vehicle, Option<&LinearVelocity>)>,
    mut smoke: ResMut<TireSmokeState>,
) {
    if !settings.tire_smoke_enabled() {
        smoke.emitters.clear();
        return;
    }
    let now = time.elapsed_secs();
    smoke
        .emitters
        .retain(|(entity, _), _| vehicles.contains(*entity));

    for (vehicle_entity, vehicle, chassis_velocity) in &vehicles {
        for id in WheelId::ALL {
            let wheel = &vehicle.dynamics.wheels[id.index()];
            let key = (vehicle_entity, id);
            let strength = tire_smoke_strength(
                &vehicle.input,
                vehicle.dynamics.transmission.gear,
                vehicle.dynamics.forward_speed,
                wheel,
            );
            if strength <= 0.0 {
                smoke.emitters.remove(&key);
                continue;
            }

            let interval = 0.085 + (0.026 - 0.085) * strength;
            let current = SmokeEmitterPoint {
                position: wheel.contact_point,
                normal: wheel.contact_normal.normalize_or(Vec3::Y),
                emitted_at: now,
            };
            let previous = smoke.emitters.get(&key).copied();
            let distance = previous.map_or(0.0, |point| point.position.distance(current.position));
            let time_due = previous.is_none_or(|point| now - point.emitted_at >= interval);
            let tire_width = vehicle.config.wheel(id).tire.width;
            let spacing = smoke_spacing(tire_width, strength);
            let spatial_samples = smoke_spatial_sample_count(distance, spacing);
            let sample_count = spatial_samples
                .max(usize::from(time_due))
                .min(MAX_SMOKE_SAMPLES_PER_WHEEL_FRAME)
                .min(MAX_SMOKE_PARTICLES.saturating_sub(smoke.active_particles));
            if sample_count == 0 {
                // A teleport/reset must start a fresh trail rather than filling
                // the whole jump with smoke. At the particle budget, consume
                // the travelled distance so emission resumes without a burst.
                if distance > MAX_SMOKE_BRIDGE_DISTANCE
                    || smoke.active_particles >= MAX_SMOKE_PARTICLES
                {
                    smoke.emitters.insert(key, current);
                }
                continue;
            }

            let path_scale =
                smoke_path_coverage_scale(distance, sample_count, tire_width, strength);
            for sample_index in 0..sample_count {
                smoke.sequence = smoke.sequence.wrapping_add(1);
                let seed = smoke.sequence ^ vehicle_entity.to_bits() ^ ((id.index() as u64) << 40);
                let sideways = signed_noise(seed);
                let lengthwise = signed_noise(seed.rotate_left(19));
                let lift = 0.48 + 0.55 * unit_noise(seed.rotate_left(37));
                let initial_scale = tire_width * (0.72 + 0.38 * strength) * path_scale;
                let sample_t = (sample_index + 1) as f32 / sample_count as f32;
                let sample_position = previous.map_or(current.position, |point| {
                    point.position.lerp(current.position, sample_t)
                });
                let sample_normal = previous.map_or(current.normal, |point| {
                    point
                        .normal
                        .lerp(current.normal, sample_t)
                        .normalize_or(Vec3::Y)
                });
                let speed_mps = chassis_velocity.map_or(0.0, |velocity| velocity.0.length());
                let inherited_velocity =
                    chassis_velocity.map_or(Vec3::ZERO, |velocity| velocity.0 * 0.22);
                let particle = commands
                    .spawn((
                        Name::new(format!("TireSmoke{}", id.short_name())),
                        TireSmokeParticle {
                            age: 0.0,
                            lifetime: smoke_lifetime(
                                speed_mps,
                                strength,
                                unit_noise(seed.rotate_left(11)),
                            ),
                            velocity: inherited_velocity
                                + Vec3::new(sideways * 0.34, lift, lengthwise * 0.28),
                            expansion: 0.68 + 0.52 * strength,
                            material_stage: 0,
                        },
                        Transform::from_translation(
                            sample_position + sample_normal * (0.11 + tire_width * 0.2),
                        )
                        .with_scale(Vec3::splat(initial_scale)),
                        Visibility::default(),
                    ))
                    .id();
                commands.entity(particle).with_children(|cloud| {
                    // Four differently oriented sheets reveal separate fine
                    // wisps from every angle without looking like a flat card.
                    for (yaw, roll, scale) in [
                        (0.0_f32, 0.17_f32, Vec3::new(1.15, 1.0, 1.0)),
                        (0.785, -0.23, Vec3::new(0.94, 1.12, 1.0)),
                        (1.571, 0.31, Vec3::new(1.04, 0.90, 1.0)),
                        (2.356, -0.12, Vec3::new(0.88, 1.06, 1.0)),
                    ] {
                        cloud.spawn((
                            TireSmokeLayer,
                            Mesh3d(assets.smoke_mesh.clone()),
                            MeshMaterial3d(assets.smoke_materials[0].clone()),
                            Transform::from_rotation(
                                Quat::from_rotation_y(yaw) * Quat::from_rotation_z(roll),
                            )
                            .with_scale(scale),
                            NotShadowCaster,
                        ));
                    }
                });
                smoke.active_particles += 1;
            }
            smoke.emitters.insert(key, current);
        }
    }
}

/// A preset or toggle change removes already-visible FX immediately rather
/// than leaving their lifetime timers running over a clean performance scene.
pub fn clear_disabled_tire_fx(
    mut commands: Commands,
    settings: Res<LightingSettings>,
    mut trails: ResMut<SkidTrailState>,
    mut smoke: ResMut<TireSmokeState>,
    marks: Query<Entity, With<EmergencySkidMark>>,
    particles: Query<Entity, With<TireSmokeParticle>>,
) {
    if !settings.is_changed() {
        return;
    }
    if !settings.tire_marks_enabled() {
        for entity in &marks {
            commands.entity(entity).despawn();
        }
        trails.last_contacts.clear();
        trails.active_segments = 0;
    }
    if !settings.tire_smoke_enabled() {
        for entity in &particles {
            commands.entity(entity).despawn();
        }
        smoke.emitters.clear();
        smoke.active_particles = 0;
    }
}

/// Expand, convect and fade smoke without allocating a material per particle.
pub(crate) fn animate_tire_smoke(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<SkidMarkAssets>,
    mut smoke: ResMut<TireSmokeState>,
    mut particles: Query<(Entity, &mut TireSmokeParticle, &mut Transform, &Children)>,
    mut layers: Query<&mut MeshMaterial3d<StandardMaterial>, With<TireSmokeLayer>>,
) {
    let dt = time.delta_secs();
    for (entity, mut particle, mut transform, children) in &mut particles {
        particle.age += dt;
        if particle.age >= particle.lifetime {
            commands.entity(entity).despawn();
            smoke.active_particles = smoke.active_particles.saturating_sub(1);
            continue;
        }

        transform.translation += particle.velocity * dt;
        particle.velocity.y += 0.18 * dt;
        particle.velocity *= (-1.15 * dt).exp();
        transform.scale *= 1.0 + particle.expansion * dt;
        transform.rotate_y(0.32 * dt);

        let progress = (particle.age / particle.lifetime).clamp(0.0, 0.999_9);
        let stage = (progress * SMOKE_FADE_STEPS as f32) as usize;
        if stage != particle.material_stage {
            particle.material_stage = stage;
            for child in children.iter() {
                if let Ok(mut material) = layers.get_mut(child) {
                    material.0 = assets.smoke_materials[stage].clone();
                }
            }
        }
    }
}

/// Age marks through shared alpha levels, then remove them. Shared materials
/// avoid one `StandardMaterial` allocation and upload per trail segment.
pub fn fade_emergency_skid_marks(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<SkidMarkAssets>,
    mut trails: ResMut<SkidTrailState>,
    mut marks: Query<(
        Entity,
        &EmergencySkidMark,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let now = time.elapsed_secs();
    for (entity, mark, mut material) in &mut marks {
        let age = now - mark.born_at;
        if age >= SOLID_SECONDS + FADE_SECONDS {
            commands.entity(entity).despawn();
            trails.active_segments = trails.active_segments.saturating_sub(1);
            continue;
        }

        let fade = ((age - SOLID_SECONDS) / FADE_SECONDS).clamp(0.0, 0.999_9);
        let step = (fade * FADE_STEPS as f32) as usize;
        if material.0 != assets.fade_materials[step] {
            material.0 = assets.fade_materials[step].clone();
        }
    }
}

#[inline]
fn is_emergency_braking(
    input: &ControlInput,
    gear: i8,
    forward_speed: f32,
    wheel: &WheelState,
) -> bool {
    gear >= 0
        && input.brake >= EMERGENCY_BRAKE_INPUT
        && forward_speed >= MIN_MARK_SPEED_MPS
        && wheel.grounded
        && wheel.normal_force >= MIN_NORMAL_FORCE
        && wheel.brake_torque > 0.0
        && wheel.tire.slip_ratio <= -MIN_BRAKING_SLIP
}

#[inline]
fn is_power_wheelspin(input: &ControlInput, gear: i8, wheel: &WheelState) -> bool {
    gear > 0
        && input.throttle >= MIN_WHEELSPIN_THROTTLE
        && wheel.grounded
        && wheel.normal_force >= MIN_NORMAL_FORCE
        && wheel.drive_torque.abs() > 40.0
        && wheel.tire.slip_ratio >= MIN_POWER_SLIP
}

#[inline]
fn leaves_tire_mark(
    input: &ControlInput,
    gear: i8,
    forward_speed: f32,
    wheel: &WheelState,
) -> bool {
    is_emergency_braking(input, gear, forward_speed, wheel)
        || is_power_wheelspin(input, gear, wheel)
}

#[inline]
fn tire_smoke_strength(
    input: &ControlInput,
    gear: i8,
    forward_speed: f32,
    wheel: &WheelState,
) -> f32 {
    if is_power_wheelspin(input, gear, wheel) {
        return ((wheel.tire.slip_ratio - MIN_POWER_SLIP) / 0.75).clamp(0.15, 1.0);
    }
    if is_emergency_braking(input, gear, forward_speed, wheel) {
        return ((-wheel.tire.slip_ratio - MIN_BRAKING_SLIP) / 0.55).clamp(0.15, 1.0);
    }
    0.0
}

#[inline]
fn smoke_spacing(tire_width: f32, strength: f32) -> f32 {
    (tire_width * (1.18 + 0.18 * strength)).max(MIN_SMOKE_SPACING)
}

#[inline]
fn smoke_spatial_sample_count(distance: f32, spacing: f32) -> usize {
    if distance < spacing || distance > MAX_SMOKE_BRIDGE_DISTANCE {
        return 0;
    }
    ((distance / spacing).ceil() as usize).min(MAX_SMOKE_SAMPLES_PER_WHEEL_FRAME)
}

#[inline]
fn smoke_path_coverage_scale(
    distance: f32,
    sample_count: usize,
    tire_width: f32,
    strength: f32,
) -> f32 {
    if sample_count == 0 || distance < MIN_SMOKE_SPACING {
        return 1.0;
    }
    let actual_spacing = distance / sample_count as f32;
    let authored_puff_width = tire_width * (0.72 + 0.38 * strength);
    (actual_spacing / authored_puff_width.max(0.01)).clamp(1.0, 2.45)
}

#[inline]
fn smoke_lifetime(speed_mps: f32, strength: f32, variation: f32) -> f32 {
    let still_air_lifetime = 1.45 + 1.15 * strength + 0.30 * variation;
    let high_speed_blend = (speed_mps / 180.0).clamp(0.0, 1.0);
    still_air_lifetime * (1.0 - 0.50 * high_speed_blend)
}

#[inline]
fn unit_noise(mut value: u64) -> f32 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^= value >> 31;
    (value as u32) as f32 / u32::MAX as f32
}

#[inline]
fn signed_noise(value: u64) -> f32 {
    unit_noise(value) * 2.0 - 1.0
}

#[inline]
fn skid_quad_width(tire_width: f32) -> f32 {
    tire_width / VISIBLE_RUBBER_FRACTION
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::{VehicleKind, presets::bugatti_veyron_super_sport};

    fn locking_wheel() -> WheelState {
        let config = bugatti_veyron_super_sport();
        let mut wheel = WheelState::new(&config.wheels[0]);
        wheel.grounded = true;
        wheel.normal_force = 4_000.0;
        wheel.brake_torque = 2_000.0;
        wheel.tire.slip_ratio = -0.20;
        wheel
    }

    #[test]
    fn only_a_fast_full_brake_lockup_creates_a_mark() {
        let full_brake = ControlInput {
            brake: 1.0,
            ..default()
        };
        let wheel = locking_wheel();
        assert!(is_emergency_braking(&full_brake, 3, 40.0, &wheel));

        let partial_brake = ControlInput {
            brake: 0.75,
            ..default()
        };
        assert!(!is_emergency_braking(&partial_brake, 3, 40.0, &wheel));
        assert!(!is_emergency_braking(&full_brake, 3, 5.0, &wheel));

        let mut rolling_wheel = wheel;
        rolling_wheel.tire.slip_ratio = -0.02;
        assert!(!is_emergency_braking(&full_brake, 3, 40.0, &rolling_wheel));
    }

    #[test]
    fn reverse_acceleration_never_looks_like_emergency_braking() {
        let reverse_pedal = ControlInput {
            brake: 1.0,
            ..default()
        };
        assert!(!is_emergency_braking(
            &reverse_pedal,
            -1,
            40.0,
            &locking_wheel()
        ));
    }

    #[test]
    fn driven_wheelspin_creates_smoke_and_a_tire_mark() {
        let mut wheel = locking_wheel();
        wheel.brake_torque = 0.0;
        wheel.drive_torque = 1_500.0;
        wheel.tire.slip_ratio = 0.65;
        let throttle = ControlInput {
            throttle: 1.0,
            ..default()
        };

        assert!(is_power_wheelspin(&throttle, 1, &wheel));
        assert!(leaves_tire_mark(&throttle, 1, 2.0, &wheel));
        assert!(tire_smoke_strength(&throttle, 1, 2.0, &wheel) > 0.5);

        wheel.drive_torque = 0.0;
        assert!(!is_power_wheelspin(&throttle, 1, &wheel));
        assert_eq!(tire_smoke_strength(&throttle, 1, 2.0, &wheel), 0.0);
    }

    #[test]
    fn detailed_smoke_sprite_is_a_real_rgba_png() {
        let png = include_bytes!("../../assets/effects/tire_smoke_v3.png");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        // PNG IHDR byte 25 is colour type; 6 means true-colour with alpha.
        assert_eq!(png[25], 6);
        assert!(
            png.len() > 250_000,
            "smoke texture was unexpectedly reduced"
        );
    }

    #[test]
    fn extreme_speed_smoke_is_sampled_continuously_but_teleports_are_not() {
        let spacing = smoke_spacing(0.30, 1.0);
        let distance_per_frame_at_1_000_kmh_and_60_fps = (1_000.0 / 3.6) / 60.0;
        let samples =
            smoke_spatial_sample_count(distance_per_frame_at_1_000_kmh_and_60_fps, spacing);
        assert_eq!(samples, MAX_SMOKE_SAMPLES_PER_WHEEL_FRAME);
        assert!(distance_per_frame_at_1_000_kmh_and_60_fps / (samples as f32) < 0.40);
        assert_eq!(smoke_spatial_sample_count(20.0, spacing), 0);
    }

    #[test]
    fn fast_smoke_puffs_expand_enough_to_cover_the_sample_gap() {
        let distance = (1_000.0 / 3.6) / 60.0;
        let samples = smoke_spatial_sample_count(distance, smoke_spacing(0.30, 1.0));
        let scale = smoke_path_coverage_scale(distance, samples, 0.30, 1.0);
        let visible_puff_width = 0.30 * (0.72 + 0.38) * scale;
        assert!(visible_puff_width >= distance / samples as f32);
    }

    #[test]
    fn visible_mark_width_matches_every_configured_tire() {
        for kind in VehicleKind::ALL {
            let config = kind.config();
            for wheel in &config.wheels {
                let visible_width = skid_quad_width(wheel.tire.width) * VISIBLE_RUBBER_FRACTION;
                assert!(
                    (visible_width - wheel.tire.width).abs() < 1.0e-6,
                    "{} {}: mark {:.3} m, tyre {:.3} m",
                    config.name,
                    wheel.id.short_name(),
                    visible_width,
                    wheel.tire.width
                );
            }
        }
    }

    #[test]
    fn bugatti_rear_tracks_are_visibly_wider_than_its_front_tracks() {
        let config = bugatti_veyron_super_sport();
        let front = config.wheel(WheelId::FrontLeft).tire.width;
        let rear = config.wheel(WheelId::RearLeft).tire.width;
        assert_eq!(front, 0.265);
        assert_eq!(rear, 0.365);
        assert!(
            rear / front > 1.35,
            "rear/front width ratio is {}",
            rear / front
        );
        assert!(
            skid_quad_width(rear) - skid_quad_width(front) > 0.20,
            "transparent-gutter calibration must preserve the visible 100 mm difference"
        );
        for kind in VehicleKind::ALL {
            if kind == VehicleKind::Veyron {
                continue;
            }
            let other = kind.config();
            let other_rear = other.wheel(WheelId::RearLeft).tire.width;
            assert!(
                rear > other_rear,
                "Bugatti rear tyre {rear:.3} m must exceed {} rear tyre {other_rear:.3} m",
                other.name
            );
        }
    }

    #[test]
    fn one_moving_sample_spawns_four_independent_wheel_tracks() {
        let config = bugatti_veyron_super_sport();
        let mut vehicle = Vehicle::new(config.clone(), Transform::IDENTITY);
        vehicle.input.brake = 1.0;
        vehicle.dynamics.transmission.gear = 2;
        vehicle.dynamics.forward_speed = 30.0;
        for (wheel, wheel_config) in vehicle.dynamics.wheels.iter_mut().zip(&config.wheels) {
            wheel.grounded = true;
            wheel.normal_force = 4_000.0;
            wheel.brake_torque = 2_000.0;
            wheel.tire.slip_ratio = -0.20;
            wheel.contact_normal = Vec3::Y;
            wheel.contact_point = Vec3::new(wheel_config.anchor.x, 0.0, wheel_config.anchor.z);
        }

        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .insert_resource({
                let mut settings = LightingSettings::default();
                settings.apply_preset(crate::world::lighting::LightingQuality::Cinematic);
                settings
            })
            .init_resource::<SkidTrailState>()
            .insert_resource(SkidMarkAssets {
                unit_plane: Handle::default(),
                fade_materials: core::array::from_fn(|_| Handle::default()),
                smoke_mesh: Handle::default(),
                smoke_materials: core::array::from_fn(|_| Handle::default()),
            })
            .add_systems(Update, spawn_emergency_skid_marks)
            .world_mut()
            .spawn(vehicle);

        // First sample starts four trails without drawing a zero-length quad.
        app.update();
        {
            let world = app.world_mut();
            let mut vehicles = world.query::<&mut Vehicle>();
            let mut vehicle = vehicles.single_mut(world).expect("test vehicle");
            for wheel in &mut vehicle.dynamics.wheels {
                wheel.contact_point.z -= 0.5;
            }
        }
        app.update();

        let world = app.world_mut();
        let mut marks = world.query::<(&Name, &EmergencySkidMark, &Transform)>();
        let spawned: Vec<_> = marks
            .iter(world)
            .map(|(name, _, transform)| (name.as_str().to_owned(), transform.scale.x))
            .collect();
        assert_eq!(spawned.len(), 4);
        for (name, quad_width) in spawned {
            let wheel = WheelId::ALL
                .into_iter()
                .find(|wheel| name.ends_with(wheel.short_name()))
                .expect("mark name contains its wheel id");
            let configured = config.wheel(wheel).tire.width;
            assert!(
                (quad_width * VISIBLE_RUBBER_FRACTION - configured).abs() < 1.0e-6,
                "{} visible track width",
                wheel.short_name()
            );
        }
        assert_eq!(world.resource::<SkidTrailState>().active_segments, 4);
    }

    #[test]
    fn disabling_tire_fx_clears_existing_marks_and_smoke() {
        let mut settings = LightingSettings::default();
        settings.apply_preset(crate::world::lighting::LightingQuality::Performance);
        let mut app = App::new();
        app.insert_resource(settings)
            .init_resource::<SkidTrailState>()
            .init_resource::<TireSmokeState>()
            .add_systems(Update, clear_disabled_tire_fx);
        app.world_mut()
            .resource_mut::<SkidTrailState>()
            .active_segments = 1;
        app.world_mut()
            .resource_mut::<TireSmokeState>()
            .active_particles = 1;
        let mark = app
            .world_mut()
            .spawn(EmergencySkidMark { born_at: 0.0 })
            .id();
        let smoke = app
            .world_mut()
            .spawn(TireSmokeParticle {
                age: 0.0,
                lifetime: 1.0,
                velocity: Vec3::ZERO,
                expansion: 0.0,
                material_stage: 0,
            })
            .id();
        app.update();
        assert!(app.world().get_entity(mark).is_err());
        assert!(app.world().get_entity(smoke).is_err());
        assert_eq!(app.world().resource::<SkidTrailState>().active_segments, 0);
        assert_eq!(app.world().resource::<TireSmokeState>().active_particles, 0);
    }
}
