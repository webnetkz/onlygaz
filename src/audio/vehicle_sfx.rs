//! Physics-driven braking and vehicle impact sound effects.

use avian3d::prelude::{CollisionStart, ContactGraph};
use bevy::audio::{AudioSink, AudioSinkPlayback, Volume};
use bevy::prelude::*;

use crate::camera::controller::GameCamera;
use crate::vehicle::physics::{Vehicle, VehicleTag};

use super::AudioSettings;

const BRAKE_SKID_PATH: &str = "audio/w16/tires/skid/tire_skid_lock_ext_loop.ogg";
const WHEELSPIN_PATH: &str = "audio/w16/tires/wheelspin/burnout_loop.ogg";
const IMPACT_LIGHT: [&str; 3] = [
    "audio/w16/impacts/impact_guardrail_light_01.ogg",
    "audio/w16/impacts/impact_guardrail_light_02.ogg",
    "audio/w16/impacts/impact_guardrail_light_03.ogg",
];
const IMPACT_MEDIUM: [&str; 3] = [
    "audio/w16/impacts/impact_guardrail_medium_01.ogg",
    "audio/w16/impacts/impact_guardrail_medium_02.ogg",
    "audio/w16/impacts/impact_guardrail_medium_03.ogg",
];
const IMPACT_HEAVY: [&str; 3] = [
    "audio/w16/impacts/impact_guardrail_heavy_01.ogg",
    "audio/w16/impacts/impact_guardrail_heavy_02.ogg",
    "audio/w16/impacts/impact_guardrail_heavy_03.ogg",
];
const IMPACT_HEAVY_INTERIOR: &str = "audio/w16/impacts/impact_guardrail_heavy_int.ogg";
const IMPACT_CAR_LIGHT: [&str; 3] = [
    "audio/w16/impacts/impact_car_bump_01.ogg",
    "audio/w16/impacts/impact_car_bump_02.ogg",
    "audio/w16/impacts/impact_car_bump_03.ogg",
];
const IMPACT_CAR_HEAVY: [&str; 2] = [
    "audio/w16/impacts/impact_car_heavy_01.ogg",
    "audio/w16/impacts/impact_car_heavy_02.ogg",
];
const IMPACT_SCRAPE: [&str; 2] = [
    "audio/w16/scrapes/scrape_guardrail_short_01.ogg",
    "audio/w16/scrapes/scrape_guardrail_short_02.ogg",
];
const IMPACT_DEBRIS: [&str; 5] = [
    "audio/w16/glass_debris/debris_metal_01.ogg",
    "audio/w16/glass_debris/debris_metal_02.ogg",
    "audio/w16/glass_debris/debris_metal_03.ogg",
    "audio/w16/glass_debris/debris_plastic_01.ogg",
    "audio/w16/glass_debris/debris_plastic_02.ogg",
];
const GLASS_BREAK: [&str; 3] = [
    "audio/w16/glass_debris/glass_shatter_01.ogg",
    "audio/w16/glass_debris/glass_shatter_02.ogg",
    "audio/w16/glass_debris/glass_shatter_03.ogg",
];

/// Keep decoded source assets resident before the first crash; no file lookup
/// or load request is needed on the impact frame.
#[derive(Resource)]
pub(crate) struct CollisionSounds {
    light: [Handle<AudioSource>; 3],
    medium: [Handle<AudioSource>; 3],
    heavy: [Handle<AudioSource>; 3],
    heavy_interior: Handle<AudioSource>,
    car_light: [Handle<AudioSource>; 3],
    car_heavy: [Handle<AudioSource>; 2],
    scrape: [Handle<AudioSource>; 2],
    debris: [Handle<AudioSource>; 5],
    glass: [Handle<AudioSource>; 3],
}

pub(crate) fn load_collision_sounds(mut commands: Commands, server: Res<AssetServer>) {
    commands.insert_resource(CollisionSounds {
        light: IMPACT_LIGHT.map(|path| server.load(path)),
        medium: IMPACT_MEDIUM.map(|path| server.load(path)),
        heavy: IMPACT_HEAVY.map(|path| server.load(path)),
        heavy_interior: server.load(IMPACT_HEAVY_INTERIOR),
        car_light: IMPACT_CAR_LIGHT.map(|path| server.load(path)),
        car_heavy: IMPACT_CAR_HEAVY.map(|path| server.load(path)),
        scrape: IMPACT_SCRAPE.map(|path| server.load(path)),
        debris: IMPACT_DEBRIS.map(|path| server.load(path)),
        glass: GLASS_BREAK.map(|path| server.load(path)),
    });
}

#[derive(Resource, Default)]
pub struct VehicleSfxState {
    sequence: usize,
    last_impact_at: f32,
    last_break_at: f32,
    brake_skid_gain: f32,
    wheelspin_gain: f32,
}

#[derive(Component)]
pub(crate) struct BrakeSkidVoice;

#[derive(Component)]
pub(crate) struct WheelspinVoice;

pub(crate) fn setup_brake_skid_sfx(mut commands: Commands, server: Res<AssetServer>) {
    commands.spawn((
        Name::new("All vehicles brake skid"),
        BrakeSkidVoice,
        AudioPlayer::new(server.load(BRAKE_SKID_PATH)),
        PlaybackSettings {
            volume: Volume::Linear(0.0),
            ..PlaybackSettings::LOOP
        },
    ));
}

pub(crate) fn setup_wheelspin_sfx(mut commands: Commands, server: Res<AssetServer>) {
    commands.spawn((
        Name::new("All vehicles wheelspin"),
        WheelspinVoice,
        AudioPlayer::new(server.load(WHEELSPIN_PATH)),
        PlaybackSettings {
            volume: Volume::Linear(0.0),
            ..PlaybackSettings::LOOP
        },
    ));
}

#[inline]
fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0).max(f32::EPSILON)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The recording is reserved for actual braking under load. Gentle pedal
/// inputs and walking speed stay quiet; wheel lock makes the layer dominant.
fn brake_skid_target(vehicle: &Vehicle) -> f32 {
    let pedal = vehicle.input.brake.max(vehicle.input.handbrake);
    let speed = vehicle.dynamics.forward_speed.abs();
    let mut braking_slip: f32 = 0.0;
    for wheel in &vehicle.dynamics.wheels {
        if wheel.grounded && wheel.normal_force > 100.0 {
            braking_slip = braking_slip.max((-wheel.tire.slip_ratio).max(0.0));
        }
    }
    let pedal_presence = smoothstep(0.18, 0.92, pedal);
    let speed_presence = smoothstep(3.0, 16.0, speed);
    let skid_presence = smoothstep(0.04, 0.46, braking_slip);
    (pedal_presence * speed_presence * (0.20 + skid_presence * 0.80)).clamp(0.0, 1.0)
}

/// Power-on tyre slip. Reverse uses the brake pedal as its accelerator, so
/// the power demand follows the selected gear rather than a fixed key.
fn wheelspin_target(vehicle: &Vehicle) -> (f32, f32) {
    let power = if vehicle.dynamics.transmission.gear < 0 {
        vehicle.input.brake
    } else {
        vehicle.input.throttle
    };
    let mut driven_slip: f32 = 0.0;
    for wheel in &vehicle.dynamics.wheels {
        if wheel.grounded && wheel.normal_force > 100.0 {
            driven_slip = driven_slip.max(wheel.tire.slip_ratio.abs());
        }
    }
    let power_presence = smoothstep(0.12, 0.82, power);
    let slip_presence = smoothstep(0.08, 0.68, driven_slip);
    (
        (power_presence * slip_presence).clamp(0.0, 1.0),
        driven_slip,
    )
}

pub(crate) fn update_brake_skid_sfx(
    time: Res<Time>,
    audio: Res<AudioSettings>,
    mut state: ResMut<VehicleSfxState>,
    vehicles: Query<&Vehicle, With<VehicleTag>>,
    mut voices: Query<&mut AudioSink, With<BrakeSkidVoice>>,
) {
    let (target, speed) = vehicles
        .single()
        .map(|vehicle| {
            (
                brake_skid_target(vehicle),
                vehicle.dynamics.forward_speed.abs(),
            )
        })
        .unwrap_or((0.0, 0.0));
    let rate = if target > state.brake_skid_gain {
        15.0
    } else {
        6.0
    };
    let blend = 1.0 - (-rate * time.delta_secs().clamp(0.0, 0.1)).exp();
    state.brake_skid_gain += (target - state.brake_skid_gain) * blend;
    let pitch = 0.88 + smoothstep(3.0, 55.0, speed) * 0.18;
    for mut sink in &mut voices {
        sink.set_volume(Volume::Linear(
            state.brake_skid_gain * audio.effects_gain() * 0.88,
        ));
        sink.set_speed(pitch);
    }
}

pub(crate) fn update_wheelspin_sfx(
    time: Res<Time>,
    audio: Res<AudioSettings>,
    mut state: ResMut<VehicleSfxState>,
    vehicles: Query<&Vehicle, With<VehicleTag>>,
    mut voices: Query<&mut AudioSink, With<WheelspinVoice>>,
) {
    let (target, slip, speed) = vehicles
        .single()
        .map(|vehicle| {
            let (gain, slip) = wheelspin_target(vehicle);
            (gain, slip, vehicle.dynamics.forward_speed.abs())
        })
        .unwrap_or((0.0, 0.0, 0.0));
    let rate = if target > state.wheelspin_gain {
        18.0
    } else {
        7.0
    };
    let blend = 1.0 - (-rate * time.delta_secs().clamp(0.0, 0.1)).exp();
    state.wheelspin_gain += (target - state.wheelspin_gain) * blend;
    let pitch = 0.84 + smoothstep(0.08, 0.9, slip) * 0.18 + smoothstep(0.0, 45.0, speed) * 0.08;
    for mut sink in &mut voices {
        sink.set_volume(Volume::Linear(
            state.wheelspin_gain * audio.effects_gain() * 0.82,
        ));
        sink.set_speed(pitch);
    }
}

pub(crate) fn silence_brake_skid_sfx(
    mut state: ResMut<VehicleSfxState>,
    mut voices: Query<&mut AudioSink, With<BrakeSkidVoice>>,
) {
    state.brake_skid_gain = 0.0;
    for mut sink in &mut voices {
        sink.set_volume(Volume::Linear(0.0));
    }
}

pub(crate) fn silence_wheelspin_sfx(
    mut state: ResMut<VehicleSfxState>,
    mut voices: Query<&mut AudioSink, With<WheelspinVoice>>,
) {
    state.wheelspin_gain = 0.0;
    for mut sink in &mut voices {
        sink.set_volume(Volume::Linear(0.0));
    }
}

fn spawn_one_shot(
    commands: &mut Commands,
    name: impl Into<String>,
    source: Handle<AudioSource>,
    gain: f32,
    speed: f32,
) {
    commands.spawn((
        Name::new(name.into()),
        AudioPlayer::new(source),
        PlaybackSettings {
            volume: Volume::Linear(gain.clamp(0.0, 1.0)),
            speed: speed.clamp(0.82, 1.18),
            ..PlaybackSettings::DESPAWN
        },
    ));
}

fn audible_collision_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "barrier",
        "guard",
        "containment",
        "traffic",
        "vehicle",
        "bus",
    ]
    .iter()
    .any(|part| lower.contains(part))
}

pub(crate) fn play_collision_sfx(
    mut commands: Commands,
    time: Res<Time>,
    audio: Res<AudioSettings>,
    sounds: Res<CollisionSounds>,
    mut state: ResMut<VehicleSfxState>,
    mut collisions: MessageReader<CollisionStart>,
    contacts: Res<ContactGraph>,
    vehicles: Query<(Entity, &Vehicle), With<VehicleTag>>,
    cameras: Query<&GameCamera>,
    names: Query<&Name>,
) {
    let Ok((vehicle_entity, vehicle)) = vehicles.single() else {
        return;
    };
    let interior = cameras
        .single()
        .is_ok_and(|camera| camera.mode.is_interior() && !camera.rear_view);
    for collision in collisions.read() {
        let candidates = [
            collision.collider1,
            collision.collider2,
            collision.body1.unwrap_or(collision.collider1),
            collision.body2.unwrap_or(collision.collider2),
        ];
        if !candidates.contains(&vehicle_entity) {
            continue;
        }
        let (audible, car_hit) = candidates
            .iter()
            .copied()
            .filter(|e| *e != vehicle_entity)
            .filter_map(|e| names.get(e).ok())
            .fold((false, false), |(audible, car_hit), name| {
                let lower = name.as_str().to_ascii_lowercase();
                (
                    audible || audible_collision_name(&lower),
                    car_hit
                        || ["traffic", "vehicle", "bus"]
                            .iter()
                            .any(|part| lower.contains(part)),
                )
            });
        if !audible {
            continue;
        }
        let Some((_edge, pair)) = contacts.get(collision.collider1, collision.collider2) else {
            continue;
        };
        let delta_speed = pair.total_normal_impulse_magnitude() / vehicle.config.mass.max(1.0);
        if delta_speed < 0.45 || time.elapsed_secs() - state.last_impact_at < 0.09 {
            continue;
        }
        state.last_impact_at = time.elapsed_secs();
        let index = state.sequence;
        state.sequence = state.sequence.wrapping_add(1);
        let road_speed = vehicle.dynamics.forward_speed.abs();
        let (path, label, gain) = if car_hit && delta_speed >= 5.0 {
            (
                sounds.car_heavy[index % sounds.car_heavy.len()].clone(),
                "car heavy",
                0.68,
            )
        } else if car_hit {
            (
                sounds.car_light[index % sounds.car_light.len()].clone(),
                "car light",
                0.54,
            )
        } else if delta_speed >= 12.0 && interior {
            (sounds.heavy_interior.clone(), "heavy interior", 0.68)
        } else if delta_speed >= 12.0 {
            (
                sounds.heavy[index % sounds.heavy.len()].clone(),
                "heavy",
                0.68,
            )
        } else if delta_speed >= 5.0 {
            (
                sounds.medium[index % sounds.medium.len()].clone(),
                "medium",
                0.58,
            )
        } else if delta_speed < 1.8 && road_speed > 7.0 {
            (
                sounds.scrape[index % sounds.scrape.len()].clone(),
                "scrape",
                0.45,
            )
        } else {
            (
                sounds.light[index % sounds.light.len()].clone(),
                "light",
                0.48,
            )
        };
        let master = audio.effects_gain();
        spawn_one_shot(
            &mut commands,
            format!("Vehicle {label} impact"),
            path,
            master * gain * if interior { 0.8 } else { 1.0 },
            0.96 + (index % 5) as f32 * 0.02,
        );
    }
}

/// Several panes/panels may fail in one physics step. Layer at most one
/// glass recording and one metal recording per crash, not twenty voices.
pub(crate) fn play_breakage_sfx(
    mut commands: Commands,
    time: Res<Time>,
    audio: Res<AudioSettings>,
    sounds: Res<CollisionSounds>,
    mut state: ResMut<VehicleSfxState>,
    mut events: MessageReader<crate::vehicle::damage::PartBroken>,
    local_car: Query<(), With<VehicleTag>>,
) {
    let mut glass = false;
    let mut metal = false;
    for event in events.read() {
        if !local_car.contains(event.vehicle) {
            continue;
        }
        use crate::vehicle::damage::DamagePartKind::*;
        glass |= matches!(
            event.kind,
            SideGlassLeft
                | SideGlassRight
                | Windshield
                | RearGlass
                | MirrorLeft
                | MirrorRight
                | HeadlightLeft
                | HeadlightRight
                | TaillightLeft
                | TaillightRight
        );
        metal |= !matches!(
            event.kind,
            SideGlassLeft | SideGlassRight | Windshield | RearGlass
        );
    }
    if (!glass && !metal) || time.elapsed_secs() - state.last_break_at < 0.09 {
        return;
    }
    state.last_break_at = time.elapsed_secs();
    let index = state.sequence;
    state.sequence = state.sequence.wrapping_add(1);
    let gain = audio.effects_gain();
    if glass {
        spawn_one_shot(
            &mut commands,
            "Vehicle glass breaking",
            sounds.glass[index % sounds.glass.len()].clone(),
            gain * 0.78,
            1.0,
        );
    }
    if metal {
        spawn_one_shot(
            &mut commands,
            "Vehicle panel tearing away",
            sounds.debris[index % sounds.debris.len()].clone(),
            gain * 0.7,
            1.0,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_active_collision_asset_is_packaged() {
        for path in IMPACT_LIGHT
            .into_iter()
            .chain(IMPACT_MEDIUM)
            .chain(IMPACT_HEAVY)
            .chain([IMPACT_HEAVY_INTERIOR])
            .chain(IMPACT_CAR_LIGHT)
            .chain(IMPACT_CAR_HEAVY)
            .chain(IMPACT_SCRAPE)
            .chain(IMPACT_DEBRIS)
            .chain(GLASS_BREAK)
        {
            assert!(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("assets")
                    .join(path)
                    .is_file(),
                "missing {path}"
            );
        }
    }

    #[test]
    fn brake_asset_is_packaged_as_vorbis() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join(BRAKE_SKID_PATH);
        let bytes = std::fs::read(path).expect("W16 brake skid asset");
        assert_eq!(&bytes[..4], b"OggS");
        assert!(bytes.windows(6).any(|window| window == b"vorbis"));
    }

    #[test]
    fn wheelspin_asset_is_packaged_as_vorbis() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join(WHEELSPIN_PATH);
        let bytes = std::fs::read(path).expect("W16 wheelspin asset");
        assert_eq!(&bytes[..4], b"OggS");
        assert!(bytes.windows(6).any(|window| window == b"vorbis"));
    }

    #[test]
    fn brake_sound_requires_speed_and_rises_with_wheel_lock() {
        let mut vehicle = Vehicle::new(
            crate::vehicle::presets::bugatti_veyron_super_sport(),
            Transform::IDENTITY,
        );
        for wheel in &mut vehicle.dynamics.wheels {
            wheel.grounded = true;
            wheel.normal_force = 4_000.0;
        }
        vehicle.input.brake = 1.0;
        assert_eq!(brake_skid_target(&vehicle), 0.0);
        vehicle.dynamics.forward_speed = 30.0;
        let rolling = brake_skid_target(&vehicle);
        vehicle.dynamics.wheels[0].tire.slip_ratio = -0.5;
        let locked = brake_skid_target(&vehicle);
        assert!(rolling > 0.0);
        assert!(locked > rolling * 4.0);
    }

    #[test]
    fn wheelspin_sound_requires_power_and_real_tire_slip() {
        let mut vehicle = Vehicle::new(
            crate::vehicle::presets::bugatti_veyron_super_sport(),
            Transform::IDENTITY,
        );
        for wheel in &mut vehicle.dynamics.wheels {
            wheel.grounded = true;
            wheel.normal_force = 4_000.0;
        }
        vehicle.input.throttle = 1.0;
        assert_eq!(wheelspin_target(&vehicle).0, 0.0);
        vehicle.dynamics.wheels[2].tire.slip_ratio = 0.7;
        assert!(wheelspin_target(&vehicle).0 > 0.9);
        vehicle.input.throttle = 0.0;
        assert_eq!(wheelspin_target(&vehicle).0, 0.0);
    }

    #[test]
    fn collision_filter_excludes_the_ground_but_accepts_rails_and_cars() {
        assert!(!audible_collision_name("RoadCollider"));
        assert!(!audible_collision_name("TerrainCollider"));
        assert!(audible_collision_name("BarrierCollider12_R"));
        assert!(audible_collision_name("TrafficVehicle"));
    }
}
