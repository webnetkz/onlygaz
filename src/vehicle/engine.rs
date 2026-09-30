//! Engine model: a torque curve plus an rpm limiter, idle control and engine
//! braking. No Bevy types, so it can be unit tested on its own.

use super::config::EngineConfig;

/// Proportional gain of the idle controller, in units of full control torque
/// per unit of relative rpm error. High enough that idle settles within a few
/// tens of rpm of the target.
const IDLE_CONTROL_GAIN: f32 = 12.0;
/// Safety ceiling used only when the gameplay power mode removes the ECU cut.
/// It is far above the usable range and is not a vehicle speed limiter.
const UNLIMITED_RPM_SAFETY_CEILING: f32 = 20_000.0;

/// Convert rpm to rad/s.
#[inline]
pub fn rpm_to_rad(rpm: f32) -> f32 {
    rpm * core::f32::consts::TAU / 60.0
}

/// Convert rad/s to rpm.
#[inline]
pub fn rad_to_rpm(rad: f32) -> f32 {
    rad * 60.0 / core::f32::consts::TAU
}

/// Mutable engine state.
#[derive(Debug, Clone, Copy)]
pub struct EngineState {
    /// Crankshaft speed [rad/s].
    pub angular_velocity: f32,
    /// Latched limiter flag - the ECU keeps fuel cut until the rpm has dropped
    /// by `limiter_hysteresis_rpm`, otherwise the engine chatters on the limit.
    pub limiter_active: bool,
    /// Whether the normal ECU rev limiter is enabled.
    pub rev_limiter_enabled: bool,
}

impl EngineState {
    pub fn new(cfg: &EngineConfig) -> Self {
        Self {
            angular_velocity: rpm_to_rad(cfg.idle_rpm),
            limiter_active: false,
            rev_limiter_enabled: true,
        }
    }

    #[inline]
    pub fn rpm(&self) -> f32 {
        rad_to_rpm(self.angular_velocity)
    }

    /// Net crank torque for the current state.
    ///
    /// * `throttle` - 0..1 pedal position.
    /// * The result already contains pumping/friction losses (engine braking)
    ///   and the idle controller, and is zero-fuel while the limiter is latched.
    pub fn torque(&mut self, cfg: &EngineConfig, throttle: f32) -> f32 {
        self.torque_with_power_multiplier(cfg, throttle, 1.0)
    }

    /// Net crank torque with a multiplier applied only to combustion torque.
    ///
    /// Keeping idle control and engine braking outside the multiplier means a
    /// power-up changes acceleration and maximum speed without making the car
    /// surge at idle or engine-brake three times harder when the pedal closes.
    pub fn torque_with_power_multiplier(
        &mut self,
        cfg: &EngineConfig,
        throttle: f32,
        power_multiplier: f32,
    ) -> f32 {
        let rpm = self.rpm();

        // Rev limiter with hysteresis. F7 disables the ECU cut so the car can
        // continue pulling in top gear instead of hitting an artificial Vmax.
        if self.rev_limiter_enabled {
            if rpm >= cfg.limiter_rpm {
                self.limiter_active = true;
            } else if rpm < cfg.limiter_rpm - cfg.limiter_hysteresis_rpm {
                self.limiter_active = false;
            }
        } else {
            self.limiter_active = false;
        }

        let throttle = throttle.clamp(0.0, 1.0);
        let fuelling = if self.limiter_active { 0.0 } else { throttle };
        let drive = cfg.torque_curve.torque_at(rpm) * fuelling * power_multiplier.max(0.0);

        // Engine braking grows with speed, and is scaled by how far the
        // throttle is *closed*. Published torque curves are already net of
        // internal friction at full throttle, so subtracting the full pumping
        // loss again would quietly cost the car about 200 N·m at the top end.
        let braking = (cfg.engine_brake_constant
            + cfg.engine_brake_per_rad * self.angular_velocity)
            * (1.0 - fuelling);

        // Idle controller: a soft PI-free proportional term that only ever adds
        // torque, and only near or below the idle speed.
        let idle_error = (cfg.idle_rpm - rpm) / cfg.idle_rpm.max(1.0);
        let idle = if idle_error > 0.0 && !self.limiter_active {
            (idle_error * IDLE_CONTROL_GAIN).clamp(0.0, 1.0) * cfg.idle_control_torque
        } else {
            0.0
        };

        drive + idle - braking
    }

    /// Integrate the free (declutched) engine.
    pub fn integrate_free(&mut self, cfg: &EngineConfig, torque: f32, dt: f32) {
        let inertia = cfg.inertia.max(1.0e-4);
        self.angular_velocity += torque / inertia * dt;
        self.clamp(cfg);
    }

    /// Force the crank onto the speed dictated by the wheels (locked clutch).
    pub fn set_from_driveline(&mut self, cfg: &EngineConfig, driveline_speed: f32) {
        self.angular_velocity = driveline_speed;
        self.clamp(cfg);
    }

    /// Keep the crank inside its physical range.
    pub fn clamp(&mut self, cfg: &EngineConfig) {
        let min = rpm_to_rad(cfg.idle_rpm * 0.6);
        let max_rpm = if self.rev_limiter_enabled {
            cfg.limiter_rpm + 400.0
        } else {
            UNLIMITED_RPM_SAFETY_CEILING
        };
        let max = rpm_to_rad(max_rpm);
        self.angular_velocity = self.angular_velocity.clamp(min, max);
    }

    /// Instantaneous shaft power [W].
    #[inline]
    pub fn power(&self, torque: f32) -> f32 {
        torque * self.angular_velocity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::presets::bugatti_veyron_super_sport;

    #[test]
    fn torque_curve_is_interpolated_not_constant() {
        let cfg = bugatti_veyron_super_sport();
        let curve = &cfg.engine.torque_curve;
        let low = curve.torque_at(1200.0);
        let mid = curve.torque_at(4000.0);
        let high = curve.torque_at(6800.0);
        assert!(low < mid, "{low} !< {mid}");
        assert!(high < mid, "{high} !< {mid}");
        // Between two samples the value must actually interpolate.
        let a = curve.torque_at(1000.0);
        let b = curve.torque_at(1500.0);
        let m = curve.torque_at(1250.0);
        assert!(m > a && m < b, "{a} {m} {b}");
    }

    #[test]
    fn limiter_cuts_fuel_and_latches() {
        let cfg = bugatti_veyron_super_sport();
        let mut engine = EngineState::new(&cfg.engine);
        engine.angular_velocity = rpm_to_rad(cfg.engine.limiter_rpm + 50.0);
        let cut = engine.torque(&cfg.engine, 1.0);
        assert!(cut < 0.0, "limiter must not produce drive torque: {cut}");
        assert!(engine.limiter_active);
        // Still latched slightly below the limit.
        engine.angular_velocity = rpm_to_rad(cfg.engine.limiter_rpm - 100.0);
        let _ = engine.torque(&cfg.engine, 1.0);
        assert!(engine.limiter_active);
        // Released well below.
        engine.angular_velocity = rpm_to_rad(cfg.engine.limiter_rpm - 400.0);
        let t = engine.torque(&cfg.engine, 1.0);
        assert!(!engine.limiter_active);
        assert!(t > 0.0);
    }

    #[test]
    fn closed_throttle_produces_engine_braking() {
        let cfg = bugatti_veyron_super_sport();
        let mut engine = EngineState::new(&cfg.engine);
        engine.angular_velocity = rpm_to_rad(5000.0);
        let t = engine.torque(&cfg.engine, 0.0);
        assert!(t < 0.0, "expected negative torque, got {t}");
    }

    #[test]
    fn power_multiplier_triples_drive_without_tripling_engine_braking() {
        let cfg = bugatti_veyron_super_sport();
        let mut normal = EngineState::new(&cfg.engine);
        let mut boosted = normal;
        normal.angular_velocity = rpm_to_rad(4000.0);
        boosted.angular_velocity = normal.angular_velocity;

        let normal_torque = normal.torque(&cfg.engine, 1.0);
        let boosted_torque = boosted.torque_with_power_multiplier(&cfg.engine, 1.0, 3.0);
        assert!((boosted_torque - normal_torque * 3.0).abs() < 0.01);

        let normal_braking = normal.torque(&cfg.engine, 0.0);
        let boosted_braking = boosted.torque_with_power_multiplier(&cfg.engine, 0.0, 3.0);
        assert!((boosted_braking - normal_braking).abs() < 0.01);
    }

    #[test]
    fn disabled_rev_limiter_allows_top_gear_overspeed() {
        let cfg = bugatti_veyron_super_sport();
        let mut engine = EngineState::new(&cfg.engine);
        engine.rev_limiter_enabled = false;
        engine.angular_velocity = rpm_to_rad(cfg.engine.limiter_rpm + 500.0);

        let torque = engine.torque(&cfg.engine, 1.0);
        assert!(torque > 0.0, "disabled limiter still cut fuel: {torque}");
        assert!(engine.rpm() > cfg.engine.limiter_rpm);
    }

    #[test]
    fn idle_controller_holds_idle() {
        let cfg = bugatti_veyron_super_sport();
        let mut engine = EngineState::new(&cfg.engine);
        for _ in 0..4000 {
            let t = engine.torque(&cfg.engine, 0.0);
            engine.integrate_free(&cfg.engine, t, 1.0 / 240.0);
        }
        let rpm = engine.rpm();
        assert!(
            (cfg.engine.idle_rpm * 0.6..=cfg.engine.idle_rpm * 1.3).contains(&rpm),
            "idle settled at {rpm} rpm"
        );
    }
}
