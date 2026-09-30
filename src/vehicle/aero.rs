//! Aerodynamics: drag and downforce.
//!
//! Both follow the usual quadratic law `F = 0.5 * rho * C * A * v^2`. Drag acts
//! against the velocity vector at the aerodynamic centre; downforce acts along
//! the car's own down axis and is split between two application points so that
//! it also produces the correct pitch moment - which is what makes the car feel
//! progressively planted between 200 and 400+ km/h.

use bevy::prelude::*;

use super::config::AeroConfig;

/// One aerodynamic force and the vehicle-local point it acts at.
#[derive(Debug, Clone, Copy)]
pub struct AeroForce {
    /// World-space force [N].
    pub force: Vec3,
    /// Vehicle-local application point [m].
    pub local_point: Vec3,
}

/// Result of an aerodynamic evaluation, for both physics and telemetry.
#[derive(Debug, Clone, Copy, Default)]
pub struct AeroOutput {
    pub drag_force: f32,
    pub downforce_front: f32,
    pub downforce_rear: f32,
}

impl AeroOutput {
    #[inline]
    pub fn total_downforce(&self) -> f32 {
        self.downforce_front + self.downforce_rear
    }
}

/// Compute the three aerodynamic forces.
///
/// * `velocity` - world-space velocity of the centre of mass [m/s].
/// * `up` - the car's own up axis in world space.
pub fn evaluate(cfg: &AeroConfig, velocity: Vec3, up: Vec3) -> (AeroOutput, [AeroForce; 3]) {
    let speed = velocity.length();
    let drag_magnitude = cfg.drag_force(speed);
    let drag_direction = if speed > 1.0e-3 {
        -velocity / speed
    } else {
        Vec3::ZERO
    };

    let front = cfg.downforce_front(speed);
    let rear = cfg.downforce_rear(speed);
    let down = -up;

    let output = AeroOutput {
        drag_force: drag_magnitude,
        downforce_front: front,
        downforce_rear: rear,
    };

    let forces = [
        AeroForce {
            force: drag_direction * drag_magnitude,
            local_point: cfg.drag_center,
        },
        AeroForce {
            force: down * front,
            local_point: cfg.downforce_center_front,
        },
        AeroForce {
            force: down * rear,
            local_point: cfg.downforce_center_rear,
        },
    ];

    (output, forces)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::presets::bugatti_veyron_super_sport;

    #[test]
    fn drag_grows_with_the_square_of_speed() {
        let cfg = bugatti_veyron_super_sport().aero;
        let a = cfg.drag_force(50.0);
        let b = cfg.drag_force(100.0);
        assert!((b / a - 4.0).abs() < 1.0e-3, "{a} {b}");
    }

    #[test]
    fn downforce_is_meaningful_at_high_speed() {
        let cfg = bugatti_veyron_super_sport().aero;
        // At 300 km/h the car should be pressed down by a couple of tonnes-force
        // worth of newtons but nowhere near its own weight in the top speed set-up.
        let total = cfg.downforce_front(300.0 / 3.6) + cfg.downforce_rear(300.0 / 3.6);
        assert!(
            (1800.0..4200.0).contains(&total),
            "downforce at 300 km/h is {total:.0} N"
        );
        let at_100 = cfg.downforce_front(100.0 / 3.6) + cfg.downforce_rear(100.0 / 3.6);
        assert!(at_100 < total * 0.2);
    }

    #[test]
    fn drag_opposes_motion() {
        let cfg = bugatti_veyron_super_sport().aero;
        let (_, forces) = evaluate(&cfg, Vec3::new(0.0, 0.0, -80.0), Vec3::Y);
        assert!(forces[0].force.z > 0.0, "drag must push backwards");
        assert!(forces[1].force.y < 0.0, "downforce must push down");
    }
}
