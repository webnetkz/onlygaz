//! The vehicle solver.
//!
//! This is the heart of the simulator and it is deliberately free of any
//! physics-engine or renderer types: it takes the rigid body's current state,
//! asks a [`GroundProbe`] what is under each wheel, and returns a list of forces
//! to apply. That makes the whole model unit-testable without a window, which is
//! how the acceleration and braking figures in `tests/physics_validation.rs` are
//! produced.
//!
//! Chain of causality, in order:
//!
//! ```text
//!   throttle -> engine torque curve -> clutch -> gearbox -> differentials
//!            -> wheel angular acceleration -> tyre slip -> tyre force
//!            -> chassis acceleration
//! ```
//!
//! Nothing anywhere sets a velocity or a transform directly.

use bevy::prelude::*;

use super::aero::{self, AeroOutput};
use super::brakes;
use super::config::{VehicleConfig, WheelId};
use super::drivetrain;
use super::engine::{EngineState, rad_to_rpm, rpm_to_rad};
use super::tire;
use super::transmission::{ShiftContext, TransmissionState};
use super::wheels::{GroundProbe, WheelState, anti_roll_force, suspension_force};

/// Extra ray length below the fully drooped wheel, so the probe still finds the
/// road when the car is a few millimetres airborne.
const RAY_MARGIN: f32 = 0.25;
/// Hard ceiling on a single wheel load, purely as a numerical safety net [N].
const MAX_NORMAL_FORCE: f32 = 6.0e5;
/// How quickly the launch clutch takes up drive [s].
const CLUTCH_ENGAGE_TIME: f32 = 0.55;
/// Slip speed over which the clutch is considered fully slipping [rad/s].
const CLUTCH_SLIP_SCALE: f32 = 12.0;
/// Below this road speed a held brake pedal selects reverse.
const REVERSE_SELECT_SPEED: f32 = 0.6;
/// How long the brake must be held at a standstill before reverse engages [s].
const REVERSE_SELECT_DELAY: f32 = 0.4;
/// Standard and F7 power-mode multipliers.
pub const NORMAL_POWER_MULTIPLIER: f32 = 1.0;
pub const TRIPLE_POWER_MULTIPLIER: f32 = 3.0;
/// Direct acceleration added while nitro is held. Applying the corresponding
/// force through the centre of mass produces a very strong arcade boost
/// without introducing a pitch moment that could launch the car [m/s²].
pub const NITRO_ACCELERATION_MPS2: f32 = 24.0;
/// Low-speed steering is intentionally more direct than the pure tyre model.
/// The assist is fully gone at this speed, keeping the proven high-speed setup
/// bit-for-bit unchanged.
const ARCADE_STEERING_FULL_MPS: f32 = 45.0 / 3.6;
const ARCADE_STEERING_END_MPS: f32 = 100.0 / 3.6;
const ARCADE_STEERING_ANGLE_GAIN: f32 = 0.28;
const ARCADE_YAW_RESPONSE: f32 = 4.0;
const ARCADE_MAX_LATERAL_ACCELERATION: f32 = 3.5;
const ARCADE_MAX_YAW_RATE: f32 = 1.6;

/// Driver inputs, already smoothed by the controller. All in 0..1 except
/// `steering`, which is -1 (full left) to +1 (full right).
#[derive(Debug, Clone, Copy, Default)]
pub struct ControlInput {
    pub throttle: f32,
    pub brake: f32,
    pub steering: f32,
    pub handbrake: f32,
    pub nitro: f32,
}

/// The rigid body state the solver needs, in world space.
#[derive(Debug, Clone, Copy)]
pub struct ChassisState {
    /// Origin of the vehicle local frame.
    pub translation: Vec3,
    pub rotation: Quat,
    /// Velocity of the centre of mass.
    pub linear_velocity: Vec3,
    pub angular_velocity: Vec3,
    /// Centre of mass in world space.
    pub center_of_mass: Vec3,
}

impl ChassisState {
    #[inline]
    pub fn up(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }
    #[inline]
    pub fn forward(&self) -> Vec3 {
        self.rotation * Vec3::NEG_Z
    }
    #[inline]
    pub fn right(&self) -> Vec3 {
        self.rotation * Vec3::X
    }

    /// Velocity of a world-space point rigidly attached to the body.
    #[inline]
    pub fn velocity_at(&self, world_point: Vec3) -> Vec3 {
        self.linear_velocity
            + self
                .angular_velocity
                .cross(world_point - self.center_of_mass)
    }
}

/// A force to apply to the rigid body, together with the world point it acts at.
#[derive(Debug, Clone, Copy, Default)]
pub struct AppliedForce {
    pub force: Vec3,
    pub point: Vec3,
}

/// Everything the solver produced this tick.
#[derive(Debug, Clone, Default)]
pub struct SolverOutput {
    forces: [AppliedForce; 12],
    count: usize,
}

impl SolverOutput {
    fn push(&mut self, force: Vec3, point: Vec3) {
        if self.count < self.forces.len() && force.is_finite() {
            self.forces[self.count] = AppliedForce { force, point };
            self.count += 1;
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &AppliedForce> {
        self.forces[..self.count].iter()
    }

    /// Net force, for telemetry and tests.
    pub fn net_force(&self) -> Vec3 {
        self.iter().map(|f| f.force).sum()
    }

    /// Net torque about `center`, for telemetry and tests.
    pub fn net_torque(&self, center: Vec3) -> Vec3 {
        self.iter().map(|f| (f.point - center).cross(f.force)).sum()
    }
}

/// The persistent, mutable state of one car.
#[derive(Debug, Clone)]
pub struct VehicleDynamics {
    pub engine: EngineState,
    pub transmission: TransmissionState,
    pub wheels: [WheelState; 4],
    /// A torn-off wheel no longer contributes suspension, grip or drive.
    pub detached_wheels: [bool; 4],
    /// 0 = clutch open, 1 = clutch locked.
    pub clutch_engagement: f32,
    pub aero: AeroOutput,
    pub abs_enabled: bool,
    /// Multiplier for combustion torque. F7 toggles this between 1 and 3.
    pub power_multiplier: f32,
    /// Crank torque produced this tick [N·m].
    pub engine_torque: f32,
    /// Torque leaving the gearbox this tick [N·m].
    pub axle_torque: f32,
    /// Signed forward speed [m/s].
    pub forward_speed: f32,
    /// How long the brake has been held at a standstill [s].
    reverse_timer: f32,
}

impl VehicleDynamics {
    pub fn new(cfg: &VehicleConfig) -> Self {
        Self {
            engine: EngineState::new(&cfg.engine),
            transmission: TransmissionState::default(),
            wheels: [
                WheelState::new(&cfg.wheels[0]),
                WheelState::new(&cfg.wheels[1]),
                WheelState::new(&cfg.wheels[2]),
                WheelState::new(&cfg.wheels[3]),
            ],
            detached_wheels: [false; 4],
            clutch_engagement: 0.0,
            aero: AeroOutput::default(),
            abs_enabled: cfg.brakes.abs_enabled,
            power_multiplier: NORMAL_POWER_MULTIPLIER,
            engine_torque: 0.0,
            axle_torque: 0.0,
            forward_speed: 0.0,
            reverse_timer: 0.0,
        }
    }

    /// Put the car back to "engine running, gearbox in Drive, standing still".
    pub fn reset(&mut self, cfg: &VehicleConfig) {
        self.engine = EngineState::new(&cfg.engine);
        self.engine.rev_limiter_enabled = !self.triple_power_enabled();
        self.transmission = TransmissionState::default();
        for (state, wheel_cfg) in self.wheels.iter_mut().zip(cfg.wheels.iter()) {
            state.reset(wheel_cfg);
        }
        self.detached_wheels = [false; 4];
        self.clutch_engagement = 0.0;
        self.aero = AeroOutput::default();
        // Power mode is a driver toggle, so a vehicle respawn must not turn it
        // off behind the player's back.
        self.engine_torque = 0.0;
        self.axle_torque = 0.0;
        self.forward_speed = 0.0;
        self.reverse_timer = 0.0;
    }

    #[inline]
    pub fn rpm(&self) -> f32 {
        self.engine.rpm()
    }

    /// Mean speed of the driven wheels expressed in m/s, for the HUD.
    pub fn mean_wheel_speed(&self, cfg: &VehicleConfig) -> f32 {
        let mut sum = 0.0;
        let mut count = 0;
        for (index, (state, wheel)) in self.wheels.iter().zip(cfg.wheels.iter()).enumerate() {
            if !self.detached_wheels[index] {
                sum += state.angular_velocity * wheel.tire.radius;
                count += 1;
            }
        }
        if count == 0 { 0.0 } else { sum / count as f32 }
    }

    /// True if any wheel is spinning up or locked well past its peak slip.
    pub fn any_wheel_sliding(&self) -> bool {
        self.wheels.iter().any(|w| w.forces.is_sliding())
    }

    #[inline]
    pub fn triple_power_enabled(&self) -> bool {
        self.power_multiplier > NORMAL_POWER_MULTIPLIER
    }

    /// Toggle the F7 power mode and return its new state.
    pub fn toggle_triple_power(&mut self) -> bool {
        self.power_multiplier = if self.triple_power_enabled() {
            NORMAL_POWER_MULTIPLIER
        } else {
            TRIPLE_POWER_MULTIPLIER
        };
        self.engine.rev_limiter_enabled = !self.triple_power_enabled();
        if self.triple_power_enabled() {
            self.engine.limiter_active = false;
        }
        self.triple_power_enabled()
    }
}

/// Steering angle of each front wheel for a requested normalised input.
///
/// With Ackermann geometry the inside wheel is turned further than the outside
/// one so that both roll about a common centre instead of scrubbing.
pub fn steering_angles(cfg: &VehicleConfig, input: f32, speed: f32) -> (f32, f32) {
    let arcade_gain = 1.0 + ARCADE_STEERING_ANGLE_GAIN * low_speed_arcade_blend(speed);
    let max_angle = cfg.steering.max_angle_at(speed) * arcade_gain;
    let average = input.clamp(-1.0, 1.0) * max_angle;

    if !cfg.steering.ackermann || average.abs() < 1.0e-4 {
        return (average, average);
    }

    let wheel_base = cfg.wheel_base;
    let track = cfg.track_width_front;
    // Turn radius at the centre of the front axle.
    let radius = wheel_base / average.tan().abs();
    let inner = (wheel_base / (radius - track * 0.5).max(0.1)).atan();
    let outer = (wheel_base / (radius + track * 0.5)).atan();
    let sign = average.signum();

    // Turning right (positive) makes the right wheel the inner one.
    if sign > 0.0 {
        (outer * sign, inner * sign)
    } else {
        (inner * sign, outer * sign)
    }
}

/// 1 below 45 km/h, eased to exactly 0 at 100 km/h.
#[inline]
pub(crate) fn low_speed_arcade_blend(speed_mps: f32) -> f32 {
    let t = ((speed_mps.abs() - ARCADE_STEERING_FULL_MPS)
        / (ARCADE_STEERING_END_MPS - ARCADE_STEERING_FULL_MPS))
        .clamp(0.0, 1.0);
    let smooth = t * t * (3.0 - 2.0 * t);
    1.0 - smooth
}

/// Extra front-axle force that makes parking-lot and city-speed steering react
/// promptly to a digital key. It follows the bicycle-model yaw target, remains
/// grip-limited, requires ground contact and disappears completely by 100 km/h.
fn low_speed_arcade_steering_force(
    cfg: &VehicleConfig,
    dynamics: &VehicleDynamics,
    chassis: &ChassisState,
    steering_input: f32,
    average_steer_angle: f32,
) -> Option<AppliedForce> {
    let forward_speed = dynamics.forward_speed;
    let speed = forward_speed.abs();
    let moving = {
        let t = (speed / (8.0 / 3.6)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let strength = low_speed_arcade_blend(speed) * moving * steering_input.abs();
    if strength <= 1.0e-4
        || dynamics
            .wheels
            .iter()
            .filter(|wheel| wheel.grounded)
            .count()
            < 2
    {
        return None;
    }

    let target_yaw_rate = (-forward_speed / cfg.wheel_base * average_steer_angle.tan())
        .clamp(-ARCADE_MAX_YAW_RATE, ARCADE_MAX_YAW_RATE);
    let current_yaw_rate = chassis.angular_velocity.dot(chassis.up());
    let requested_torque =
        (target_yaw_rate - current_yaw_rate) * cfg.inertia.y * ARCADE_YAW_RESPONSE * strength;
    let lever_arm = (cfg.wheel_base * 0.45).max(0.5);
    let max_force = cfg.mass * ARCADE_MAX_LATERAL_ACCELERATION * strength;
    // The application point is ahead along local -Z, so a force to the right
    // produces negative world-Y yaw. Negate the requested torque when solving
    // F = tau / r to keep positive steering input turning the car right.
    let lateral_force = (-requested_torque / lever_arm).clamp(-max_force, max_force);

    Some(AppliedForce {
        force: chassis.right() * lateral_force,
        point: chassis.center_of_mass + chassis.forward() * lever_arm,
    })
}

/// Advance the vehicle model by `dt` and return the forces to hand to the rigid
/// body solver.
pub fn step(
    cfg: &VehicleConfig,
    dynamics: &mut VehicleDynamics,
    chassis: &ChassisState,
    input: &ControlInput,
    probe: &impl GroundProbe,
    dt: f32,
) -> SolverOutput {
    let mut output = SolverOutput::default();
    if dt <= 0.0 {
        return output;
    }

    let up = chassis.up();
    let forward = chassis.forward();
    let speed = chassis.linear_velocity.length();
    let forward_speed = chassis.linear_velocity.dot(forward);
    dynamics.forward_speed = forward_speed;

    // ---------------------------------------------------------------- steering
    let (left_angle, right_angle) = steering_angles(cfg, input.steering, speed);
    dynamics.wheels[WheelId::FrontLeft.index()].steer_angle = left_angle;
    dynamics.wheels[WheelId::FrontRight.index()].steer_angle = right_angle;

    // ------------------------------------------------------- suspension probe
    // Pass 1: geometry and spring travel for every corner.
    let mut compressions = [0.0f32; 4];
    for (index, ((wheel_cfg, state), compression)) in cfg
        .wheels
        .iter()
        .zip(dynamics.wheels.iter_mut())
        .zip(compressions.iter_mut())
        .enumerate()
    {
        if dynamics.detached_wheels[index] {
            state.grounded = false;
            state.normal_force = 0.0;
            state.applied_force = Vec3::ZERO;
            state.forces = Default::default();
            state.drive_torque = 0.0;
            state.brake_torque = 0.0;
            *compression = 0.0;
            continue;
        }
        let suspension = &wheel_cfg.suspension;

        let anchor = chassis.translation + chassis.rotation * wheel_cfg.anchor;
        let down = Dir3::new(-up).unwrap_or(Dir3::NEG_Y);
        let max_distance =
            suspension.rest_length + suspension.max_droop + wheel_cfg.tire.radius + RAY_MARGIN;

        let previous_length = state.suspension_length;
        match probe.cast(anchor, down, max_distance) {
            Some(hit) => {
                let length = (hit.distance - wheel_cfg.tire.radius).clamp(
                    suspension.rest_length - suspension.max_compression,
                    suspension.rest_length + suspension.max_droop,
                );
                state.suspension_length = length;
                state.grounded = hit.distance
                    <= suspension.rest_length + suspension.max_droop + wheel_cfg.tire.radius;
                state.contact_point = hit.point;
                state.contact_normal = hit.normal.normalize_or(Vec3::Y);
            }
            None => {
                state.suspension_length = suspension.rest_length + suspension.max_droop;
                state.grounded = false;
                state.contact_normal = up;
                state.contact_point =
                    anchor - up * (state.suspension_length + wheel_cfg.tire.radius);
            }
        }

        state.suspension_velocity = (previous_length - state.suspension_length) / dt;
        state.compression = suspension.rest_length - state.suspension_length;
        *compression = state.compression;
    }

    // Pass 2: spring, damper and anti-roll bar give the vertical load.
    for (index, (wheel_cfg, state)) in cfg
        .wheels
        .iter()
        .zip(dynamics.wheels.iter_mut())
        .enumerate()
    {
        if dynamics.detached_wheels[index] || !state.grounded {
            state.normal_force = 0.0;
            continue;
        }
        let paired = match wheel_cfg.id {
            WheelId::FrontLeft => WheelId::FrontRight,
            WheelId::FrontRight => WheelId::FrontLeft,
            WheelId::RearLeft => WheelId::RearRight,
            WheelId::RearRight => WheelId::RearLeft,
        };
        let arb = anti_roll_force(
            &wheel_cfg.suspension,
            compressions[index],
            compressions[paired.index()],
        );
        let force = suspension_force(
            &wheel_cfg.suspension,
            state.compression,
            state.suspension_velocity,
        ) + arb;
        state.normal_force = force.clamp(0.0, MAX_NORMAL_FORCE);
    }

    // ------------------------------------------------- contact patch kinematics
    // Wheel-aligned basis, projected onto the road surface.
    let mut wheel_forward = [Vec3::NEG_Z; 4];
    let mut wheel_right = [Vec3::X; 4];
    for ((state, forward_out), right_out) in dynamics
        .wheels
        .iter_mut()
        .zip(wheel_forward.iter_mut())
        .zip(wheel_right.iter_mut())
    {
        let normal = state.contact_normal;
        let steered = Quat::from_axis_angle(up, -state.steer_angle) * forward;
        let projected = (steered - normal * steered.dot(normal)).normalize_or(forward);
        let side = projected.cross(normal).normalize_or(chassis.right());
        *forward_out = projected;
        *right_out = side;

        let contact_velocity = chassis.velocity_at(state.contact_point);
        state.longitudinal_velocity = contact_velocity.dot(projected);
        state.lateral_velocity = contact_velocity.dot(side);
    }

    // --------------------------------------------------------------- gearbox
    let weights = active_torque_weights(cfg, &dynamics.detached_wheels);
    let wheel_speeds = wheel_speed_array(dynamics);
    let reference_speed = drivetrain::reference_wheel_speed(&weights, &wheel_speeds);

    // Reverse selection: holding the brake at a standstill picks reverse, and
    // touching the throttle again picks Drive. The keyboard only has one pedal
    // for each, so this is the usual convention.
    update_gear_direction(dynamics, input, forward_speed, dt);

    let forward_demand = input.throttle.max(input.nitro);
    dynamics.transmission.update(
        &cfg.transmission,
        &ShiftContext {
            engine_speed: dynamics.engine.angular_velocity,
            wheel_speed_ref: reference_speed,
            throttle: forward_demand,
            brake: input.brake,
            road_speed: forward_speed,
        },
        dt,
    );

    // The keyboard has no separate reverse pedal: once the gearbox is in R,
    // the driver's brake key becomes the accelerator for backing up. Keep the
    // regular brake input while selecting reverse, then reinterpret it only
    // after the gear has actually changed so holding `S` both selects R and
    // moves the car backwards.
    let driveline_input = if dynamics.transmission.gear < 0 {
        ControlInput {
            throttle: input.brake,
            brake: 0.0,
            steering: input.steering,
            handbrake: input.handbrake,
            nitro: 0.0,
        }
    } else {
        ControlInput {
            throttle: forward_demand,
            ..*input
        }
    };

    // --------------------------------------------- substepped wheel + engine ODE
    let substeps = cfg.solver_substeps.max(1);
    let h = dt / substeps as f32;
    let mut accumulated_long = [0.0f32; 4];
    let mut accumulated_lat = [0.0f32; 4];

    for _ in 0..substeps {
        integrate_driveline(cfg, dynamics, &driveline_input, h);

        for ((state, long), lat) in dynamics
            .wheels
            .iter_mut()
            .zip(accumulated_long.iter_mut())
            .zip(accumulated_lat.iter_mut())
        {
            *long += state.forces.longitudinal;
            *lat += state.forces.lateral;
            state.spin_angle += state.angular_velocity * h;
        }
    }

    let inverse_substeps = 1.0 / substeps as f32;
    for (long, lat) in accumulated_long.iter_mut().zip(accumulated_lat.iter_mut()) {
        *long *= inverse_substeps;
        *lat *= inverse_substeps;
    }

    // ------------------------------------------------------- assemble forces
    for (index, state) in dynamics.wheels.iter_mut().enumerate() {
        if !state.grounded {
            state.applied_force = Vec3::ZERO;
            continue;
        }
        let force = state.contact_normal * state.normal_force
            + wheel_forward[index] * accumulated_long[index]
            + wheel_right[index] * accumulated_lat[index];
        state.applied_force = force;
        output.push(force, state.contact_point);
    }

    let average_steer_angle = (left_angle + right_angle) * 0.5;
    if let Some(assist) =
        low_speed_arcade_steering_force(cfg, dynamics, chassis, input.steering, average_steer_angle)
    {
        output.push(assist.force, assist.point);
    }

    // ---------------------------------------------------------------- aero
    let (aero_output, aero_forces) = aero::evaluate(&cfg.aero, chassis.linear_velocity, up);
    dynamics.aero = aero_output;
    for entry in aero_forces {
        output.push(
            entry.force,
            chassis.translation + chassis.rotation * entry.local_point,
        );
    }

    // Nitro is an intentionally exaggerated game boost rather than another
    // engine torque multiplier. It remains effective in top gear and raises
    // the drag-limited maximum speed, while the centre-of-mass application and
    // grounded check keep it from pitching or rocketing an airborne car.
    if nitro_can_push(dynamics, input, forward_speed) {
        output.push(
            forward * (cfg.mass * NITRO_ACCELERATION_MPS2 * input.nitro.clamp(0.0, 1.0)),
            chassis.center_of_mass,
        );
    }

    output
}

/// The four hub speeds as a plain array [rad/s].
#[inline]
fn wheel_speed_array(dynamics: &VehicleDynamics) -> [f32; 4] {
    core::array::from_fn(|index| {
        if dynamics.detached_wheels[index] {
            0.0
        } else {
            dynamics.wheels[index].angular_velocity
        }
    })
}

fn active_torque_weights(cfg: &VehicleConfig, detached: &[bool; 4]) -> [f32; 4] {
    let mut weights = drivetrain::torque_weights(cfg);
    for (weight, missing) in weights.iter_mut().zip(detached) {
        if *missing {
            *weight = 0.0;
        }
    }
    let total: f32 = weights.iter().sum();
    if total > 0.0 {
        for weight in &mut weights {
            *weight /= total;
        }
    }
    weights
}

/// Pick Drive or Reverse from the pedals.
fn update_gear_direction(
    dynamics: &mut VehicleDynamics,
    input: &ControlInput,
    forward_speed: f32,
    dt: f32,
) {
    let nearly_stopped = forward_speed.abs() < REVERSE_SELECT_SPEED;
    if nearly_stopped && input.brake > 0.5 && input.throttle < 0.05 {
        dynamics.reverse_timer += dt;
    } else if input.throttle > 0.1 || input.nitro > 0.1 || !nearly_stopped {
        dynamics.reverse_timer = 0.0;
    }

    if dynamics.transmission.gear >= 0
        && dynamics.reverse_timer > REVERSE_SELECT_DELAY
        && forward_speed < REVERSE_SELECT_SPEED
    {
        dynamics.transmission.gear = -1;
        dynamics.transmission.shift_timer = 0.0;
    } else if dynamics.transmission.gear < 0 && (input.throttle > 0.1 || input.nitro > 0.1) {
        dynamics.transmission.gear = 1;
        dynamics.transmission.shift_timer = 0.0;
    }
}

#[inline]
fn nitro_can_push(dynamics: &VehicleDynamics, input: &ControlInput, forward_speed: f32) -> bool {
    input.nitro > 0.0
        && dynamics.transmission.gear >= 0
        && forward_speed >= -1.0
        && dynamics
            .wheels
            .iter()
            .filter(|wheel| wheel.grounded)
            .count()
            >= 2
}

/// One sub-step of the engine / clutch / wheel system.
///
/// The wheels carry the reflected inertia of the engine, which is what stops a
/// 1500 N·m first gear from spinning the tyres up instantly, and each wheel is
/// accelerated by `drive - brake - Fx * r - rolling resistance`. The tyre force
/// therefore *reacts back* on the hub, which is the mechanism that produces real
/// wheel spin and lock-up.
fn integrate_driveline(
    cfg: &VehicleConfig,
    dynamics: &mut VehicleDynamics,
    input: &ControlInput,
    h: f32,
) {
    let weights = active_torque_weights(cfg, &dynamics.detached_wheels);
    let wheel_speeds = wheel_speed_array(dynamics);
    let reference_speed = drivetrain::reference_wheel_speed(&weights, &wheel_speeds);

    let ratio = dynamics.transmission.total_ratio(&cfg.transmission);
    let efficiency = cfg.transmission.efficiency;
    let idle_speed = rpm_to_rad(cfg.engine.idle_rpm);
    let driveline_speed = reference_speed * ratio;

    // Clutch engagement: fully locked once the car is rolling and the driveline
    // is spinning at least as fast as idle; otherwise it takes drive up over
    // CLUTCH_ENGAGE_TIME so the car can pull away from rest.
    let locked =
        ratio != 0.0 && !dynamics.transmission.is_shifting() && driveline_speed.abs() >= idle_speed;
    // At a standstill the clutch stays open until the driver asks for drive.
    // A real dual clutch box would creep in Drive; here that would mean the car
    // wandering off on its own while parked, so drive is taken up with the
    // throttle instead.
    let target_engagement = if dynamics.transmission.is_shifting() || ratio == 0.0 {
        0.0
    } else if locked {
        1.0
    } else {
        (input.throttle * 0.95).clamp(0.0, 1.0)
    };
    let rate = (h / CLUTCH_ENGAGE_TIME).clamp(0.0, 1.0);
    dynamics.clutch_engagement += (target_engagement - dynamics.clutch_engagement) * rate;

    let axle_torque;
    if locked {
        // Rigid driveline: the crank speed is dictated by the wheels.
        dynamics
            .engine
            .set_from_driveline(&cfg.engine, driveline_speed);
        let crank_torque = dynamics.engine.torque_with_power_multiplier(
            &cfg.engine,
            input.throttle,
            dynamics.power_multiplier,
        );
        dynamics.engine_torque = crank_torque;
        axle_torque = crank_torque * ratio * efficiency;
    } else {
        // Slipping clutch: the engine runs on its own inertia.
        let crank_torque = dynamics.engine.torque_with_power_multiplier(
            &cfg.engine,
            input.throttle,
            dynamics.power_multiplier,
        );
        dynamics.engine_torque = crank_torque;

        let slip = dynamics.engine.angular_velocity - driveline_speed;
        let mut capacity = dynamics.clutch_engagement * cfg.transmission.clutch_capacity;
        // Anti stall: give way rather than dragging the crank under idle.
        if dynamics.engine.angular_velocity < idle_speed * 1.1 {
            let headroom = ((dynamics.engine.angular_velocity - idle_speed * 0.7)
                / (0.4 * idle_speed))
                .clamp(0.0, 1.0);
            capacity *= headroom;
        }
        let clutch_torque = capacity * (slip / CLUTCH_SLIP_SCALE).clamp(-1.0, 1.0);

        dynamics
            .engine
            .integrate_free(&cfg.engine, crank_torque - clutch_torque, h);
        axle_torque = clutch_torque * ratio * efficiency;
    }
    dynamics.axle_torque = axle_torque;

    let abs_enabled = dynamics.abs_enabled;
    let drive_torques = drivetrain::distribute(cfg, &weights, axle_torque, &wheel_speeds);
    let reflected = drivetrain::reflected_inertia(cfg, &weights, if locked { ratio } else { 0.0 });

    for (index, (wheel_cfg, state)) in cfg
        .wheels
        .iter()
        .zip(dynamics.wheels.iter_mut())
        .enumerate()
    {
        if dynamics.detached_wheels[index] {
            state.forces = Default::default();
            state.drive_torque = 0.0;
            state.brake_torque = 0.0;
            state.abs_active = false;
            continue;
        }
        let tire_cfg = &wheel_cfg.tire;

        // 1. Tyre slip and force at the *current* hub speed.
        state.tire.update(
            tire_cfg,
            state.longitudinal_velocity,
            state.lateral_velocity,
            state.angular_velocity,
            tire_cfg.radius,
            h,
        );
        state.forces = tire::evaluate(tire_cfg, &state.tire, state.normal_force);

        // 2. Torques acting on the hub.
        let drive = drive_torques[index];
        state.drive_torque = drive;
        let reaction = -state.forces.longitudinal * tire_cfg.radius;
        let rolling = tire::rolling_resistance_torque(
            tire_cfg,
            state.normal_force,
            state.longitudinal_velocity,
            tire_cfg.radius,
        ) * state.angular_velocity.signum();

        let command = brakes::compute(
            &cfg.brakes,
            wheel_cfg,
            input.brake,
            input.handbrake,
            state.tire.slip_ratio,
            abs_enabled,
        );
        state.brake_torque = command.torque;
        state.abs_active = command.abs_active;

        let inertia = (tire_cfg.inertia + reflected[index]).max(1.0e-3);
        let free_torque = drive + reaction - rolling;
        let brake_torque = brakes::limited_brake_torque(
            command.torque,
            state.angular_velocity,
            inertia,
            h,
            free_torque,
        );

        state.angular_velocity += (free_torque + brake_torque) / inertia * h;

        if !state.angular_velocity.is_finite() {
            state.angular_velocity = 0.0;
        }
    }

    // Keep the crank inside its range even when the wheels are wild.
    dynamics.engine.clamp(&cfg.engine);
    debug_assert!(rad_to_rpm(dynamics.engine.angular_velocity) >= 0.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::presets::bugatti_veyron_super_sport;
    use crate::vehicle::testbed::VehicleTestbed;

    #[test]
    fn ackermann_turns_the_inner_wheel_further() {
        let cfg = bugatti_veyron_super_sport();
        let (left, right) = steering_angles(&cfg, 1.0, 0.0);
        assert!(
            right > left,
            "turning right: inner (right) must exceed outer"
        );
        let (left, right) = steering_angles(&cfg, -1.0, 0.0);
        assert!(
            left.abs() > right.abs(),
            "turning left: inner is the left one"
        );
    }

    #[test]
    fn steering_authority_shrinks_with_speed() {
        let cfg = bugatti_veyron_super_sport();
        let (slow, _) = steering_angles(&cfg, 1.0, 0.0);
        let (fast, _) = steering_angles(&cfg, 1.0, 350.0 / 3.6);
        assert!(slow.abs() > fast.abs() * 8.0, "{slow} {fast}");
    }

    #[test]
    fn arcade_steering_is_stronger_only_below_100_kmh() {
        let cfg = bugatti_veyron_super_sport();
        let (_, parking_inner) = steering_angles(&cfg, 1.0, 0.0);
        assert!(parking_inner > cfg.steering.max_angle_at(0.0) * 1.28);

        let speed_100 = 100.0 / 3.6;
        assert_eq!(low_speed_arcade_blend(speed_100), 0.0);
        assert_eq!(low_speed_arcade_blend(200.0 / 3.6), 0.0);
    }

    #[test]
    fn straight_ahead_means_zero_steer() {
        let cfg = bugatti_veyron_super_sport();
        let (left, right) = steering_angles(&cfg, 0.0, 40.0);
        assert_eq!(left, 0.0);
        assert_eq!(right, 0.0);
    }

    #[test]
    fn brake_pedal_drives_the_car_backwards_after_reverse_is_selected() {
        let mut testbed = VehicleTestbed::new(bugatti_veyron_super_sport());
        let reverse_pedal = ControlInput {
            brake: 1.0,
            ..Default::default()
        };

        // Holding S at rest selects reverse after the normal safety delay.
        for _ in 0..(120 / 2) {
            testbed.step(&reverse_pedal, 1.0 / 120.0);
        }
        assert_eq!(testbed.dynamics.transmission.gear, -1);

        // The same held S input must now produce negative (backward) speed,
        // rather than applying the service brakes a second time.
        for _ in 0..120 {
            testbed.step(&reverse_pedal, 1.0 / 120.0);
        }
        assert!(
            testbed.speed_kmh() < -1.0,
            "reverse pedal did not move the car: {:.2} km/h",
            testbed.speed_kmh()
        );
    }

    #[test]
    fn throttle_selects_first_immediately_at_any_reverse_speed() {
        let cfg = bugatti_veyron_super_sport();
        let mut dynamics = VehicleDynamics::new(&cfg);
        dynamics.transmission.gear = -1;
        let throttle = ControlInput {
            throttle: 1.0,
            ..Default::default()
        };

        update_gear_direction(&mut dynamics, &throttle, -100.0 / 3.6, 1.0 / 120.0);

        assert_eq!(dynamics.transmission.gear, 1);
        assert_eq!(dynamics.transmission.shift_timer, 0.0);
    }

    #[test]
    fn nitro_selects_first_immediately_from_reverse() {
        let cfg = bugatti_veyron_super_sport();
        let mut dynamics = VehicleDynamics::new(&cfg);
        dynamics.transmission.gear = -1;
        let nitro = ControlInput {
            nitro: 1.0,
            ..Default::default()
        };

        update_gear_direction(&mut dynamics, &nitro, -80.0 / 3.6, 1.0 / 120.0);

        assert_eq!(dynamics.transmission.gear, 1);
    }

    #[test]
    fn nitro_only_pushes_a_grounded_forward_car() {
        let cfg = bugatti_veyron_super_sport();
        let mut dynamics = VehicleDynamics::new(&cfg);
        let nitro = ControlInput {
            nitro: 1.0,
            ..Default::default()
        };
        assert!(!nitro_can_push(&dynamics, &nitro, 20.0));

        dynamics.wheels[0].grounded = true;
        dynamics.wheels[1].grounded = true;
        assert!(nitro_can_push(&dynamics, &nitro, 20.0));
        dynamics.transmission.gear = -1;
        assert!(!nitro_can_push(&dynamics, &nitro, 20.0));
    }

    #[test]
    fn nitro_accelerates_far_harder_than_full_throttle() {
        let mut ordinary = VehicleTestbed::new(bugatti_veyron_super_sport());
        let mut boosted = VehicleTestbed::new(bugatti_veyron_super_sport());
        let throttle = ControlInput {
            throttle: 1.0,
            ..Default::default()
        };
        let nitro = ControlInput {
            throttle: 1.0,
            nitro: 1.0,
            ..Default::default()
        };
        for _ in 0..240 {
            ordinary.step(&throttle, 1.0 / 120.0);
            boosted.step(&nitro, 1.0 / 120.0);
        }

        assert!(
            boosted.speed_kmh() > ordinary.speed_kmh() + 100.0,
            "ordinary {:.1} km/h, nitro {:.1} km/h",
            ordinary.speed_kmh(),
            boosted.speed_kmh()
        );
    }

    #[test]
    fn triple_power_toggle_survives_vehicle_reset() {
        let cfg = bugatti_veyron_super_sport();
        let mut dynamics = VehicleDynamics::new(&cfg);

        assert!(!dynamics.triple_power_enabled());
        assert!(dynamics.toggle_triple_power());
        assert_eq!(dynamics.power_multiplier, TRIPLE_POWER_MULTIPLIER);

        dynamics.reset(&cfg);
        assert!(dynamics.triple_power_enabled());

        assert!(!dynamics.toggle_triple_power());
        assert_eq!(dynamics.power_multiplier, NORMAL_POWER_MULTIPLIER);
    }

    #[test]
    fn throttle_reverses_a_moving_car_back_into_forward_motion() {
        let mut testbed = VehicleTestbed::new(bugatti_veyron_super_sport());
        let reverse_pedal = ControlInput {
            brake: 1.0,
            ..Default::default()
        };
        for _ in 0..(120 * 3) {
            testbed.step(&reverse_pedal, 1.0 / 120.0);
        }
        assert_eq!(testbed.dynamics.transmission.gear, -1);
        assert!(
            testbed.speed_kmh() < -5.0,
            "{:.2} km/h",
            testbed.speed_kmh()
        );

        let throttle = ControlInput {
            throttle: 1.0,
            ..Default::default()
        };
        testbed.step(&throttle, 1.0 / 120.0);
        assert_eq!(testbed.dynamics.transmission.gear, 1);

        for _ in 0..(120 * 5) {
            testbed.step(&throttle, 1.0 / 120.0);
        }
        assert!(testbed.speed_kmh() > 1.0, "{:.2} km/h", testbed.speed_kmh());
    }
}
