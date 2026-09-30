//! Friction brakes, handbrake and a simple slip-controlled ABS.

use super::config::BrakeConfig;
use super::config::WheelConfig;

/// Per-wheel brake command produced by [`compute`].
#[derive(Debug, Clone, Copy, Default)]
pub struct BrakeCommand {
    /// Magnitude of the clamping torque [N·m]; always positive.
    pub torque: f32,
    /// True while ABS is bleeding pressure off this wheel.
    pub abs_active: bool,
}

/// Brake torque for one wheel.
///
/// * `brake` / `handbrake` - 0..1 pedal and lever positions.
/// * `slip_ratio` - the tyre's current longitudinal slip; a locking wheel has a
///   slip ratio approaching -1.
pub fn compute(
    cfg: &BrakeConfig,
    wheel: &WheelConfig,
    brake: f32,
    handbrake: f32,
    slip_ratio: f32,
    abs_enabled: bool,
) -> BrakeCommand {
    let service = wheel.brake_torque * brake.clamp(0.0, 1.0);
    let park = wheel.handbrake_torque * handbrake.clamp(0.0, 1.0);

    // ABS only modulates the service brake; the handbrake is meant to lock.
    let mut abs_active = false;
    let service = if abs_enabled && cfg.abs_enabled && service > 0.0 {
        let excess = -slip_ratio - cfg.abs_slip_target;
        if excess > 0.0 {
            abs_active = true;
            let release = (excess * cfg.abs_gain).clamp(0.0, 1.0);
            service * (1.0 - release)
        } else {
            service
        }
    } else {
        service
    };

    BrakeCommand {
        torque: service + park,
        abs_active,
    }
}

/// Apply a brake torque to a spinning wheel without letting it reverse
/// direction inside one step - integrating `T = -sign(w) * T_brake` explicitly
/// makes a nearly stopped wheel oscillate, so the torque is capped at exactly
/// the amount needed to bring the wheel to a stop.
///
/// Returns the signed torque to apply [N·m].
#[inline]
pub fn limited_brake_torque(
    brake_torque: f32,
    wheel_speed: f32,
    inertia: f32,
    dt: f32,
    other_torque: f32,
) -> f32 {
    if brake_torque <= 0.0 {
        return 0.0;
    }
    // Torque that would bring the wheel to a standstill exactly at the end of
    // the step, given everything else acting on it.
    let stopping = (inertia * wheel_speed / dt.max(1.0e-6) + other_torque).abs();
    let magnitude = brake_torque.min(stopping);
    -wheel_speed.signum() * magnitude
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::presets::bugatti_veyron_super_sport;

    #[test]
    fn abs_releases_pressure_when_the_wheel_locks() {
        let cfg = bugatti_veyron_super_sport();
        let wheel = cfg.wheels[0];
        let normal = compute(&cfg.brakes, &wheel, 1.0, 0.0, -0.05, true);
        let locking = compute(&cfg.brakes, &wheel, 1.0, 0.0, -0.6, true);
        assert!(!normal.abs_active);
        assert!(locking.abs_active);
        assert!(locking.torque < normal.torque * 0.5);
    }

    #[test]
    fn abs_off_lets_the_wheel_lock() {
        let cfg = bugatti_veyron_super_sport();
        let wheel = cfg.wheels[0];
        let locking = compute(&cfg.brakes, &wheel, 1.0, 0.0, -0.9, false);
        assert!((locking.torque - wheel.brake_torque).abs() < 1.0e-3);
        assert!(!locking.abs_active);
    }

    #[test]
    fn handbrake_only_affects_the_rear() {
        let cfg = bugatti_veyron_super_sport();
        let front = compute(&cfg.brakes, &cfg.wheels[0], 0.0, 1.0, 0.0, true);
        let rear = compute(&cfg.brakes, &cfg.wheels[2], 0.0, 1.0, 0.0, true);
        assert_eq!(front.torque, 0.0);
        assert!(rear.torque > 0.0);
    }

    #[test]
    fn brake_torque_never_reverses_a_wheel() {
        // A wheel barely turning must be stopped, not spun backwards.
        let torque = limited_brake_torque(10_000.0, 0.01, 2.0, 1.0 / 120.0, 0.0);
        assert!(torque.abs() <= 2.0 * 0.01 * 120.0 + 1.0e-6);
        assert!(torque < 0.0);
    }
}
