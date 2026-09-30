//! Connect the simulated W16 to the production sound pack.
//!
//! The pack contains separate load/coast RPM loops and interior/exterior
//! recordings. Only the player's car gets the full sound bank; traffic remains
//! inexpensive and does not create dozens of decoders per NPC.

use bevy::prelude::*;
use w16_audio::{CameraView, CarAudio, W16Shift};

use crate::camera::controller::{GameCamera, SideView};
use crate::game::GameState;
use crate::vehicle::physics::{Vehicle, VehicleTag};

use super::AudioSettings;

#[derive(Resource, Debug, Clone, Reflect)]
#[reflect(Resource)]
pub struct EngineAudioSettings {
    pub enabled: bool,
}

impl Default for EngineAudioSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Resource, Default)]
pub(crate) struct PreviousGear {
    gear: Option<i8>,
    reset_generation: u32,
}

/// The simulator's ECU stops at 6900 RPM, while the supplied W16 recordings
/// top out at 6400. Map the usable range, rather than pitching the final loop
/// sharply upwards or making the limiter sound during F7's unlimited mode.
fn sample_rpm(vehicle: &Vehicle) -> f32 {
    let engine = &vehicle.config.engine;
    let span = (engine.limiter_rpm - engine.idle_rpm).max(1.0);
    let fraction = (vehicle.dynamics.rpm() - engine.idle_rpm) / span;
    (800.0 + fraction * 5600.0).clamp(800.0, 6400.0)
}

fn tire_slip(vehicle: &Vehicle) -> f32 {
    vehicle
        .dynamics
        .wheels
        .iter()
        .filter(|wheel| wheel.grounded)
        .map(|wheel| {
            let lateral = wheel.tire.slip_angle().abs() / 0.35;
            let longitudinal = wheel.tire.slip_ratio.abs() / 0.8;
            lateral.max(longitudinal)
        })
        .fold(0.0_f32, f32::max)
        .clamp(0.0, 1.0)
}

/// Runs in PreUpdate so the pack's mixer sees a complete physics snapshot
/// before its Update systems. Level trims leave room for impacts and UI sounds.
pub(crate) fn feed_w16_audio(
    state: Res<State<GameState>>,
    audio: Res<AudioSettings>,
    engine: Res<EngineAudioSettings>,
    cameras: Query<&GameCamera>,
    mut previous: ResMut<PreviousGear>,
    mut shifts: MessageWriter<W16Shift>,
    mut vehicles: Query<(Entity, &Vehicle, &mut CarAudio), With<VehicleTag>>,
) {
    let Ok((entity, vehicle, mut car)) = vehicles.single_mut() else {
        return;
    };
    let driving = *state.get() == GameState::Driving;
    let engine_level = if engine.enabled {
        audio.engine_gain()
    } else {
        0.0
    };
    let gear = vehicle.dynamics.transmission.gear;
    if driving
        && previous.reset_generation == vehicle.reset_generation
        && previous
            .gear
            .is_some_and(|old| old > 0 && gear > 0 && old != gear)
    {
        shifts.write(W16Shift {
            car: entity,
            up: gear > previous.gear.unwrap_or(gear),
        });
    }
    previous.gear = Some(gear);
    previous.reset_generation = vehicle.reset_generation;

    car.rpm = sample_rpm(vehicle);
    car.throttle = vehicle.input.throttle.clamp(0.0, 1.0);
    car.speed_kmh = vehicle.speed_kmh().abs();
    car.slip = tire_slip(vehicle);
    car.brake = vehicle.input.brake.max(vehicle.input.handbrake);
    // The dedicated brake layer handles longitudinal lock. The pack's tire
    // layers remain responsible for rolling and lateral squeal.
    car.wheels_locked = false;
    car.limiter_active = vehicle.dynamics.engine.limiter_active;
    car.view = cameras
        .single()
        .map(|camera| {
            if camera.mode.is_interior() && !camera.rear_view && camera.side_view == SideView::None
            {
                CameraView::Interior
            } else {
                CameraView::Exterior
            }
        })
        .unwrap_or(CameraView::Exterior);
    car.volume = if driving { audio.effects_gain() } else { 0.0 };
    car.crackle_chance = if driving && engine.enabled { 0.28 } else { 0.0 };
    car.mix.engine = 0.60 * engine_level;
    car.mix.turbo = 0.20 * engine_level;
    car.mix.blowoff = 0.42 * engine_level;
    car.mix.pops = 0.24 * engine_level;
    car.mix.shifts = 0.50 * engine_level;
    car.mix.tires = 0.26;
    car.mix.skid = 0.30;
    car.mix.wind = 0.14;
    car.mix.transmission = 0.12;
    car.mix.scrape = 0.40;
}

/// `M` mutes the motor but leaves tires, wind and collision effects audible.
pub(crate) fn toggle_engine_audio(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<crate::vehicle::controller::KeyBindings>,
    mut settings: ResMut<EngineAudioSettings>,
    rebind: Option<Res<crate::ui::menu::RebindState>>,
) {
    if rebind.as_ref().is_some_and(|state| state.pending.is_some()) {
        return;
    }
    if bindings.just_pressed(&keys, crate::vehicle::controller::ControlAction::MuteAudio) {
        settings.enabled = !settings.enabled;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampled_engine_speed_is_bounded_by_the_pack() {
        let mut car = Vehicle::new(
            crate::vehicle::presets::bugatti_veyron_super_sport(),
            Transform::IDENTITY,
        );
        assert!((sample_rpm(&car) - 800.0).abs() < 0.1);
        car.dynamics.engine.angular_velocity =
            crate::vehicle::engine::rpm_to_rad(car.config.engine.limiter_rpm);
        assert!((sample_rpm(&car) - 6400.0).abs() < 0.1);
        car.dynamics.engine.angular_velocity = crate::vehicle::engine::rpm_to_rad(20_000.0);
        assert_eq!(sample_rpm(&car), 6400.0);
    }

    #[test]
    fn grounded_tire_slip_drives_skid_layers() {
        let mut car = Vehicle::new(
            crate::vehicle::presets::bugatti_veyron_super_sport(),
            Transform::IDENTITY,
        );
        assert_eq!(tire_slip(&car), 0.0);
        car.dynamics.wheels[0].grounded = true;
        car.dynamics.wheels[0].tire.slip_ratio = 0.8;
        assert_eq!(tire_slip(&car), 1.0);
    }
}
