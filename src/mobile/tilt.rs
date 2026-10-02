//! Tilt steering: turn the phone like a steering wheel.
//!
//! The phone is held sideways (landscape) in both hands. Rolling it
//! clockwise — right edge down — steers right. The roll angle is measured from
//! the gravity vector in the axes of the *screen* (the Android layer converts
//! the sensor's device axes using the current display rotation, so turning the
//! phone over by 180° keeps working), and it does not change when the phone
//! is tipped towards or away from the player.

use bevy::prelude::*;

use crate::game::GameState;
use crate::mobile::{SteeringMode, TouchControlSettings};
use crate::ui::menu::{PauseMenuPage, TiltCentreButton, TiltStatusText};

/// Latest tilt reading, refreshed every frame while tilt steering is used.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct TiltInput {
    /// A motion sensor is delivering data.
    pub available: bool,
    /// Smoothed roll of the phone [degrees], positive = right edge down.
    pub roll_degrees: f32,
    /// Steering demand from the roll and the player's settings, -1..1.
    pub steer: f32,
}

/// Smoothing rate of the roll angle [1/s]; removes hand tremor and sensor
/// noise while keeping the response well under a tenth of a second.
const ROLL_SMOOTHING: f32 = 18.0;

pub struct TiltPlugin;

impl Plugin for TiltPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TiltInput>()
            .add_systems(PreUpdate, update_tilt)
            // Menu buttons report `Pressed` to Update systems (see gestures).
            .add_systems(Update, (centre_tilt_button, show_tilt_status).chain());
    }
}

/// Roll angle [degrees] of the phone from the accelerometer / gravity reading
/// in screen axes (x to the right of the picture, y up, z out of the screen).
/// `None` when the reading is unusable (free fall, bad data).
pub fn roll_from_gravity(gravity: Vec3) -> Option<f32> {
    if !gravity.is_finite() || gravity.length() < 3.0 {
        return None;
    }
    let lateral = -gravity.x;
    let rest = (gravity.y * gravity.y + gravity.z * gravity.z).sqrt();
    Some(lateral.atan2(rest).to_degrees())
}

/// Convert a sensor reading from the device's natural axes into screen axes
/// for an Android display `rotation` (`Display.getRotation()`: 0..3 for 0°,
/// 90°, 180° and 270°).
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(super) fn device_to_screen(reading: Vec3, rotation: u32) -> Vec3 {
    match rotation & 3 {
        0 => reading,
        1 => Vec3::new(-reading.y, reading.x, reading.z),
        2 => Vec3::new(-reading.x, -reading.y, reading.z),
        _ => Vec3::new(reading.y, -reading.x, reading.z),
    }
}

/// Whether the sensor should be running: only when it is actually used.
fn sensor_wanted(settings: &TouchControlSettings, state: GameState, page: PauseMenuPage) -> bool {
    settings.steering == SteeringMode::Tilt
        && (state == GameState::Driving || page == PauseMenuPage::Controls)
}

fn update_tilt(
    time: Res<Time>,
    settings: Res<TouchControlSettings>,
    state: Res<State<GameState>>,
    page: Res<PauseMenuPage>,
    mut tilt: ResMut<TiltInput>,
) {
    let wanted = sensor_wanted(&settings, *state.get(), *page);
    let sample = platform_gravity(wanted);
    let mut next = *tilt;
    match sample.and_then(roll_from_gravity) {
        Some(roll) => {
            let blend = if next.available {
                1.0 - (-ROLL_SMOOTHING * time.delta_secs()).exp()
            } else {
                1.0
            };
            next.available = true;
            next.roll_degrees += (roll - next.roll_degrees) * blend;
            next.steer = settings.steer_from_roll(next.roll_degrees);
        }
        None => {
            next.available = false;
            next.steer = 0.0;
        }
    }
    if *tilt != next {
        *tilt = next;
    }
}

#[cfg(target_os = "android")]
fn platform_gravity(wanted: bool) -> Option<Vec3> {
    super::android::gravity(wanted)
}

/// Desktop preview: there is no motion sensor, so tilt mode falls back to
/// the on-screen arrows and the keyboard. For testing the tilt path,
/// `ONLYGAZ_FAKE_TILT_DEGREES=<roll>` simulates a phone held at that roll.
#[cfg(not(target_os = "android"))]
fn platform_gravity(wanted: bool) -> Option<Vec3> {
    if !wanted {
        return None;
    }
    let roll: f32 = std::env::var("ONLYGAZ_FAKE_TILT_DEGREES")
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let roll = roll.to_radians();
    Some(Vec3::new(-9.81 * roll.sin(), 9.81 * roll.cos(), 0.0))
}

/// "Straight ahead: SET CURRENT" stores the present roll as neutral.
fn centre_tilt_button(
    tilt: Res<TiltInput>,
    mut settings: ResMut<TouchControlSettings>,
    buttons: Query<&Interaction, (Changed<Interaction>, With<TiltCentreButton>)>,
) {
    for interaction in &buttons {
        if *interaction == Interaction::Pressed && tilt.available {
            settings.tilt_centre_degrees = (tilt.roll_degrees * 10.0).round() / 10.0;
            info!("tilt centre set to {:.1}°", settings.tilt_centre_degrees);
        }
    }
}

fn show_tilt_status(
    tilt: Res<TiltInput>,
    settings: Res<TouchControlSettings>,
    mut texts: Query<&mut Text, With<TiltStatusText>>,
) {
    if !tilt.is_changed() && !settings.is_changed() {
        return;
    }
    let value = if settings.steering != SteeringMode::Tilt {
        "Tilt steering is off — the arrow buttons steer.".to_string()
    } else if !tilt.available {
        "Tilt sensor: no data yet (or not present) — arrows stay on screen.".to_string()
    } else {
        format!(
            "Phone roll {:+.0}°   straight ahead {:+.0}°   steering {:+.0}%",
            tilt.roll_degrees,
            settings.tilt_centre_degrees,
            tilt.steer * 100.0
        )
    };
    for mut text in &mut texts {
        if text.as_str() != value {
            **text = value.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: f32 = 9.81;

    #[test]
    fn upright_phone_has_no_roll() {
        assert!(roll_from_gravity(Vec3::new(0.0, G, 0.0)).unwrap().abs() < 1.0e-4);
    }

    #[test]
    fn right_edge_down_is_positive_roll() {
        // Rotating the phone clockwise by 20° moves gravity's reaction onto
        // the negative device x axis.
        let angle = 20.0_f32.to_radians();
        let reading = Vec3::new(-G * angle.sin(), G * angle.cos(), 0.0);
        let roll = roll_from_gravity(reading).unwrap();
        assert!((roll - 20.0).abs() < 0.01, "{roll}");
    }

    #[test]
    fn tipping_the_phone_back_does_not_steer() {
        // Phone leaned back by 50° (screen facing up) without any roll.
        let pitch = 50.0_f32.to_radians();
        let reading = Vec3::new(0.0, G * pitch.cos(), G * pitch.sin());
        assert!(roll_from_gravity(reading).unwrap().abs() < 1.0e-4);
        // Same lean with 10° roll still reads 10°.
        let roll = 10.0_f32.to_radians();
        let leaned = Vec3::new(
            -G * roll.sin(),
            G * roll.cos() * pitch.cos(),
            G * roll.cos() * pitch.sin(),
        );
        assert!((roll_from_gravity(leaned).unwrap() - 10.0).abs() < 0.01);
    }

    /// Device-axis reading of a phone in landscape at display `rotation`,
    /// rolled clockwise (right edge of the picture down) by `roll` degrees.
    fn landscape_reading(rotation: u32, roll: f32) -> Vec3 {
        let roll = roll.to_radians();
        // In screen axes the reaction to gravity points up the picture.
        let screen = Vec3::new(-G * roll.sin(), G * roll.cos(), 0.0);
        match rotation {
            // Device turned anticlockwise: screen x = -device y, y = device x.
            1 => Vec3::new(screen.y, -screen.x, 0.0),
            // Device turned clockwise: screen x = device y, y = -device x.
            3 => Vec3::new(-screen.y, screen.x, 0.0),
            _ => unreachable!(),
        }
    }

    #[test]
    fn landscape_roll_is_measured_in_screen_axes_for_both_rotations() {
        for rotation in [1, 3] {
            for roll in [-25.0, 0.0, 12.0] {
                let screen = device_to_screen(landscape_reading(rotation, roll), rotation);
                let measured = roll_from_gravity(screen).unwrap();
                assert!(
                    (measured - roll).abs() < 0.01,
                    "rotation {rotation}: {measured} != {roll}"
                );
            }
        }
        // The natural orientation (rotation 0) passes through unchanged.
        let reading = Vec3::new(1.0, 9.0, 2.0);
        assert_eq!(device_to_screen(reading, 0), reading);
        assert_eq!(device_to_screen(reading, 2), Vec3::new(-1.0, -9.0, 2.0));
    }

    #[test]
    fn free_fall_is_rejected() {
        assert!(roll_from_gravity(Vec3::new(0.1, 0.2, 0.1)).is_none());
        assert!(roll_from_gravity(Vec3::new(f32::NAN, G, 0.0)).is_none());
    }

    #[test]
    fn sensor_runs_only_for_tilt_steering() {
        let mut settings = TouchControlSettings::default();
        assert!(!sensor_wanted(
            &settings,
            GameState::Driving,
            PauseMenuPage::Main
        ));
        settings.steering = SteeringMode::Tilt;
        assert!(sensor_wanted(
            &settings,
            GameState::Driving,
            PauseMenuPage::Main
        ));
        assert!(sensor_wanted(
            &settings,
            GameState::Paused,
            PauseMenuPage::Controls
        ));
        assert!(!sensor_wanted(
            &settings,
            GameState::MainMenu,
            PauseMenuPage::Main
        ));
    }
}
