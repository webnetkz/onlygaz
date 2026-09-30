//! Seven speed dual clutch gearbox with an automatic shift strategy.
//!
//! The strategy is deliberately conservative: a shift may only happen when the
//! cooldown has expired, and a downshift is only allowed if the engine would
//! not be thrown past the limiter. Together those two rules stop the classic
//! two-gear hunting problem.

use super::config::TransmissionConfig;
use super::engine::{rad_to_rpm, rpm_to_rad};

/// What the driver has selected. Manual mode is not wired to a key yet, but the
/// solver already honours it, so adding a paddle is a controller change only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShiftMode {
    #[default]
    Automatic,
    Manual,
}

/// The signals the shift strategy looks at, grouped so the call site reads as
/// a description of the car's state rather than a list of bare floats.
#[derive(Debug, Clone, Copy, Default)]
pub struct ShiftContext {
    /// Crank speed [rad/s].
    pub engine_speed: f32,
    /// Wheel speed reflected to the gearbox output [rad/s].
    pub wheel_speed_ref: f32,
    /// Throttle pedal, 0..1.
    pub throttle: f32,
    /// Brake pedal, 0..1.
    pub brake: f32,
    /// Signed forward road speed [m/s].
    pub road_speed: f32,
}

/// Gearbox state.
#[derive(Debug, Clone, Copy)]
pub struct TransmissionState {
    /// 0 = neutral, negative = reverse, 1..=7 = forward gears.
    pub gear: i8,
    /// Remaining torque interruption of the shift in progress [s].
    pub shift_timer: f32,
    /// Time until another shift is permitted [s].
    pub cooldown: f32,
    pub mode: ShiftMode,
    /// Clutch engagement, 0 = fully open, 1 = fully locked.
    pub clutch: f32,
}

impl Default for TransmissionState {
    fn default() -> Self {
        Self {
            gear: 1,
            shift_timer: 0.0,
            cooldown: 0.0,
            mode: ShiftMode::Automatic,
            clutch: 1.0,
        }
    }
}

impl TransmissionState {
    /// True while the clutch is open for a gear change.
    #[inline]
    pub fn is_shifting(&self) -> bool {
        self.shift_timer > 0.0
    }

    #[inline]
    pub fn total_ratio(&self, cfg: &TransmissionConfig) -> f32 {
        cfg.total_ratio(self.gear)
    }

    /// Human readable gear for the HUD.
    pub fn display(&self) -> String {
        match self.gear {
            0 => "N".to_string(),
            g if g < 0 => "R".to_string(),
            g => g.to_string(),
        }
    }

    fn request(&mut self, cfg: &TransmissionConfig, gear: i8) {
        if gear == self.gear || self.is_shifting() || self.cooldown > 0.0 {
            return;
        }
        self.gear = gear;
        self.shift_timer = cfg.shift_time;
        self.cooldown = cfg.shift_cooldown;
    }

    pub fn shift_up(&mut self, cfg: &TransmissionConfig) {
        if self.gear < cfg.top_gear() {
            self.request(cfg, self.gear + 1);
        }
    }

    pub fn shift_down(&mut self, cfg: &TransmissionConfig) {
        if self.gear > 1 {
            self.request(cfg, self.gear - 1);
        }
    }

    /// Advance timers and, in [`ShiftMode::Automatic`], pick a gear.
    ///
    /// The predicted rpm after a candidate shift is computed from
    /// [`ShiftContext::wheel_speed_ref`], which is what stops the box from
    /// downshifting into the limiter or upshifting below idle.
    pub fn update(&mut self, cfg: &TransmissionConfig, ctx: &ShiftContext, dt: f32) {
        let ShiftContext {
            engine_speed,
            wheel_speed_ref,
            throttle,
            brake,
            road_speed,
        } = *ctx;

        self.shift_timer = (self.shift_timer - dt).max(0.0);
        self.cooldown = (self.cooldown - dt).max(0.0);

        // Clutch: open during a shift, slipping below the lock speed.
        self.clutch = if self.is_shifting() {
            0.0
        } else if road_speed.abs() >= cfg.clutch_lock_speed {
            1.0
        } else {
            (road_speed.abs() / cfg.clutch_lock_speed).clamp(0.0, 1.0)
        };

        if self.mode != ShiftMode::Automatic || self.gear <= 0 {
            return;
        }

        let rpm = rad_to_rpm(engine_speed);

        // Load-sensitive shift points: lift off and the box short-shifts.
        let load = throttle.clamp(0.0, 1.0);
        let upshift_rpm =
            cfg.downshift_rpm + (cfg.upshift_rpm - cfg.downshift_rpm) * (0.55 + 0.45 * load);

        if rpm >= upshift_rpm && self.gear < cfg.top_gear() {
            // Never upshift into a gear that would drop below idle.
            let next_ratio = cfg.total_ratio(self.gear + 1);
            let predicted = rad_to_rpm(wheel_speed_ref * next_ratio);
            if predicted > cfg.downshift_rpm * 0.55 {
                self.shift_up(cfg);
            }
            return;
        }

        if self.gear > 1 && (rpm <= cfg.downshift_rpm || brake > 0.35) {
            let lower_ratio = cfg.total_ratio(self.gear - 1);
            let predicted = rad_to_rpm(wheel_speed_ref * lower_ratio);
            // Only downshift if the engine stays comfortably below the limiter,
            // and only if it actually raises the revs.
            if predicted < cfg.upshift_rpm * 0.94 && predicted > rpm + 150.0 {
                self.shift_down(cfg);
            }
        }
    }

    /// Engine speed implied by the wheels for the current gear [rad/s].
    #[inline]
    pub fn driveline_engine_speed(&self, cfg: &TransmissionConfig, wheel_speed_ref: f32) -> f32 {
        wheel_speed_ref * cfg.total_ratio(self.gear)
    }

    /// Lowest crank speed the gearbox may impose, so the engine never stalls.
    #[inline]
    pub fn idle_floor(idle_rpm: f32) -> f32 {
        rpm_to_rad(idle_rpm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::presets::bugatti_veyron_super_sport;

    #[test]
    fn seven_forward_gears_with_falling_ratios() {
        let cfg = bugatti_veyron_super_sport();
        assert_eq!(cfg.transmission.gear_ratios.len(), 7);
        for pair in cfg.transmission.gear_ratios.windows(2) {
            assert!(pair[0] > pair[1], "ratios must fall: {pair:?}");
        }
    }

    #[test]
    fn upshifts_at_high_rpm_and_respects_cooldown() {
        let cfg = bugatti_veyron_super_sport();
        let mut tx = TransmissionState::default();
        let dt = 1.0 / 120.0;
        // Wheel speed consistent with ~6700 rpm in first gear.
        let engine_speed = rpm_to_rad(6700.0);
        let wheel_ref = engine_speed / cfg.transmission.total_ratio(1);
        let ctx = ShiftContext {
            engine_speed,
            wheel_speed_ref: wheel_ref,
            throttle: 1.0,
            brake: 0.0,
            road_speed: 25.0,
        };
        tx.update(&cfg.transmission, &ctx, dt);
        assert_eq!(tx.gear, 2, "should have upshifted");
        // Immediately after, another upshift must be blocked by the cooldown.
        tx.update(&cfg.transmission, &ctx, dt);
        assert_eq!(tx.gear, 2, "cooldown must prevent a second shift");
    }

    #[test]
    fn does_not_hunt_between_two_gears() {
        let cfg = bugatti_veyron_super_sport();
        let mut tx = TransmissionState::default();
        let dt = 1.0 / 120.0;
        // Hold a road speed that sits right between two shift points.
        let wheel_ref = rpm_to_rad(6600.0) / cfg.transmission.total_ratio(1);
        let mut shifts = 0;
        let mut previous = tx.gear;
        for _ in 0..(120 * 10) {
            let engine_speed = wheel_ref * tx.total_ratio(&cfg.transmission);
            tx.update(
                &cfg.transmission,
                &ShiftContext {
                    engine_speed,
                    wheel_speed_ref: wheel_ref,
                    throttle: 0.9,
                    brake: 0.0,
                    road_speed: 30.0,
                },
                dt,
            );
            if tx.gear != previous {
                shifts += 1;
                previous = tx.gear;
            }
        }
        assert!(shifts <= 2, "gearbox hunted, {shifts} shifts in 10 s");
    }
}
