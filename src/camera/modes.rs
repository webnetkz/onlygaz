//! The six camera modes and their tuning.
//!
//! Positions are given in the *vehicle local* frame (-Z forward, +Y up, +X
//! right), so they read directly off the car's own geometry.

use bevy::prelude::*;

/// Degrees to radians.
const fn deg(d: f32) -> f32 {
    d * core::f32::consts::PI / 180.0
}

/// The six modes, cycled with `C`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect)]
pub enum CameraMode {
    /// Close chase camera - the whole car in frame, just behind it.
    #[default]
    ChaseClose,
    /// Higher, further back, wider field of view.
    ChaseFar,
    /// On the front bumper, a few centimetres off the tarmac.
    FrontBumper,
    /// Just above the bonnet, with the bonnet visible along the bottom edge.
    Hood,
    /// Driver's eye point inside the cockpit.
    Cockpit,
    /// Driver's eye aligned directly behind the steering wheel.
    SteeringWheel,
}

impl CameraMode {
    pub const ALL: [CameraMode; 6] = [
        CameraMode::ChaseClose,
        CameraMode::ChaseFar,
        CameraMode::FrontBumper,
        CameraMode::Hood,
        CameraMode::Cockpit,
        CameraMode::SteeringWheel,
    ];

    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            CameraMode::ChaseClose => "CHASE CLOSE",
            CameraMode::ChaseFar => "CHASE FAR",
            CameraMode::FrontBumper => "FRONT BUMPER",
            CameraMode::Hood => "HOOD",
            CameraMode::Cockpit => "COCKPIT",
            CameraMode::SteeringWheel => "STEERING WHEEL",
        }
    }

    /// Cameras mounted inside the passenger compartment.
    pub const fn is_interior(self) -> bool {
        matches!(self, Self::Cockpit | Self::SteeringWheel)
    }
}

/// Field of view as a function of speed. The brief asks for roughly
/// 65 degrees at a standstill rising to 80 degrees at 400 km/h.
#[derive(Debug, Clone, Reflect)]
pub struct FovCurve {
    /// `(speed [km/h], vertical field of view [rad])`.
    pub points: Vec<(f32, f32)>,
}

impl Default for FovCurve {
    fn default() -> Self {
        Self {
            points: vec![
                (0.0, deg(65.0)),
                (100.0, deg(68.0)),
                (200.0, deg(72.0)),
                (300.0, deg(76.0)),
                (400.0, deg(80.0)),
            ],
        }
    }
}

impl FovCurve {
    pub fn sample(&self, speed_kmh: f32) -> f32 {
        let speed = speed_kmh.abs();
        match self.points.as_slice() {
            [] => deg(65.0),
            points => {
                if speed <= points[0].0 {
                    return points[0].1;
                }
                if speed >= points[points.len() - 1].0 {
                    return points[points.len() - 1].1;
                }
                let index = points.partition_point(|p| p.0 <= speed).max(1);
                let (s0, f0) = points[index - 1];
                let (s1, f1) = points[index];
                let t = (speed - s0) / (s1 - s0).max(f32::EPSILON);
                f0 + (f1 - f0) * t
            }
        }
    }
}

/// Everything that distinguishes one camera mode from another.
#[derive(Debug, Clone, Reflect)]
pub struct CameraModeConfig {
    pub mode: CameraMode,
    /// Eye point in vehicle local space [m].
    pub eye: Vec3,
    /// Aim point in vehicle local space [m].
    pub target: Vec3,
    /// Position smoothing rate [1/s]. Higher is tighter.
    pub position_smoothing: f32,
    /// Aim smoothing rate [1/s].
    pub rotation_smoothing: f32,
    /// Chase cameras ignore the car's roll and pitch so the horizon stays level.
    pub yaw_only: bool,
    /// Scales the speed-dependent part of the field of view, 0 disables it.
    pub fov_speed_scale: f32,
}

/// The six modes, in cycling order.
pub fn default_modes() -> Vec<CameraModeConfig> {
    vec![
        CameraModeConfig {
            mode: CameraMode::ChaseClose,
            eye: Vec3::new(0.0, 1.55, 5.90),
            target: Vec3::new(0.0, 0.85, -4.0),
            position_smoothing: 7.5,
            rotation_smoothing: 9.0,
            yaw_only: true,
            fov_speed_scale: 1.0,
        },
        CameraModeConfig {
            mode: CameraMode::ChaseFar,
            eye: Vec3::new(0.0, 3.30, 11.0),
            target: Vec3::new(0.0, 0.80, -7.0),
            position_smoothing: 4.5,
            rotation_smoothing: 6.0,
            yaw_only: true,
            fov_speed_scale: 1.0,
        },
        CameraModeConfig {
            mode: CameraMode::FrontBumper,
            // The nose of the car sits at z = -2.22; this is just in front of
            // it and only 35 cm off the road, so the bodywork stays out of shot.
            eye: Vec3::new(0.0, 0.35, -2.30),
            target: Vec3::new(0.0, 0.42, -30.0),
            // Bolted to the car. Any smoothing at all makes an on-board camera
            // drift backwards under acceleration and swim over bumps, which is
            // exactly what an on-board camera must never do.
            position_smoothing: 0.0,
            rotation_smoothing: 0.0,
            yaw_only: false,
            fov_speed_scale: 0.0,
        },
        CameraModeConfig {
            mode: CameraMode::Hood,
            // Measured off the model's own centre-line profile: the bonnet is
            // 0.87 m high at z = -1.30 and the windscreen starts around
            // z = -1.00, so this sits 11 cm above the bonnet and well clear of
            // the glass, aimed so the bonnet fills the bottom of the frame.
            eye: Vec3::new(0.0, 0.98, -1.30),
            target: Vec3::new(0.0, 0.74, -26.0),
            position_smoothing: 0.0,
            rotation_smoothing: 0.0,
            yaw_only: false,
            fov_speed_scale: 0.0,
        },
        CameraModeConfig {
            mode: CameraMode::Cockpit,
            // Centred between the two seats, just above the centre console.
            // Keeping both eye and aim on x = 0 gives a symmetrical cockpit
            // view while the steering wheel remains visible to the left.
            eye: Vec3::new(0.0, 0.96, -0.10),
            target: Vec3::new(0.0, 0.91, -25.0),
            position_smoothing: 0.0,
            rotation_smoothing: 0.0,
            yaw_only: false,
            fov_speed_scale: 0.0,
        },
        CameraModeConfig {
            mode: CameraMode::SteeringWheel,
            // `follow_vehicle` replaces this generic eye with the selected
            // car's authored driver-eye point. A ten-degree downward aim keeps
            // its steering wheel and instrument cluster inside the lower part
            // of the frame without sacrificing the view through the screen.
            eye: Vec3::new(-0.34, 0.98, 0.08),
            target: Vec3::new(-0.34, -3.36, -24.54),
            position_smoothing: 0.0,
            rotation_smoothing: 0.0,
            yaw_only: false,
            fov_speed_scale: 0.0,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycling_visits_all_six_modes_and_returns() {
        let mut mode = CameraMode::default();
        let mut seen = vec![mode];
        for _ in 0..5 {
            mode = mode.next();
            seen.push(mode);
        }
        assert_eq!(seen.len(), 6);
        for candidate in CameraMode::ALL {
            assert!(seen.contains(&candidate), "{candidate:?} never selected");
        }
        assert_eq!(mode.next(), CameraMode::default());
    }

    #[test]
    fn there_are_exactly_six_configured_modes() {
        let modes = default_modes();
        assert_eq!(modes.len(), 6);
        for (config, expected) in modes.iter().zip(CameraMode::ALL) {
            assert_eq!(config.mode, expected);
        }
    }

    #[test]
    fn fov_grows_with_speed_but_stays_sane() {
        let curve = FovCurve::default();
        let still = curve.sample(0.0).to_degrees();
        let fast = curve.sample(400.0).to_degrees();
        assert!((64.0..66.0).contains(&still), "{still}");
        assert!((79.0..81.0).contains(&fast), "{fast}");
        assert!(curve.sample(150.0) > curve.sample(100.0));
        // Beyond the last point the curve is held, not extrapolated.
        assert_eq!(curve.sample(600.0), curve.sample(400.0));
    }

    #[test]
    fn on_board_cameras_are_rigidly_attached() {
        for config in default_modes() {
            if config.yaw_only {
                continue;
            }
            assert_eq!(
                (config.position_smoothing, config.rotation_smoothing),
                (0.0, 0.0),
                "{:?} must not smooth - an on-board camera has to be bolted on",
                config.mode
            );
            assert_eq!(
                config.fov_speed_scale, 0.0,
                "{:?} must not zoom with speed",
                config.mode
            );
        }
    }

    #[test]
    fn chase_cameras_are_behind_the_car_and_interior_ones_are_not() {
        let modes = default_modes();
        assert!(modes[0].eye.z > 3.0, "chase close must sit behind the car");
        assert!(modes[1].eye.z > modes[0].eye.z, "chase far must be further");
        assert!(modes[1].eye.y > modes[0].eye.y, "chase far must be higher");
        assert!(modes[2].eye.z < -2.0, "bumper camera must be at the nose");
        assert!(modes[2].eye.y < 0.5, "bumper camera must be low");
        assert!(
            modes[3].eye.z < -1.0,
            "the bonnet camera must be out on the bonnet, not inside the cabin"
        );
        assert_eq!(modes[4].eye.x, 0.0, "cockpit camera must be centred");
        assert_eq!(modes[4].target.x, 0.0, "cockpit view must aim centrally");
        assert!(
            modes[4].eye.y < modes[3].eye.y,
            "the cockpit view sits lower than the bonnet camera"
        );
        assert_eq!(modes[5].mode, CameraMode::SteeringWheel);
        assert!(
            modes[5].eye.x < 0.0,
            "wheel view must use the driver's side"
        );
        assert!(
            modes[5].target.y < modes[5].eye.y,
            "wheel view must keep the dashboard in frame"
        );
    }
}
