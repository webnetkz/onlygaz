//! Static configuration of a vehicle.
//!
//! Everything in this module is *pure data*: no Bevy systems, no physics engine
//! types. A different car is a different [`VehicleConfig`] value and nothing else.
//!
//! # Coordinate conventions
//!
//! All values are SI (metres, seconds, kilograms, newtons, radians) and are
//! expressed in the **vehicle local frame**, which follows Bevy's convention:
//!
//! ```text
//!   -Z  forward (nose)
//!   +Y  up
//!   +X  right
//! ```
//!
//! The origin of the frame sits on the ground plane, between the axles, at the
//! lateral centre of the car. It coincides with the origin of the supplied glTF
//! model after the model has been yaw-rotated by 180° (the model is authored
//! with +Z forward), see [`crate::vehicle::model_rig`].

use bevy::prelude::*;

/// Which wheels receive engine torque.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub enum Drivetrain {
    /// Front wheel drive.
    Fwd,
    /// Rear wheel drive.
    Rwd,
    /// All wheel drive with a `front_bias` fraction of torque on the front axle.
    Awd,
}

/// Index of a corner of the car. The numeric value doubles as the index into
/// the per-wheel arrays used all over the solver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum WheelId {
    FrontLeft = 0,
    FrontRight = 1,
    RearLeft = 2,
    RearRight = 3,
}

impl WheelId {
    pub const ALL: [WheelId; 4] = [
        WheelId::FrontLeft,
        WheelId::FrontRight,
        WheelId::RearLeft,
        WheelId::RearRight,
    ];

    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[inline]
    pub const fn is_front(self) -> bool {
        matches!(self, WheelId::FrontLeft | WheelId::FrontRight)
    }

    #[inline]
    pub const fn is_left(self) -> bool {
        matches!(self, WheelId::FrontLeft | WheelId::RearLeft)
    }

    pub const fn short_name(self) -> &'static str {
        match self {
            WheelId::FrontLeft => "FL",
            WheelId::FrontRight => "FR",
            WheelId::RearLeft => "RL",
            WheelId::RearRight => "RR",
        }
    }
}

/// A monotonically increasing engine torque curve, sampled at discrete engine
/// speeds and linearly interpolated in between.
#[derive(Debug, Clone, Reflect)]
pub struct TorqueCurve {
    /// `(engine speed [rpm], full-throttle crank torque [N·m])`, sorted by rpm.
    pub points: Vec<(f32, f32)>,
}

impl TorqueCurve {
    pub fn new(points: Vec<(f32, f32)>) -> Self {
        let mut points = points;
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        Self { points }
    }

    /// Full-throttle crank torque at `rpm`, clamped to the ends of the curve.
    pub fn torque_at(&self, rpm: f32) -> f32 {
        match self.points.as_slice() {
            [] => 0.0,
            [only] => only.1,
            points => {
                if rpm <= points[0].0 {
                    return points[0].1;
                }
                if rpm >= points[points.len() - 1].0 {
                    return points[points.len() - 1].1;
                }
                // Linear interpolation between the two bracketing samples.
                let idx = points.partition_point(|p| p.0 <= rpm).max(1);
                let (r0, t0) = points[idx - 1];
                let (r1, t1) = points[idx];
                let f = if (r1 - r0).abs() < f32::EPSILON {
                    0.0
                } else {
                    (rpm - r0) / (r1 - r0)
                };
                t0 + (t1 - t0) * f
            }
        }
    }

    /// Peak power of the curve in watts, together with the rpm it occurs at.
    pub fn peak_power(&self) -> (f32, f32) {
        let mut best = (0.0f32, 0.0f32);
        let mut rpm = self.points.first().map(|p| p.0).unwrap_or(0.0);
        let end = self.points.last().map(|p| p.0).unwrap_or(0.0);
        while rpm <= end {
            let power = self.torque_at(rpm) * rpm * core::f32::consts::TAU / 60.0;
            if power > best.0 {
                best = (power, rpm);
            }
            rpm += 10.0;
        }
        best
    }
}

/// Internal combustion engine parameters.
#[derive(Debug, Clone, Reflect)]
pub struct EngineConfig {
    pub torque_curve: TorqueCurve,
    /// Rotational inertia of crank + flywheel + clutch basket [kg·m²].
    pub inertia: f32,
    pub idle_rpm: f32,
    pub redline_rpm: f32,
    /// Hard cut. Above this the ECU stops fuelling until rpm drops below
    /// `limiter_rpm - limiter_hysteresis_rpm`.
    pub limiter_rpm: f32,
    pub limiter_hysteresis_rpm: f32,
    /// Constant part of the closed-throttle drag torque [N·m].
    pub engine_brake_constant: f32,
    /// Speed-proportional part of the closed-throttle drag torque [N·m per rad/s].
    pub engine_brake_per_rad: f32,
    /// Torque the idle controller may add to hold [`Self::idle_rpm`] [N·m].
    pub idle_control_torque: f32,
}

/// Gearbox parameters.
#[derive(Debug, Clone, Reflect)]
pub struct TransmissionConfig {
    /// Forward gear ratios, index 0 == 1st gear.
    pub gear_ratios: Vec<f32>,
    pub reverse_ratio: f32,
    pub final_drive_ratio: f32,
    /// Mechanical efficiency of gearbox + differentials, 0..1.
    pub efficiency: f32,
    /// Upshift when engine speed exceeds this.
    pub upshift_rpm: f32,
    /// Downshift when engine speed would stay below this after the shift.
    pub downshift_rpm: f32,
    /// Torque interruption during a shift [s].
    pub shift_time: f32,
    /// Minimum time between two shifts, prevents hunting between gears [s].
    pub shift_cooldown: f32,
    /// Clutch torque capacity while launching [N·m].
    pub clutch_capacity: f32,
    /// Below this road speed the clutch is allowed to slip [m/s].
    pub clutch_lock_speed: f32,
}

impl TransmissionConfig {
    /// Total engine -> wheel ratio of `gear` (1-based; 0 == neutral, -1 == reverse).
    pub fn total_ratio(&self, gear: i8) -> f32 {
        match gear {
            0 => 0.0,
            g if g < 0 => -self.reverse_ratio * self.final_drive_ratio,
            g => {
                self.gear_ratios
                    .get((g - 1) as usize)
                    .copied()
                    .unwrap_or(0.0)
                    * self.final_drive_ratio
            }
        }
    }

    pub fn top_gear(&self) -> i8 {
        self.gear_ratios.len() as i8
    }
}

/// Coil-over spring/damper parameters for one axle.
#[derive(Debug, Clone, Copy, Reflect)]
pub struct SuspensionConfig {
    /// Distance from the strut anchor to the hub with the spring unloaded [m].
    pub rest_length: f32,
    /// Maximum travel above the rest position (bump) [m].
    pub max_compression: f32,
    /// Maximum travel below the rest position (droop) [m].
    pub max_droop: f32,
    /// Linear spring rate [N/m].
    pub spring_stiffness: f32,
    /// Extra progressive rate applied to the square of compression [N/m²].
    pub progressive_stiffness: f32,
    /// Damping while the spring compresses [N·s/m].
    pub damper_compression: f32,
    /// Damping while the spring extends [N·s/m].
    pub damper_rebound: f32,
    /// Bump-stop rate once travel is exhausted [N/m].
    pub bump_stop_stiffness: f32,
    /// Anti-roll bar rate, applied to the compression difference across the axle [N/m].
    pub anti_roll_stiffness: f32,
}

/// Pacejka-style Magic Formula coefficients for one slip direction.
#[derive(Debug, Clone, Copy, Reflect)]
pub struct MagicFormula {
    /// Stiffness factor.
    pub b: f32,
    /// Shape factor.
    pub c: f32,
    /// Curvature factor.
    pub e: f32,
}

impl MagicFormula {
    /// `F/D = sin(C * atan(B*s - E*(B*s - atan(B*s))))`, normalised to +-1.
    #[inline]
    pub fn evaluate(&self, slip: f32) -> f32 {
        let bs = self.b * slip;
        (self.c * (bs - self.e * (bs - bs.atan())).atan()).sin()
    }

    /// Slip value at which the curve peaks.
    ///
    /// The peak is where the inner argument reaches `tan(pi / (2C))`. The inner
    /// function is strictly increasing for any `E <= 1`, so a bisection finds
    /// the root exactly - the usual closed form is only correct for `E = 0` and
    /// is badly wrong for the `E` values real tyre fits use.
    pub fn peak_slip(&self) -> f32 {
        let target = (core::f32::consts::FRAC_PI_2 / self.c).tan();
        if !target.is_finite() || target <= 0.0 {
            return 0.15;
        }
        let inner = |u: f32| (1.0 - self.e) * u + self.e * u.atan();
        let mut low = 0.0f32;
        let mut high = 1.0f32;
        for _ in 0..40 {
            if inner(high) >= target {
                break;
            }
            high *= 2.0;
        }
        for _ in 0..40 {
            let mid = 0.5 * (low + high);
            if inner(mid) < target {
                low = mid;
            } else {
                high = mid;
            }
        }
        (0.5 * (low + high)) / self.b.max(1.0e-3)
    }
}

/// Tyre parameters for one axle.
#[derive(Debug, Clone, Copy, Reflect)]
pub struct TireConfig {
    /// Unloaded rolling radius [m].
    pub radius: f32,
    /// Tread width [m], used for the contact patch visualisation only.
    pub width: f32,
    /// Wheel + tyre + brake disc rotational inertia [kg·m²].
    pub inertia: f32,
    /// Peak friction coefficient at the nominal vertical load.
    pub peak_friction: f32,
    /// Vertical load the peak friction is quoted at [N].
    pub nominal_load: f32,
    /// Fractional loss of grip per unit of relative overload.
    /// `mu = peak * (1 - sensitivity * (Fz / nominal - 1))`.
    pub load_sensitivity: f32,
    /// Lower/upper clamp applied to the load-corrected friction coefficient.
    pub friction_clamp: (f32, f32),
    pub longitudinal: MagicFormula,
    pub lateral: MagicFormula,
    /// Longitudinal relaxation length [m] - how far the tyre must roll for the
    /// slip ratio to reach its steady-state value.
    pub relaxation_long: f32,
    /// Lateral relaxation length [m].
    pub relaxation_lat: f32,
    /// Constant part of the rolling resistance coefficient.
    pub rolling_resistance: f32,
    /// Speed-dependent part of the rolling resistance coefficient [s/m].
    pub rolling_resistance_speed: f32,
}

/// Friction brake parameters.
#[derive(Debug, Clone, Copy, Reflect)]
pub struct BrakeConfig {
    /// Maximum clamping torque of one front brake [N·m].
    pub front_torque: f32,
    /// Maximum clamping torque of one rear brake [N·m].
    pub rear_torque: f32,
    /// Handbrake torque per rear wheel [N·m].
    pub handbrake_torque: f32,
    /// ABS releases pressure above this magnitude of (negative) slip ratio.
    pub abs_slip_target: f32,
    /// How aggressively ABS bleeds pressure once the target is exceeded.
    pub abs_gain: f32,
    pub abs_enabled: bool,
}

/// Steering geometry and speed sensitivity.
#[derive(Debug, Clone, Reflect)]
pub struct SteeringConfig {
    /// Maximum road-wheel angle at standstill [rad].
    pub max_angle: f32,
    /// `(road speed [m/s], maximum road-wheel angle [rad])`, sorted by speed.
    /// The curve is interpolated and clamped at both ends.
    pub speed_curve: Vec<(f32, f32)>,
    /// How fast the normalised steering input approaches the stick position [1/s].
    pub input_rate: f32,
    /// How fast it returns to centre when the key is released [1/s].
    pub return_rate: f32,
    /// Use Ackermann geometry (inner wheel steers more than the outer one).
    pub ackermann: bool,
    /// Steering-wheel rotation for full lock [rad], used to animate the model.
    pub steering_wheel_rotation: f32,
}

impl SteeringConfig {
    /// Speed-dependent maximum road-wheel angle [rad].
    pub fn max_angle_at(&self, speed: f32) -> f32 {
        let speed = speed.abs();
        match self.speed_curve.as_slice() {
            [] => self.max_angle,
            points => {
                if speed <= points[0].0 {
                    return points[0].1;
                }
                if speed >= points[points.len() - 1].0 {
                    return points[points.len() - 1].1;
                }
                let idx = points.partition_point(|p| p.0 <= speed).max(1);
                let (s0, a0) = points[idx - 1];
                let (s1, a1) = points[idx];
                let f = if (s1 - s0).abs() < f32::EPSILON {
                    0.0
                } else {
                    (speed - s0) / (s1 - s0)
                };
                a0 + (a1 - a0) * f
            }
        }
    }
}

/// Aerodynamic parameters. Forces follow `F = 0.5 * rho * C * A * v^2`.
#[derive(Debug, Clone, Copy, Reflect)]
pub struct AeroConfig {
    pub air_density: f32,
    pub drag_coefficient: f32,
    pub frontal_area: f32,
    /// Negative-lift coefficient acting on the front axle.
    pub lift_coefficient_front: f32,
    /// Negative-lift coefficient acting on the rear axle.
    pub lift_coefficient_rear: f32,
    /// Point the drag force is applied at, vehicle local [m].
    pub drag_center: Vec3,
    /// Point the front downforce is applied at, vehicle local [m].
    pub downforce_center_front: Vec3,
    /// Point the rear downforce is applied at, vehicle local [m].
    pub downforce_center_rear: Vec3,
}

impl AeroConfig {
    #[inline]
    pub fn dynamic_pressure(&self, speed: f32) -> f32 {
        0.5 * self.air_density * speed * speed
    }

    #[inline]
    pub fn drag_force(&self, speed: f32) -> f32 {
        self.dynamic_pressure(speed) * self.drag_coefficient * self.frontal_area
    }

    #[inline]
    pub fn downforce_front(&self, speed: f32) -> f32 {
        self.dynamic_pressure(speed) * self.lift_coefficient_front * self.frontal_area
    }

    #[inline]
    pub fn downforce_rear(&self, speed: f32) -> f32 {
        self.dynamic_pressure(speed) * self.lift_coefficient_rear * self.frontal_area
    }
}

/// Everything needed to place and simulate one corner of the car.
#[derive(Debug, Clone, Copy, Reflect)]
pub struct WheelConfig {
    pub id: WheelId,
    /// Top of the suspension strut in vehicle local space [m].
    pub anchor: Vec3,
    /// This wheel is steered by the driver.
    pub steerable: bool,
    pub tire: TireConfig,
    pub suspension: SuspensionConfig,
    /// Maximum friction brake torque on this wheel [N·m].
    pub brake_torque: f32,
    /// Handbrake torque on this wheel [N·m].
    pub handbrake_torque: f32,
}

/// The full description of a car.
#[derive(Debug, Clone, Resource, Reflect)]
pub struct VehicleConfig {
    pub name: &'static str,
    /// Dry mass including driver and fluids [kg].
    pub mass: f32,
    /// Principal moments of inertia about the local X (pitch), Y (yaw) and
    /// Z (roll) axes [kg·m²].
    pub inertia: Vec3,
    /// Centre of mass in vehicle local space [m].
    pub center_of_mass: Vec3,
    pub wheel_base: f32,
    pub track_width_front: f32,
    pub track_width_rear: f32,
    pub wheels: [WheelConfig; 4],
    pub engine: EngineConfig,
    pub transmission: TransmissionConfig,
    pub drivetrain: Drivetrain,
    /// Fraction of drive torque sent to the front axle when [`Drivetrain::Awd`].
    pub front_torque_bias: f32,
    /// 0 = fully open differential, 1 = fully locked.
    pub differential_locking: f32,
    pub brakes: BrakeConfig,
    pub steering: SteeringConfig,
    pub aero: AeroConfig,
    /// Half extents and centres of the convex boxes making up the body collider.
    pub collider_boxes: Vec<(Vec3, Vec3)>,
    /// Where the driver's eyes are, vehicle local [m].
    pub driver_eye: Vec3,
    /// Vehicle-specific correction for the gameplay cockpit camera [m].
    /// Positive Z moves the view rearwards because the car faces -Z.
    pub cockpit_camera_offset: Vec3,
    /// Vehicle-specific horizontal aim correction for the cockpit camera.
    pub cockpit_camera_yaw_degrees: f32,
    /// Centre of the steering wheel, vehicle local [m].
    pub steering_wheel_center: Vec3,
    /// Steering column axis in vehicle local space (the wheel spins about it).
    pub steering_wheel_axis: Vec3,
    /// Number of internal sub-steps the wheel/tyre ODE is integrated with per
    /// physics tick. Keeps the stiff tyre model stable without raising the
    /// global physics rate.
    pub solver_substeps: u32,
}

impl VehicleConfig {
    #[inline]
    pub fn wheel(&self, id: WheelId) -> &WheelConfig {
        &self.wheels[id.index()]
    }

    /// Total mass of the car in newtons of weight.
    #[inline]
    pub fn weight(&self, gravity: f32) -> f32 {
        self.mass * gravity
    }

    /// Distance from the centre of mass to the front axle [m].
    pub fn com_to_front_axle(&self) -> f32 {
        (self.center_of_mass.z - self.wheels[WheelId::FrontLeft.index()].anchor.z).abs()
    }

    /// Distance from the centre of mass to the rear axle [m].
    pub fn com_to_rear_axle(&self) -> f32 {
        (self.wheels[WheelId::RearLeft.index()].anchor.z - self.center_of_mass.z).abs()
    }

    /// Static front axle weight fraction implied by the centre of mass.
    pub fn static_front_weight_fraction(&self) -> f32 {
        let a = self.com_to_front_axle();
        let b = self.com_to_rear_axle();
        b / (a + b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_formula_peak_is_found_numerically() {
        for (b, c, e) in [
            (11.5f32, 1.90f32, 0.97f32),
            (9.8, 1.45, -0.5),
            (10.0, 1.6, 0.0),
        ] {
            let mf = MagicFormula { b, c, e };
            let peak = mf.peak_slip();
            let value = mf.evaluate(peak);
            // The reported peak must actually be the maximum of the curve.
            for offset in [-0.05f32, -0.02, 0.02, 0.05] {
                let other = mf.evaluate(peak + offset);
                assert!(
                    other <= value + 1.0e-3,
                    "B={b} C={c} E={e}: peak_slip {peak:.3} is not the maximum \
                     ({value:.4} vs {other:.4} at {:.3})",
                    peak + offset
                );
            }
            assert!((value - 1.0).abs() < 1.0e-3, "peak value {value}");
        }
    }

    #[test]
    fn torque_curve_interpolation_is_monotonic_between_samples() {
        let curve = TorqueCurve::new(vec![(1000.0, 500.0), (2000.0, 900.0)]);
        assert_eq!(curve.torque_at(500.0), 500.0);
        assert_eq!(curve.torque_at(3000.0), 900.0);
        assert!((curve.torque_at(1500.0) - 700.0).abs() < 1.0e-3);
    }
}
