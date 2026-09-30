//! Driver input: collection, smoothing and the keys that are not pedals.
//!
//! Raw device input lands in [`DriverInput`]; a second stage smooths it into the
//! [`ControlInput`] the solver consumes. Keeping the two apart is what makes
//! adding a gamepad a matter of writing one more system into [`DriverInput`] -
//! nothing downstream needs to change.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::physics::Vehicle;
use super::solver::{ControlInput, low_speed_arcade_blend};

/// How quickly a keyboard "pedal" is pressed and released [1/s].
const THROTTLE_PRESS_RATE: f32 = 4.5;
const THROTTLE_RELEASE_RATE: f32 = 9.0;
const BRAKE_PRESS_RATE: f32 = 6.5;
const BRAKE_RELEASE_RATE: f32 = 11.0;
const HANDBRAKE_RATE: f32 = 12.0;
const NITRO_PRESS_RATE: f32 = 12.0;
const NITRO_RELEASE_RATE: f32 = 18.0;

/// Above this speed steering input starts becoming progressively calmer.
const HIGH_SPEED_STEERING_START_KMH: f32 = 200.0;
/// The reduction reaches its full strength here.
const HIGH_SPEED_STEERING_FULL_KMH: f32 = 400.0;
/// At maximum speed, steering builds at 35 % of the normal input rate.
const HIGH_SPEED_INPUT_RATE_FACTOR: f32 = 0.35;
/// Returning to centre remains faster for stability, but is still softened.
const HIGH_SPEED_RETURN_RATE_FACTOR: f32 = 0.65;
/// A digital steering key reaches useful lock much sooner around town. The
/// multiplier is eased away by the same 100 km/h boundary as the yaw assist.
const LOW_SPEED_INPUT_RATE_FACTOR: f32 = 1.65;
const LOW_SPEED_RETURN_RATE_FACTOR: f32 = 1.35;

/// Raw, unsmoothed driver demand. Range 0..1, except `steer` which is -1..1.
#[derive(Debug, Clone, Copy, Default, Component, Reflect)]
#[reflect(Component)]
pub struct DriverInput {
    pub throttle: f32,
    pub brake: f32,
    /// -1 = full left, +1 = full right.
    pub steer: f32,
    pub handbrake: f32,
    /// 0..1 demand for the temporary nitro thrust.
    pub nitro: f32,
}

/// The slot edited by the controls menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub enum KeySlot {
    Primary,
    Alternate,
}

/// Actions that can be rebound in the controls menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum ControlAction {
    Throttle,
    Brake,
    SteerLeft,
    SteerRight,
    Handbrake,
    Nitro,
    Reset,
    CycleCamera,
    ToggleDebug,
    TogglePhysicsGizmos,
    ToggleRearView,
    ViewLeft,
    ViewRight,
    ToggleAbs,
    ToggleTriplePower,
    CycleQuality,
    MuteAudio,
    PreviousRadioTrack,
    NextRadioTrack,
    Menu,
    Quit,
}

impl ControlAction {
    pub const ALL: [Self; 21] = [
        Self::Throttle,
        Self::Brake,
        Self::SteerLeft,
        Self::SteerRight,
        Self::Handbrake,
        Self::Nitro,
        Self::Reset,
        Self::CycleCamera,
        Self::ToggleDebug,
        Self::TogglePhysicsGizmos,
        Self::ToggleRearView,
        Self::ViewLeft,
        Self::ViewRight,
        Self::ToggleAbs,
        Self::ToggleTriplePower,
        Self::CycleQuality,
        Self::MuteAudio,
        Self::PreviousRadioTrack,
        Self::NextRadioTrack,
        Self::Menu,
        Self::Quit,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Throttle => "Throttle",
            Self::Brake => "Brake / reverse",
            Self::SteerLeft => "Steer left",
            Self::SteerRight => "Steer right",
            Self::Handbrake => "Handbrake",
            Self::Nitro => "Nitro",
            Self::Reset => "Reset vehicle",
            Self::CycleCamera => "Cycle camera",
            Self::ToggleDebug => "Telemetry",
            Self::TogglePhysicsGizmos => "Physics gizmos",
            Self::ToggleRearView => "Rear view (hold)",
            Self::ViewLeft => "Left view (hold)",
            Self::ViewRight => "Right view (hold)",
            Self::ToggleAbs => "ABS",
            Self::ToggleTriplePower => "Power x3",
            Self::CycleQuality => "Graphics preset",
            Self::MuteAudio => "Engine audio",
            Self::PreviousRadioTrack => "Previous radio track",
            Self::NextRadioTrack => "Next radio track",
            Self::Menu => "Pause / menu",
            Self::Quit => "Quit game",
        }
    }
}

/// Key bindings, kept in one place and editable at run time.
#[derive(Debug, Clone, Resource, Reflect, Serialize, Deserialize)]
#[reflect(Resource)]
#[serde(default)]
pub struct KeyBindings {
    pub throttle: KeyCode,
    pub throttle_alt: Option<KeyCode>,
    pub brake: KeyCode,
    pub brake_alt: Option<KeyCode>,
    pub steer_left: KeyCode,
    pub steer_left_alt: Option<KeyCode>,
    pub steer_right: KeyCode,
    pub steer_right_alt: Option<KeyCode>,
    pub handbrake: KeyCode,
    pub handbrake_alt: Option<KeyCode>,
    pub nitro: KeyCode,
    pub nitro_alt: Option<KeyCode>,
    pub reset: KeyCode,
    pub reset_alt: Option<KeyCode>,
    pub cycle_camera: KeyCode,
    pub cycle_camera_alt: Option<KeyCode>,
    pub toggle_debug: KeyCode,
    pub toggle_debug_alt: Option<KeyCode>,
    pub toggle_physics_gizmos: KeyCode,
    pub toggle_physics_gizmos_alt: Option<KeyCode>,
    pub toggle_rear_view: KeyCode,
    pub toggle_rear_view_alt: Option<KeyCode>,
    pub view_left: KeyCode,
    pub view_left_alt: Option<KeyCode>,
    pub view_right: KeyCode,
    pub view_right_alt: Option<KeyCode>,
    pub toggle_abs: KeyCode,
    pub toggle_abs_alt: Option<KeyCode>,
    pub toggle_triple_power: KeyCode,
    pub toggle_triple_power_alt: Option<KeyCode>,
    pub cycle_quality: KeyCode,
    pub cycle_quality_alt: Option<KeyCode>,
    pub mute_audio: KeyCode,
    pub mute_audio_alt: Option<KeyCode>,
    pub previous_radio_track: KeyCode,
    pub previous_radio_track_alt: Option<KeyCode>,
    pub next_radio_track: KeyCode,
    pub next_radio_track_alt: Option<KeyCode>,
    pub menu: KeyCode,
    pub menu_alt: Option<KeyCode>,
    pub quit: KeyCode,
    pub quit_alt: Option<KeyCode>,
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            throttle: KeyCode::KeyW,
            throttle_alt: Some(KeyCode::ArrowUp),
            brake: KeyCode::KeyS,
            brake_alt: Some(KeyCode::ArrowDown),
            steer_left: KeyCode::KeyA,
            steer_left_alt: Some(KeyCode::ArrowLeft),
            steer_right: KeyCode::KeyD,
            steer_right_alt: Some(KeyCode::ArrowRight),
            handbrake: KeyCode::Space,
            handbrake_alt: None,
            nitro: KeyCode::ShiftLeft,
            nitro_alt: Some(KeyCode::ShiftRight),
            reset: KeyCode::KeyR,
            reset_alt: None,
            cycle_camera: KeyCode::KeyC,
            cycle_camera_alt: None,
            toggle_debug: KeyCode::F3,
            toggle_debug_alt: None,
            toggle_physics_gizmos: KeyCode::F4,
            toggle_physics_gizmos_alt: None,
            toggle_rear_view: KeyCode::KeyB,
            toggle_rear_view_alt: None,
            view_left: KeyCode::Comma,
            view_left_alt: None,
            view_right: KeyCode::Period,
            view_right_alt: None,
            toggle_abs: KeyCode::F8,
            toggle_abs_alt: None,
            toggle_triple_power: KeyCode::F7,
            toggle_triple_power_alt: None,
            cycle_quality: KeyCode::F5,
            cycle_quality_alt: None,
            mute_audio: KeyCode::KeyM,
            mute_audio_alt: None,
            previous_radio_track: KeyCode::Digit9,
            previous_radio_track_alt: None,
            next_radio_track: KeyCode::Digit0,
            next_radio_track_alt: None,
            menu: KeyCode::Escape,
            menu_alt: None,
            quit: KeyCode::KeyQ,
            quit_alt: None,
        }
    }
}

impl KeyBindings {
    pub(crate) fn migrate_default_arrows(&mut self) {
        for (action, primary, alternate) in [
            (ControlAction::Throttle, KeyCode::KeyW, KeyCode::ArrowUp),
            (ControlAction::Brake, KeyCode::KeyS, KeyCode::ArrowDown),
            (ControlAction::SteerLeft, KeyCode::KeyA, KeyCode::ArrowLeft),
            (
                ControlAction::SteerRight,
                KeyCode::KeyD,
                KeyCode::ArrowRight,
            ),
        ] {
            if self.primary(action) == primary
                && self.alternate(action).is_none()
                && !self.is_bound_to_other(action, alternate)
            {
                self.set(action, KeySlot::Alternate, Some(alternate));
            }
        }
    }
    pub fn primary(&self, action: ControlAction) -> KeyCode {
        match action {
            ControlAction::Throttle => self.throttle,
            ControlAction::Brake => self.brake,
            ControlAction::SteerLeft => self.steer_left,
            ControlAction::SteerRight => self.steer_right,
            ControlAction::Handbrake => self.handbrake,
            ControlAction::Nitro => self.nitro,
            ControlAction::Reset => self.reset,
            ControlAction::CycleCamera => self.cycle_camera,
            ControlAction::ToggleDebug => self.toggle_debug,
            ControlAction::TogglePhysicsGizmos => self.toggle_physics_gizmos,
            ControlAction::ToggleRearView => self.toggle_rear_view,
            ControlAction::ViewLeft => self.view_left,
            ControlAction::ViewRight => self.view_right,
            ControlAction::ToggleAbs => self.toggle_abs,
            ControlAction::ToggleTriplePower => self.toggle_triple_power,
            ControlAction::CycleQuality => self.cycle_quality,
            ControlAction::MuteAudio => self.mute_audio,
            ControlAction::PreviousRadioTrack => self.previous_radio_track,
            ControlAction::NextRadioTrack => self.next_radio_track,
            ControlAction::Menu => self.menu,
            ControlAction::Quit => self.quit,
        }
    }

    pub fn alternate(&self, action: ControlAction) -> Option<KeyCode> {
        match action {
            ControlAction::Throttle => self.throttle_alt,
            ControlAction::Brake => self.brake_alt,
            ControlAction::SteerLeft => self.steer_left_alt,
            ControlAction::SteerRight => self.steer_right_alt,
            ControlAction::Handbrake => self.handbrake_alt,
            ControlAction::Nitro => self.nitro_alt,
            ControlAction::Reset => self.reset_alt,
            ControlAction::CycleCamera => self.cycle_camera_alt,
            ControlAction::ToggleDebug => self.toggle_debug_alt,
            ControlAction::TogglePhysicsGizmos => self.toggle_physics_gizmos_alt,
            ControlAction::ToggleRearView => self.toggle_rear_view_alt,
            ControlAction::ViewLeft => self.view_left_alt,
            ControlAction::ViewRight => self.view_right_alt,
            ControlAction::ToggleAbs => self.toggle_abs_alt,
            ControlAction::ToggleTriplePower => self.toggle_triple_power_alt,
            ControlAction::CycleQuality => self.cycle_quality_alt,
            ControlAction::MuteAudio => self.mute_audio_alt,
            ControlAction::PreviousRadioTrack => self.previous_radio_track_alt,
            ControlAction::NextRadioTrack => self.next_radio_track_alt,
            ControlAction::Menu => self.menu_alt,
            ControlAction::Quit => self.quit_alt,
        }
    }

    pub fn set(&mut self, action: ControlAction, slot: KeySlot, key: Option<KeyCode>) {
        match (action, slot) {
            (ControlAction::Throttle, KeySlot::Primary) => {
                self.throttle = key.unwrap_or(self.throttle)
            }
            (ControlAction::Throttle, KeySlot::Alternate) => self.throttle_alt = key,
            (ControlAction::Brake, KeySlot::Primary) => self.brake = key.unwrap_or(self.brake),
            (ControlAction::Brake, KeySlot::Alternate) => self.brake_alt = key,
            (ControlAction::SteerLeft, KeySlot::Primary) => {
                self.steer_left = key.unwrap_or(self.steer_left)
            }
            (ControlAction::SteerLeft, KeySlot::Alternate) => self.steer_left_alt = key,
            (ControlAction::SteerRight, KeySlot::Primary) => {
                self.steer_right = key.unwrap_or(self.steer_right)
            }
            (ControlAction::SteerRight, KeySlot::Alternate) => self.steer_right_alt = key,
            (ControlAction::Handbrake, KeySlot::Primary) => {
                self.handbrake = key.unwrap_or(self.handbrake)
            }
            (ControlAction::Handbrake, KeySlot::Alternate) => self.handbrake_alt = key,
            (ControlAction::Nitro, KeySlot::Primary) => self.nitro = key.unwrap_or(self.nitro),
            (ControlAction::Nitro, KeySlot::Alternate) => self.nitro_alt = key,
            (ControlAction::Reset, KeySlot::Primary) => self.reset = key.unwrap_or(self.reset),
            (ControlAction::Reset, KeySlot::Alternate) => self.reset_alt = key,
            (ControlAction::CycleCamera, KeySlot::Primary) => {
                self.cycle_camera = key.unwrap_or(self.cycle_camera)
            }
            (ControlAction::CycleCamera, KeySlot::Alternate) => self.cycle_camera_alt = key,
            (ControlAction::ToggleDebug, KeySlot::Primary) => {
                self.toggle_debug = key.unwrap_or(self.toggle_debug)
            }
            (ControlAction::ToggleDebug, KeySlot::Alternate) => self.toggle_debug_alt = key,
            (ControlAction::TogglePhysicsGizmos, KeySlot::Primary) => {
                self.toggle_physics_gizmos = key.unwrap_or(self.toggle_physics_gizmos)
            }
            (ControlAction::TogglePhysicsGizmos, KeySlot::Alternate) => {
                self.toggle_physics_gizmos_alt = key
            }
            (ControlAction::ToggleRearView, KeySlot::Primary) => {
                self.toggle_rear_view = key.unwrap_or(self.toggle_rear_view)
            }
            (ControlAction::ToggleRearView, KeySlot::Alternate) => self.toggle_rear_view_alt = key,
            (ControlAction::ViewLeft, KeySlot::Primary) => {
                self.view_left = key.unwrap_or(self.view_left)
            }
            (ControlAction::ViewLeft, KeySlot::Alternate) => self.view_left_alt = key,
            (ControlAction::ViewRight, KeySlot::Primary) => {
                self.view_right = key.unwrap_or(self.view_right)
            }
            (ControlAction::ViewRight, KeySlot::Alternate) => self.view_right_alt = key,
            (ControlAction::ToggleAbs, KeySlot::Primary) => {
                self.toggle_abs = key.unwrap_or(self.toggle_abs)
            }
            (ControlAction::ToggleAbs, KeySlot::Alternate) => self.toggle_abs_alt = key,
            (ControlAction::ToggleTriplePower, KeySlot::Primary) => {
                self.toggle_triple_power = key.unwrap_or(self.toggle_triple_power)
            }
            (ControlAction::ToggleTriplePower, KeySlot::Alternate) => {
                self.toggle_triple_power_alt = key
            }
            (ControlAction::CycleQuality, KeySlot::Primary) => {
                self.cycle_quality = key.unwrap_or(self.cycle_quality)
            }
            (ControlAction::CycleQuality, KeySlot::Alternate) => self.cycle_quality_alt = key,
            (ControlAction::MuteAudio, KeySlot::Primary) => {
                self.mute_audio = key.unwrap_or(self.mute_audio)
            }
            (ControlAction::MuteAudio, KeySlot::Alternate) => self.mute_audio_alt = key,
            (ControlAction::PreviousRadioTrack, KeySlot::Primary) => {
                self.previous_radio_track = key.unwrap_or(self.previous_radio_track)
            }
            (ControlAction::PreviousRadioTrack, KeySlot::Alternate) => {
                self.previous_radio_track_alt = key
            }
            (ControlAction::NextRadioTrack, KeySlot::Primary) => {
                self.next_radio_track = key.unwrap_or(self.next_radio_track)
            }
            (ControlAction::NextRadioTrack, KeySlot::Alternate) => self.next_radio_track_alt = key,
            (ControlAction::Menu, KeySlot::Primary) => self.menu = key.unwrap_or(self.menu),
            (ControlAction::Menu, KeySlot::Alternate) => self.menu_alt = key,
            (ControlAction::Quit, KeySlot::Primary) => self.quit = key.unwrap_or(self.quit),
            (ControlAction::Quit, KeySlot::Alternate) => self.quit_alt = key,
        }
    }

    pub fn is_bound_to_other(&self, action: ControlAction, key: KeyCode) -> bool {
        ControlAction::ALL.iter().any(|other| {
            *other != action && (self.primary(*other) == key || self.alternate(*other) == Some(key))
        })
    }

    #[inline]
    pub fn pressed(&self, keys: &ButtonInput<KeyCode>, action: ControlAction) -> bool {
        keys.pressed(self.primary(action))
            || self.alternate(action).is_some_and(|key| keys.pressed(key))
    }

    #[inline]
    pub fn just_pressed(&self, keys: &ButtonInput<KeyCode>, action: ControlAction) -> bool {
        keys.just_pressed(self.primary(action))
            || self
                .alternate(action)
                .is_some_and(|key| keys.just_pressed(key))
    }
}

/// Read the keyboard into [`DriverInput`].
pub fn collect_keyboard_input(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<KeyBindings>,
    mut query: Query<&mut DriverInput>,
) {
    let throttle = bindings.pressed(&keys, ControlAction::Throttle) as u8 as f32;
    let brake = bindings.pressed(&keys, ControlAction::Brake) as u8 as f32;
    let left = bindings.pressed(&keys, ControlAction::SteerLeft) as u8 as f32;
    let right = bindings.pressed(&keys, ControlAction::SteerRight) as u8 as f32;
    let handbrake = bindings.pressed(&keys, ControlAction::Handbrake) as u8 as f32;
    let nitro = bindings.pressed(&keys, ControlAction::Nitro) as u8 as f32;

    for mut input in &mut query {
        input.throttle = throttle;
        input.brake = brake;
        input.steer = right - left;
        input.handbrake = handbrake;
        input.nitro = nitro;
    }
}

/// Turn the raw demand into the smoothed [`ControlInput`] the solver uses.
///
/// A keyboard is a pair of switches; without this stage the car would get a
/// step input on every key press, which no real pedal or steering rack can do.
pub fn smooth_driver_input(time: Res<Time>, mut query: Query<(&DriverInput, &mut Vehicle)>) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    for (raw, mut vehicle) in &mut query {
        let steering_cfg = vehicle.config.steering.clone();
        let current: ControlInput = vehicle.input;

        let throttle = approach(
            current.throttle,
            raw.throttle,
            THROTTLE_PRESS_RATE,
            THROTTLE_RELEASE_RATE,
            dt,
        );
        let brake = approach(
            current.brake,
            raw.brake,
            BRAKE_PRESS_RATE,
            BRAKE_RELEASE_RATE,
            dt,
        );
        let handbrake = approach(
            current.handbrake,
            raw.handbrake,
            HANDBRAKE_RATE,
            HANDBRAKE_RATE,
            dt,
        );
        let nitro = approach(
            current.nitro,
            raw.nitro,
            NITRO_PRESS_RATE,
            NITRO_RELEASE_RATE,
            dt,
        );

        // Steering uses a different rate towards centre than away from it, so
        // the car straightens up quickly when the key is released.
        let target = raw.steer.clamp(-1.0, 1.0);
        let returning = target.abs() < 1.0e-3 || target.signum() != current.steering.signum();
        let (base_rate, minimum_factor) = if returning {
            (steering_cfg.return_rate, HIGH_SPEED_RETURN_RATE_FACTOR)
        } else {
            (steering_cfg.input_rate, HIGH_SPEED_INPUT_RATE_FACTOR)
        };
        let speed_kmh = vehicle.speed_kmh().abs();
        let low_speed_factor = if returning {
            LOW_SPEED_RETURN_RATE_FACTOR
        } else {
            LOW_SPEED_INPUT_RATE_FACTOR
        };
        let arcade_rate =
            base_rate * (1.0 + (low_speed_factor - 1.0) * low_speed_arcade_blend(speed_kmh / 3.6));
        let rate = high_speed_steering_rate(arcade_rate, speed_kmh, minimum_factor);
        let steering = move_towards(current.steering, target, rate * dt);

        vehicle.input = ControlInput {
            throttle,
            brake,
            steering,
            handbrake,
            nitro,
        };
    }
}

/// Asymmetric first-order approach with separate press and release rates.
#[inline]
fn approach(current: f32, target: f32, press_rate: f32, release_rate: f32, dt: f32) -> f32 {
    let rate = if target > current {
        press_rate
    } else {
        release_rate
    };
    move_towards(current, target, rate * dt)
}

/// Move `current` towards `target` by at most `max_delta`.
#[inline]
fn move_towards(current: f32, target: f32, max_delta: f32) -> f32 {
    let delta = target - current;
    if delta.abs() <= max_delta {
        target
    } else {
        current + delta.signum() * max_delta
    }
}

/// Reduce a steering slew rate smoothly above 200 km/h.
///
/// Smoothstep gives zero slope at both ends, so crossing the 200 km/h threshold
/// cannot itself create a change that the driver feels through the steering.
#[inline]
fn high_speed_steering_rate(base_rate: f32, speed_kmh: f32, minimum_factor: f32) -> f32 {
    let t = ((speed_kmh.abs() - HIGH_SPEED_STEERING_START_KMH)
        / (HIGH_SPEED_STEERING_FULL_KMH - HIGH_SPEED_STEERING_START_KMH))
        .clamp(0.0, 1.0);
    let smooth = t * t * (3.0 - 2.0 * t);
    base_rate * (1.0 + (minimum_factor - 1.0) * smooth)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_towards_never_overshoots() {
        assert_eq!(move_towards(0.0, 1.0, 0.25), 0.25);
        assert_eq!(move_towards(0.9, 1.0, 0.25), 1.0);
        assert_eq!(move_towards(0.0, -1.0, 0.25), -0.25);
    }

    #[test]
    fn keyboard_input_is_not_binary_after_smoothing() {
        // One tick of a 60 Hz frame must not slam the throttle wide open.
        let after_one_frame = approach(
            0.0,
            1.0,
            THROTTLE_PRESS_RATE,
            THROTTLE_RELEASE_RATE,
            1.0 / 60.0,
        );
        assert!(
            after_one_frame > 0.0 && after_one_frame < 0.2,
            "{after_one_frame}"
        );
    }

    #[test]
    fn release_is_faster_than_press() {
        let press = approach(0.0, 1.0, THROTTLE_PRESS_RATE, THROTTLE_RELEASE_RATE, 0.1);
        let release = 1.0 - approach(1.0, 0.0, THROTTLE_PRESS_RATE, THROTTLE_RELEASE_RATE, 0.1);
        assert!(release > press);
    }

    #[test]
    fn steering_response_is_unchanged_through_200_kmh() {
        for speed in [0.0, 100.0, 199.9, 200.0] {
            assert_eq!(high_speed_steering_rate(3.6, speed, 0.35), 3.6);
        }
    }

    #[test]
    fn low_speed_keyboard_steering_reaches_lock_faster() {
        let base = 3.6;
        let parking =
            base * (1.0 + (LOW_SPEED_INPUT_RATE_FACTOR - 1.0) * low_speed_arcade_blend(0.0));
        let at_100 = base
            * (1.0 + (LOW_SPEED_INPUT_RATE_FACTOR - 1.0) * low_speed_arcade_blend(100.0 / 3.6));
        assert!((parking - base * LOW_SPEED_INPUT_RATE_FACTOR).abs() < 1.0e-6);
        assert_eq!(at_100, base);
    }

    #[test]
    fn steering_response_softens_progressively_above_200_kmh() {
        let at_200 = high_speed_steering_rate(3.6, 200.0, HIGH_SPEED_INPUT_RATE_FACTOR);
        let at_300 = high_speed_steering_rate(3.6, 300.0, HIGH_SPEED_INPUT_RATE_FACTOR);
        let at_400 = high_speed_steering_rate(3.6, 400.0, HIGH_SPEED_INPUT_RATE_FACTOR);

        assert!(at_200 > at_300 && at_300 > at_400);
        assert!((at_400 - 3.6 * HIGH_SPEED_INPUT_RATE_FACTOR).abs() < 1.0e-6);
    }

    #[test]
    fn high_speed_return_to_centre_stays_faster_than_steering_in() {
        let input = high_speed_steering_rate(3.6, 400.0, HIGH_SPEED_INPUT_RATE_FACTOR);
        let returning = high_speed_steering_rate(6.0, 400.0, HIGH_SPEED_RETURN_RATE_FACTOR);
        assert!(returning > input);
    }

    #[test]
    fn alternate_binding_is_used_for_driver_input() {
        let mut bindings = KeyBindings::default();
        bindings.set(
            ControlAction::Throttle,
            KeySlot::Alternate,
            Some(KeyCode::KeyT),
        );
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyT);
        assert!(bindings.pressed(&keys, ControlAction::Throttle));
    }

    #[test]
    fn default_driving_accepts_wasd_and_arrows() {
        let bindings = KeyBindings::default();
        for (action, keys) in [
            (ControlAction::Throttle, [KeyCode::KeyW, KeyCode::ArrowUp]),
            (ControlAction::Brake, [KeyCode::KeyS, KeyCode::ArrowDown]),
            (
                ControlAction::SteerLeft,
                [KeyCode::KeyA, KeyCode::ArrowLeft],
            ),
            (
                ControlAction::SteerRight,
                [KeyCode::KeyD, KeyCode::ArrowRight],
            ),
        ] {
            for key in keys {
                let mut input = ButtonInput::default();
                input.press(key);
                assert!(bindings.pressed(&input, action));
            }
        }
    }

    #[test]
    fn binding_conflicts_are_detected() {
        let bindings = KeyBindings::default();
        assert!(bindings.is_bound_to_other(ControlAction::Brake, KeyCode::KeyW));
        assert!(!bindings.is_bound_to_other(ControlAction::Throttle, KeyCode::KeyW));
    }

    #[test]
    fn triple_power_is_bound_to_f7_by_default() {
        let bindings = KeyBindings::default();
        assert_eq!(
            bindings.primary(ControlAction::ToggleTriplePower),
            KeyCode::F7
        );
    }

    #[test]
    fn rear_view_is_bound_to_b_and_abs_moves_to_f8() {
        let bindings = KeyBindings::default();
        assert_eq!(
            bindings.primary(ControlAction::ToggleRearView),
            KeyCode::KeyB
        );
        assert_eq!(bindings.primary(ControlAction::ToggleAbs), KeyCode::F8);
    }

    #[test]
    fn side_views_use_the_angle_bracket_keys() {
        let bindings = KeyBindings::default();
        assert_eq!(bindings.primary(ControlAction::ViewLeft), KeyCode::Comma);
        assert_eq!(bindings.primary(ControlAction::ViewRight), KeyCode::Period);
    }

    #[test]
    fn nitro_uses_both_shift_keys_by_default() {
        let bindings = KeyBindings::default();
        assert_eq!(bindings.primary(ControlAction::Nitro), KeyCode::ShiftLeft);
        assert_eq!(
            bindings.alternate(ControlAction::Nitro),
            Some(KeyCode::ShiftRight)
        );
    }

    #[test]
    fn radio_uses_nine_and_zero_and_both_actions_can_be_rebound() {
        let mut bindings = KeyBindings::default();
        assert_eq!(
            bindings.primary(ControlAction::PreviousRadioTrack),
            KeyCode::Digit9
        );
        assert_eq!(
            bindings.primary(ControlAction::NextRadioTrack),
            KeyCode::Digit0
        );
        bindings.set(
            ControlAction::PreviousRadioTrack,
            KeySlot::Primary,
            Some(KeyCode::KeyP),
        );
        bindings.set(
            ControlAction::NextRadioTrack,
            KeySlot::Alternate,
            Some(KeyCode::KeyN),
        );
        assert_eq!(
            bindings.primary(ControlAction::PreviousRadioTrack),
            KeyCode::KeyP
        );
        assert_eq!(
            bindings.alternate(ControlAction::NextRadioTrack),
            Some(KeyCode::KeyN)
        );
    }
}
