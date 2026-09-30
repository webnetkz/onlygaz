//! Tyre model.
//!
//! A Pacejka-style Magic Formula is evaluated separately for the longitudinal
//! and lateral directions, then combined through a friction ellipse expressed
//! in *normalised slip* coordinates:
//!
//! ```text
//!     s = sqrt( (kappa / kappa_peak)^2 + (tan(alpha) / tan(alpha)_peak)^2 )
//!     Fx = (|kappa / kappa_peak| / s) * Fx_pure(kappa)
//!     Fy = (|tan(alpha) / alpha_peak| / s) * Fy_pure(alpha)
//! ```
//!
//! When only one slip is present the pure-slip curve is reproduced exactly, and
//! when both are equal the forces land precisely on the friction ellipse. Grip
//! is therefore always finite: past the peak the curves fall away and the car
//! starts to slide, which is the whole point of the exercise.
//!
//! Slip itself is not taken instantaneously from the kinematics - it is passed
//! through a first-order *relaxation length* filter, which is what real carcass
//! flexibility does and what keeps the stiff tyre ODE stable near zero speed.

use super::config::TireConfig;

/// Denominator floor for the slip ratio [m/s]. Without it the slip ratio is
/// singular at standstill.
const SLIP_RATIO_SPEED_FLOOR: f32 = 1.0;
/// Speed used to drive the relaxation filter when the car is nearly stopped.
const RELAXATION_SPEED_FLOOR: f32 = 1.5;
/// Slip ratios beyond this are physically meaningless and only cause blow-ups.
const MAX_SLIP_RATIO: f32 = 4.0;

/// Filtered slip state of a single tyre. Persisted between ticks.
#[derive(Debug, Clone, Copy, Default)]
pub struct TireState {
    /// Relaxed longitudinal slip ratio (kappa).
    pub slip_ratio: f32,
    /// Relaxed lateral slip, stored as `tan(alpha)`.
    pub slip_tangent: f32,
}

impl TireState {
    /// Slip angle in radians.
    #[inline]
    pub fn slip_angle(&self) -> f32 {
        self.slip_tangent.atan()
    }

    /// Advance the relaxation filter.
    ///
    /// * `longitudinal_velocity` - contact-patch velocity along the wheel
    ///   heading [m/s].
    /// * `lateral_velocity` - contact-patch velocity across the wheel [m/s].
    /// * `wheel_speed` - wheel angular velocity [rad/s].
    /// * `radius` - rolling radius [m].
    pub fn update(
        &mut self,
        cfg: &TireConfig,
        longitudinal_velocity: f32,
        lateral_velocity: f32,
        wheel_speed: f32,
        radius: f32,
        dt: f32,
    ) {
        let vx = longitudinal_velocity;
        let denominator = vx.abs().max(SLIP_RATIO_SPEED_FLOOR);

        let target_ratio =
            ((wheel_speed * radius - vx) / denominator).clamp(-MAX_SLIP_RATIO, MAX_SLIP_RATIO);
        let target_tangent =
            (lateral_velocity / denominator).clamp(-MAX_SLIP_RATIO, MAX_SLIP_RATIO);

        // First order lag with a distance constant: the slip needs to roll
        // `relaxation` metres to reach its steady state value.
        let travel = vx.abs().max(RELAXATION_SPEED_FLOOR) * dt;
        let alpha_long = (travel / cfg.relaxation_long.max(1.0e-3)).clamp(0.0, 1.0);
        let alpha_lat = (travel / cfg.relaxation_lat.max(1.0e-3)).clamp(0.0, 1.0);

        self.slip_ratio += (target_ratio - self.slip_ratio) * alpha_long;
        self.slip_tangent += (target_tangent - self.slip_tangent) * alpha_lat;

        if !self.slip_ratio.is_finite() {
            self.slip_ratio = 0.0;
        }
        if !self.slip_tangent.is_finite() {
            self.slip_tangent = 0.0;
        }
    }

    pub fn reset(&mut self) {
        self.slip_ratio = 0.0;
        self.slip_tangent = 0.0;
    }
}

/// Forces produced by one contact patch, in the wheel's own frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct TireForces {
    /// Along the wheel heading, positive drives the car forward [N].
    pub longitudinal: f32,
    /// Across the wheel, positive points to the wheel's right [N].
    pub lateral: f32,
    /// Load-corrected friction coefficient actually used.
    pub friction: f32,
    /// Normalised combined slip; >= 1 means the tyre is past its peak.
    pub combined_slip: f32,
    /// Fraction of the available friction circle in use, 0..~1.
    pub utilisation: f32,
}

impl TireForces {
    /// True once the tyre is beyond the peak of the curve, i.e. sliding.
    #[inline]
    pub fn is_sliding(&self) -> bool {
        self.combined_slip > 1.0
    }
}

/// Load-sensitive friction coefficient: heavily loaded tyres grip relatively
/// less, which is what produces load transfer sensitivity and, ultimately,
/// the difference between understeer and oversteer.
pub fn friction_at_load(cfg: &TireConfig, normal_force: f32) -> f32 {
    if normal_force <= 0.0 {
        return 0.0;
    }
    let relative = normal_force / cfg.nominal_load.max(1.0);
    let mu = cfg.peak_friction * (1.0 - cfg.load_sensitivity * (relative - 1.0));
    mu.clamp(cfg.friction_clamp.0, cfg.friction_clamp.1)
}

/// Evaluate the tyre for a given slip state and vertical load.
pub fn evaluate(cfg: &TireConfig, state: &TireState, normal_force: f32) -> TireForces {
    if normal_force <= 0.0 {
        return TireForces::default();
    }

    let mu = friction_at_load(cfg, normal_force);
    let peak_capacity = mu * normal_force;

    let kappa_peak = cfg.longitudinal.peak_slip().max(1.0e-3);
    let alpha_peak = cfg.lateral.peak_slip().max(1.0e-3);

    let nx = state.slip_ratio / kappa_peak;
    let ny = state.slip_tangent / alpha_peak;
    let s = (nx * nx + ny * ny).sqrt();

    if s < 1.0e-6 {
        return TireForces {
            friction: mu,
            ..Default::default()
        };
    }

    // Pure slip curves.
    let fx_pure = peak_capacity * cfg.longitudinal.evaluate(state.slip_ratio);
    let fy_pure = -peak_capacity * cfg.lateral.evaluate(state.slip_tangent);

    // Friction ellipse in normalised slip space.
    let wx = nx.abs() / s;
    let wy = ny.abs() / s;

    let longitudinal = wx * fx_pure;
    let lateral = wy * fy_pure;

    let magnitude = (longitudinal * longitudinal + lateral * lateral).sqrt();

    TireForces {
        longitudinal,
        lateral,
        friction: mu,
        combined_slip: s,
        utilisation: if peak_capacity > 0.0 {
            magnitude / peak_capacity
        } else {
            0.0
        },
    }
}

/// Rolling resistance torque opposing wheel rotation [N·m].
pub fn rolling_resistance_torque(
    cfg: &TireConfig,
    normal_force: f32,
    road_speed: f32,
    radius: f32,
) -> f32 {
    if normal_force <= 0.0 {
        return 0.0;
    }
    let coefficient = cfg.rolling_resistance + cfg.rolling_resistance_speed * road_speed.abs();
    coefficient * normal_force * radius
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::presets::bugatti_veyron_super_sport;

    fn front_tire() -> TireConfig {
        bugatti_veyron_super_sport().wheels[0].tire
    }

    #[test]
    fn grip_is_finite_and_peaks_near_the_design_slip() {
        let cfg = front_tire();
        let fz = 4500.0;
        let mut best = (0.0f32, 0.0f32);
        let mut kappa = 0.0;
        while kappa < 1.0 {
            let state = TireState {
                slip_ratio: kappa,
                slip_tangent: 0.0,
            };
            let f = evaluate(&cfg, &state, fz).longitudinal;
            if f > best.1 {
                best = (kappa, f);
            }
            kappa += 0.005;
        }
        let mu_peak = friction_at_load(&cfg, fz);
        assert!(
            (best.1 / fz - mu_peak).abs() < 0.05,
            "peak force {:.0} N is not mu*Fz ({:.0} N)",
            best.1,
            mu_peak * fz
        );
        assert!(
            (0.08..0.25).contains(&best.0),
            "peak slip ratio at {:.3}",
            best.0
        );
        // Well past the peak the force must have fallen off - that is the loss
        // of traction that lets the car spin its wheels and slide.
        let far = evaluate(
            &cfg,
            &TireState {
                slip_ratio: 1.5,
                slip_tangent: 0.0,
            },
            fz,
        );
        assert!(
            far.longitudinal < best.1 * 0.97,
            "no fall-off past the peak"
        );
    }

    #[test]
    fn combined_slip_stays_inside_the_friction_circle() {
        let cfg = front_tire();
        let fz = 5000.0;
        let mu = friction_at_load(&cfg, fz);
        for kappa in [-0.6, -0.2, 0.0, 0.15, 0.5, 1.5] {
            for tangent in [-0.8, -0.2, 0.0, 0.3, 1.0] {
                let state = TireState {
                    slip_ratio: kappa,
                    slip_tangent: tangent,
                };
                let f = evaluate(&cfg, &state, fz);
                let magnitude = (f.longitudinal * f.longitudinal + f.lateral * f.lateral).sqrt();
                assert!(
                    magnitude <= mu * fz * 1.02,
                    "kappa {kappa} tan(a) {tangent}: {magnitude:.0} N exceeds mu*Fz {:.0} N",
                    mu * fz
                );
            }
        }
    }

    #[test]
    fn lateral_force_opposes_lateral_slip() {
        let cfg = front_tire();
        let right = evaluate(
            &cfg,
            &TireState {
                slip_ratio: 0.0,
                slip_tangent: 0.1,
            },
            4500.0,
        );
        assert!(right.lateral < 0.0, "sliding right must push left");
        let left = evaluate(
            &cfg,
            &TireState {
                slip_ratio: 0.0,
                slip_tangent: -0.1,
            },
            4500.0,
        );
        assert!(left.lateral > 0.0);
    }

    #[test]
    fn friction_falls_with_load() {
        let cfg = front_tire();
        let light = friction_at_load(&cfg, 2000.0);
        let heavy = friction_at_load(&cfg, 9000.0);
        assert!(light > heavy, "{light} !> {heavy}");
    }

    #[test]
    fn no_load_means_no_force() {
        let cfg = front_tire();
        let f = evaluate(
            &cfg,
            &TireState {
                slip_ratio: 0.5,
                slip_tangent: 0.5,
            },
            0.0,
        );
        assert_eq!(f.longitudinal, 0.0);
        assert_eq!(f.lateral, 0.0);
    }

    #[test]
    fn relaxation_converges_to_the_kinematic_slip() {
        let cfg = front_tire();
        let mut state = TireState::default();
        let vx = 30.0;
        let omega = 32.0 / cfg.radius;
        for _ in 0..2000 {
            state.update(&cfg, vx, 0.0, omega, cfg.radius, 1.0 / 240.0);
        }
        let expected = (omega * cfg.radius - vx) / vx;
        assert!(
            (state.slip_ratio - expected).abs() < 1.0e-3,
            "{} vs {}",
            state.slip_ratio,
            expected
        );
    }
}
