//! Per-wheel state: ray-cast suspension, hub kinematics and the bookkeeping the
//! HUD and the debug gizmos read back.

use bevy::prelude::*;

use super::config::{SuspensionConfig, WheelConfig, WheelId};
use super::tire::{TireForces, TireState};

/// What the suspension ray found under a wheel.
#[derive(Debug, Clone, Copy)]
pub struct GroundHit {
    /// Distance from the ray origin to the surface [m].
    pub distance: f32,
    /// Surface normal, world space.
    pub normal: Vec3,
    /// Contact point, world space.
    pub point: Vec3,
}

/// Anything that can answer "what is under this point".
///
/// The game implementation forwards to Avian's `SpatialQuery`; the headless
/// test harness uses an analytic plane. Keeping the solver generic over this is
/// what makes the physics testable without a window.
pub trait GroundProbe {
    fn cast(&self, origin: Vec3, direction: Dir3, max_distance: f32) -> Option<GroundHit>;
}

/// A perfectly flat road at a given height - used by tests and as a fallback.
pub struct FlatGround {
    pub height: f32,
}

impl GroundProbe for FlatGround {
    fn cast(&self, origin: Vec3, direction: Dir3, max_distance: f32) -> Option<GroundHit> {
        let dy = direction.y;
        if dy >= -1.0e-6 {
            return None;
        }
        let distance = (self.height - origin.y) / dy;
        if distance < 0.0 || distance > max_distance {
            return None;
        }
        Some(GroundHit {
            distance,
            normal: Vec3::Y,
            point: origin + *direction * distance,
        })
    }
}

/// Mutable state of one corner.
#[derive(Debug, Clone, Copy)]
pub struct WheelState {
    pub id: WheelId,
    /// Hub speed [rad/s]. Positive means the car rolls forward.
    pub angular_velocity: f32,
    /// Accumulated hub angle, only used to spin the visual mesh [rad].
    pub spin_angle: f32,
    /// Current road-wheel steering angle [rad], positive steers right.
    pub steer_angle: f32,
    /// Distance from the strut anchor to the hub [m].
    pub suspension_length: f32,
    /// Rate of change of the suspension length, positive while extending [m/s].
    pub suspension_velocity: f32,
    /// Positive when the spring is compressed past its rest length [m].
    pub compression: f32,
    /// Vertical load carried by this wheel [N].
    pub normal_force: f32,
    pub grounded: bool,
    pub contact_point: Vec3,
    pub contact_normal: Vec3,
    /// Contact patch velocity along the wheel heading [m/s].
    pub longitudinal_velocity: f32,
    /// Contact patch velocity across the wheel [m/s].
    pub lateral_velocity: f32,
    pub tire: TireState,
    pub forces: TireForces,
    /// Drive torque delivered to this hub this tick [N·m].
    pub drive_torque: f32,
    /// Friction brake torque commanded this tick [N·m].
    pub brake_torque: f32,
    pub abs_active: bool,
    /// World-space force applied at the contact patch this tick [N].
    pub applied_force: Vec3,
}

impl WheelState {
    pub fn new(cfg: &WheelConfig) -> Self {
        Self {
            id: cfg.id,
            angular_velocity: 0.0,
            spin_angle: 0.0,
            steer_angle: 0.0,
            suspension_length: cfg.suspension.rest_length,
            suspension_velocity: 0.0,
            compression: 0.0,
            normal_force: 0.0,
            grounded: false,
            contact_point: Vec3::ZERO,
            contact_normal: Vec3::Y,
            longitudinal_velocity: 0.0,
            lateral_velocity: 0.0,
            tire: TireState::default(),
            forces: TireForces::default(),
            drive_torque: 0.0,
            brake_torque: 0.0,
            abs_active: false,
            applied_force: Vec3::ZERO,
        }
    }

    /// Put the wheel back to a clean standing state.
    pub fn reset(&mut self, cfg: &WheelConfig) {
        let id = self.id;
        *self = Self::new(cfg);
        self.id = id;
    }

    /// Wheel speed expressed as the road speed it corresponds to [m/s].
    #[inline]
    pub fn surface_speed(&self, radius: f32) -> f32 {
        self.angular_velocity * radius
    }

    /// Hub position in vehicle local space, given the strut anchor.
    #[inline]
    pub fn hub_local(&self, cfg: &WheelConfig) -> Vec3 {
        cfg.anchor - Vec3::Y * self.suspension_length
    }
}

/// Vertical spring + damper force for one corner [N].
///
/// `compression` is positive in bump. `velocity` is the compression *rate*,
/// positive while the spring is being compressed.
pub fn suspension_force(cfg: &SuspensionConfig, compression: f32, velocity: f32) -> f32 {
    // Progressive main spring.
    let spring = cfg.spring_stiffness * compression
        + cfg.progressive_stiffness * compression * compression * compression.signum();

    // Asymmetric damper - stiffer on rebound, as on any real car.
    let damper = if velocity >= 0.0 {
        cfg.damper_compression * velocity
    } else {
        cfg.damper_rebound * velocity
    };

    // Bump stops at both ends of the travel.
    let bump_stop = if compression > cfg.max_compression {
        cfg.bump_stop_stiffness * (compression - cfg.max_compression)
    } else if compression < -cfg.max_droop {
        cfg.bump_stop_stiffness * (compression + cfg.max_droop)
    } else {
        0.0
    };

    spring + damper + bump_stop
}

/// Anti-roll bar contribution for one wheel of an axle [N].
///
/// Positive result pushes the wheel down (adds load), which is what happens to
/// the outside wheel of a corner.
#[inline]
pub fn anti_roll_force(
    cfg: &SuspensionConfig,
    own_compression: f32,
    other_compression: f32,
) -> f32 {
    cfg.anti_roll_stiffness * (own_compression - other_compression)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::presets::bugatti_veyron_super_sport;

    #[test]
    fn spring_pushes_back_against_compression() {
        let cfg = bugatti_veyron_super_sport().wheels[0].suspension;
        assert!(suspension_force(&cfg, 0.05, 0.0) > 0.0);
        assert!(suspension_force(&cfg, -0.05, 0.0) < 0.0);
        assert_eq!(suspension_force(&cfg, 0.0, 0.0), 0.0);
    }

    #[test]
    fn static_sag_supports_the_corner_weight() {
        let car = bugatti_veyron_super_sport();
        let front = car.wheels[0].suspension;
        let corner_load = car.mass * 9.81 * car.static_front_weight_fraction() * 0.5;
        // Find the sag that balances the corner load.
        let mut sag = 0.0;
        while suspension_force(&front, sag, 0.0) < corner_load && sag < 0.3 {
            sag += 0.0005;
        }
        assert!(
            (0.03..0.09).contains(&sag),
            "front static sag is {:.3} m for {corner_load:.0} N",
            sag
        );
    }

    #[test]
    fn rebound_damping_is_stronger_than_bump() {
        let cfg = bugatti_veyron_super_sport().wheels[0].suspension;
        let bump = suspension_force(&cfg, 0.0, 1.0);
        let rebound = suspension_force(&cfg, 0.0, -1.0);
        assert!(rebound.abs() > bump.abs());
    }

    #[test]
    fn bump_stop_engages_past_the_travel_limit() {
        let cfg = bugatti_veyron_super_sport().wheels[0].suspension;
        let inside = suspension_force(&cfg, cfg.max_compression - 0.001, 0.0);
        let outside = suspension_force(&cfg, cfg.max_compression + 0.02, 0.0);
        assert!(outside > inside * 2.0, "{inside} {outside}");
    }

    #[test]
    fn flat_ground_probe_reports_the_expected_distance() {
        let ground = FlatGround { height: 0.0 };
        let hit = ground
            .cast(Vec3::new(0.0, 0.7, 0.0), Dir3::NEG_Y, 2.0)
            .expect("ray must hit");
        assert!((hit.distance - 0.7).abs() < 1.0e-5);
        assert!(
            ground
                .cast(Vec3::new(0.0, 5.0, 0.0), Dir3::NEG_Y, 2.0)
                .is_none()
        );
    }
}
