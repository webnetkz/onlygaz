//! Touch-control preferences of the phone build.
//!
//! The types are always compiled so the shared menu code can display and edit
//! them, but only the `mobile` build spawns those menu rows and stores the
//! values in `settings.ron`. The desktop game never inserts the resource.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// How the car is steered on a touch screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect, Serialize, Deserialize)]
pub enum SteeringMode {
    /// On-screen left / right buttons.
    #[default]
    Buttons,
    /// Roll the phone left / right like a steering wheel.
    Tilt,
}

impl SteeringMode {
    pub fn next(self) -> Self {
        match self {
            Self::Buttons => Self::Tilt,
            Self::Tilt => Self::Buttons,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Buttons => "BUTTONS",
            Self::Tilt => "TILT",
        }
    }
}

/// How far the phone has to be rolled for full steering lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect, Serialize, Deserialize)]
pub enum TiltSensitivity {
    Low,
    #[default]
    Medium,
    High,
}

impl TiltSensitivity {
    pub fn next(self) -> Self {
        match self {
            Self::Low => Self::Medium,
            Self::Medium => Self::High,
            Self::High => Self::Low,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Low => Self::High,
            Self::Medium => Self::Low,
            Self::High => Self::Medium,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
        }
    }

    /// Phone roll that produces full steering lock [degrees].
    pub fn full_lock_degrees(self) -> f32 {
        match self {
            Self::Low => 38.0,
            Self::Medium => 26.0,
            Self::High => 17.0,
        }
    }
}

/// Everything the touch overlay and tilt steering are configured with.
#[derive(Debug, Clone, PartialEq, Resource, Reflect, Serialize, Deserialize)]
#[reflect(Resource)]
#[serde(default)]
pub struct TouchControlSettings {
    pub steering: SteeringMode,
    pub tilt_sensitivity: TiltSensitivity,
    /// Phone roll that counts as "straight ahead" [degrees]. Set with the
    /// "Centre tilt" button for players who do not hold the phone level.
    pub tilt_centre_degrees: f32,
}

impl Default for TouchControlSettings {
    fn default() -> Self {
        Self {
            steering: SteeringMode::Buttons,
            tilt_sensitivity: TiltSensitivity::Medium,
            tilt_centre_degrees: 0.0,
        }
    }
}

/// Small dead zone so a phone held "almost level" drives straight [degrees].
pub const TILT_DEAD_ZONE_DEGREES: f32 = 1.5;

impl TouchControlSettings {
    /// Map a phone roll angle (degrees, positive = right edge down) to the
    /// -1..1 steering demand.
    pub fn steer_from_roll(&self, roll_degrees: f32) -> f32 {
        let relative = roll_degrees - self.tilt_centre_degrees;
        let magnitude = relative.abs();
        if magnitude <= TILT_DEAD_ZONE_DEGREES {
            return 0.0;
        }
        let full_lock = self.tilt_sensitivity.full_lock_degrees();
        let span = (full_lock - TILT_DEAD_ZONE_DEGREES).max(1.0);
        (relative.signum() * (magnitude - TILT_DEAD_ZONE_DEGREES) / span).clamp(-1.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_phone_drives_straight() {
        let settings = TouchControlSettings::default();
        assert_eq!(settings.steer_from_roll(0.0), 0.0);
        assert_eq!(settings.steer_from_roll(1.0), 0.0);
        assert_eq!(settings.steer_from_roll(-1.4), 0.0);
    }

    #[test]
    fn rolling_right_steers_right_and_saturates() {
        let settings = TouchControlSettings::default();
        let half = settings.steer_from_roll(13.0);
        assert!(half > 0.3 && half < 0.6, "{half}");
        assert_eq!(settings.steer_from_roll(60.0), 1.0);
        assert_eq!(settings.steer_from_roll(-60.0), -1.0);
    }

    #[test]
    fn higher_sensitivity_needs_less_roll() {
        let mut settings = TouchControlSettings {
            tilt_sensitivity: TiltSensitivity::Low,
            ..Default::default()
        };
        let low = settings.steer_from_roll(10.0);
        settings.tilt_sensitivity = TiltSensitivity::High;
        let high = settings.steer_from_roll(10.0);
        assert!(high > low);
    }

    #[test]
    fn centring_moves_the_neutral_point() {
        let settings = TouchControlSettings {
            tilt_centre_degrees: 8.0,
            ..Default::default()
        };
        assert_eq!(settings.steer_from_roll(8.0), 0.0);
        assert!(settings.steer_from_roll(0.0) < 0.0);
    }

    #[test]
    fn modes_cycle_and_survive_serialisation() {
        let mut settings = TouchControlSettings::default();
        settings.steering = settings.steering.next();
        settings.tilt_sensitivity = settings.tilt_sensitivity.previous();
        let text = ron::to_string(&settings).unwrap();
        let loaded: TouchControlSettings = ron::from_str(&text).unwrap();
        assert_eq!(loaded, settings);
        assert_eq!(loaded.steering, SteeringMode::Tilt);
        assert_eq!(loaded.tilt_sensitivity, TiltSensitivity::Low);
        let empty: TouchControlSettings = ron::from_str("()").unwrap();
        assert_eq!(empty, TouchControlSettings::default());
    }
}
