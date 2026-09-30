//! Drivetrain: how crank torque reaches the four contact patches.
//!
//! ```text
//!   engine -> clutch -> gearbox -> centre differential -> axle differentials -> wheels
//! ```
//!
//! The differentials are modelled as a viscous coupling: an ideal open
//! differential splits torque evenly, and the locking fraction transfers torque
//! from the faster wheel to the slower one in proportion to their speed
//! difference. That is enough to keep a wheel that has lost load from soaking up
//! all of the torque, without the numerical grief of a hard locker.

use super::config::{Drivetrain, VehicleConfig, WheelId};

/// Viscous coupling gain per unit of `differential_locking` [N·m per rad/s].
const DIFFERENTIAL_LOCK_GAIN: f32 = 30.0;

/// Torque share of each wheel, in [`WheelId`] order. Sums to 1 for any driven
/// layout, or to 0 if nothing is driven.
pub fn torque_weights(cfg: &VehicleConfig) -> [f32; 4] {
    let (front, rear) = match cfg.drivetrain {
        Drivetrain::Fwd => (1.0, 0.0),
        Drivetrain::Rwd => (0.0, 1.0),
        Drivetrain::Awd => {
            let f = cfg.front_torque_bias.clamp(0.0, 1.0);
            (f, 1.0 - f)
        }
    };
    [front * 0.5, front * 0.5, rear * 0.5, rear * 0.5]
}

/// Reference wheel speed seen by the gearbox output [rad/s].
///
/// It is the torque-weighted mean of the driven wheels, which is exactly the
/// speed a locked driveline would impose on the engine.
pub fn reference_wheel_speed(weights: &[f32; 4], wheel_speeds: &[f32; 4]) -> f32 {
    let total: f32 = weights.iter().sum();
    if total <= 0.0 {
        return 0.0;
    }
    weights
        .iter()
        .zip(wheel_speeds)
        .map(|(w, s)| w * s)
        .sum::<f32>()
        / total
}

/// Rotational inertia of the engine reflected through the gearbox onto each
/// wheel [kg·m²]. This is why a car cannot spin its wheels instantly in first
/// gear: the crank has to be accelerated too.
pub fn reflected_inertia(cfg: &VehicleConfig, weights: &[f32; 4], total_ratio: f32) -> [f32; 4] {
    let reflected = cfg.engine.inertia * total_ratio * total_ratio;
    [
        weights[0] * reflected,
        weights[1] * reflected,
        weights[2] * reflected,
        weights[3] * reflected,
    ]
}

/// Split the gearbox output torque across the four wheels.
///
/// * `axle_torque` - torque at the differential input, i.e. crank torque times
///   the total ratio times the driveline efficiency [N·m].
pub fn distribute(
    cfg: &VehicleConfig,
    weights: &[f32; 4],
    axle_torque: f32,
    wheel_speeds: &[f32; 4],
) -> [f32; 4] {
    let mut out = [0.0f32; 4];
    for (i, w) in weights.iter().enumerate() {
        out[i] = axle_torque * w;
    }

    let lock = cfg.differential_locking.clamp(0.0, 1.0) * DIFFERENTIAL_LOCK_GAIN;
    if lock > 0.0 {
        for (left, right) in [
            (WheelId::FrontLeft.index(), WheelId::FrontRight.index()),
            (WheelId::RearLeft.index(), WheelId::RearRight.index()),
        ] {
            let axle_share = weights[left] + weights[right];
            if axle_share <= 0.0 || weights[left] <= 0.0 || weights[right] <= 0.0 {
                continue;
            }
            // Cap the transfer at the torque the axle actually carries so the
            // coupling can never invent net torque.
            let cap = (axle_torque * axle_share).abs().max(200.0);
            let transfer = (lock * (wheel_speeds[left] - wheel_speeds[right])).clamp(-cap, cap);
            out[left] -= transfer;
            out[right] += transfer;
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::presets::bugatti_veyron_super_sport;

    #[test]
    fn awd_split_matches_the_configured_bias() {
        let cfg = bugatti_veyron_super_sport();
        let w = torque_weights(&cfg);
        let front = w[0] + w[1];
        let rear = w[2] + w[3];
        assert!((front - cfg.front_torque_bias).abs() < 1.0e-6);
        assert!((front + rear - 1.0).abs() < 1.0e-6);
        assert!(rear > front, "the Veyron is rear biased");
    }

    #[test]
    fn all_four_wheels_are_driven() {
        let cfg = bugatti_veyron_super_sport();
        let w = torque_weights(&cfg);
        assert!(w.iter().all(|x| *x > 0.0), "{w:?}");
    }

    #[test]
    fn differential_moves_torque_to_the_slower_wheel() {
        let cfg = bugatti_veyron_super_sport();
        let weights = torque_weights(&cfg);
        // Rear left is spinning up, rear right has grip.
        let speeds = [10.0, 10.0, 60.0, 10.0];
        let torques = distribute(&cfg, &weights, 4000.0, &speeds);
        assert!(
            torques[WheelId::RearRight.index()] > torques[WheelId::RearLeft.index()],
            "{torques:?}"
        );
        // Total torque is conserved.
        let total: f32 = torques.iter().sum();
        assert!((total - 4000.0).abs() < 1.0, "{total}");
    }

    #[test]
    fn reflected_inertia_is_largest_in_first_gear() {
        let cfg = bugatti_veyron_super_sport();
        let weights = torque_weights(&cfg);
        let first = reflected_inertia(&cfg, &weights, cfg.transmission.total_ratio(1));
        let seventh = reflected_inertia(&cfg, &weights, cfg.transmission.total_ratio(7));
        assert!(first[2] > seventh[2] * 10.0, "{first:?} {seventh:?}");
    }
}
