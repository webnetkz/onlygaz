//! A tiny 6-DOF rigid body integrator used to exercise the vehicle solver
//! without a renderer or a physics engine.
//!
//! It is deliberately *not* a general purpose physics engine: it integrates one
//! free body under the forces the solver returns plus gravity, which is exactly
//! what Avian does to the car in the real game. Having it here means
//! `cargo test` can measure acceleration, braking and top speed deterministically
//! and in a fraction of a second.

use bevy::prelude::*;

use super::config::VehicleConfig;
use super::solver::{self, ChassisState, ControlInput, VehicleDynamics};
use super::wheels::{FlatGround, GroundProbe};

pub const GRAVITY: f32 = 9.81;

/// Free rigid body with a diagonal inertia tensor in its own local frame.
#[derive(Debug, Clone, Copy)]
pub struct RigidBodyState {
    /// World position of the vehicle local frame origin.
    pub translation: Vec3,
    pub rotation: Quat,
    /// Velocity of the centre of mass.
    pub linear_velocity: Vec3,
    /// Angular velocity in world space.
    pub angular_velocity: Vec3,
}

impl Default for RigidBodyState {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            linear_velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
        }
    }
}

/// Config + state + ground, ready to be stepped.
pub struct VehicleTestbed<P: GroundProbe = FlatGround> {
    pub config: VehicleConfig,
    pub dynamics: VehicleDynamics,
    pub body: RigidBodyState,
    pub ground: P,
    pub gravity: f32,
    /// Distance travelled along the initial heading [m].
    pub distance: f32,
    pub time: f32,
}

impl VehicleTestbed<FlatGround> {
    /// A car standing on a flat road at the origin, facing -Z.
    pub fn new(config: VehicleConfig) -> Self {
        let dynamics = VehicleDynamics::new(&config);
        let mut testbed = Self {
            config,
            dynamics,
            body: RigidBodyState::default(),
            ground: FlatGround { height: 0.0 },
            gravity: GRAVITY,
            distance: 0.0,
            time: 0.0,
        };
        testbed.settle();
        testbed
    }
}

impl<P: GroundProbe> VehicleTestbed<P> {
    /// Place the body at its natural ride height and let the suspension settle.
    pub fn settle(&mut self) {
        // A decent first guess: the anchor height minus the static sag.
        self.body.translation.y = 0.06;
        for _ in 0..600 {
            self.step(&ControlInput::default(), 1.0 / 240.0);
        }
        self.body.linear_velocity = Vec3::ZERO;
        self.body.angular_velocity = Vec3::ZERO;
        self.distance = 0.0;
        self.time = 0.0;
    }

    #[inline]
    pub fn center_of_mass(&self) -> Vec3 {
        self.body.translation + self.body.rotation * self.config.center_of_mass
    }

    #[inline]
    pub fn chassis(&self) -> ChassisState {
        ChassisState {
            translation: self.body.translation,
            rotation: self.body.rotation,
            linear_velocity: self.body.linear_velocity,
            angular_velocity: self.body.angular_velocity,
            center_of_mass: self.center_of_mass(),
        }
    }

    /// Signed forward speed [m/s].
    #[inline]
    pub fn speed(&self) -> f32 {
        self.body
            .linear_velocity
            .dot(self.body.rotation * Vec3::NEG_Z)
    }

    /// Forward speed in km/h.
    #[inline]
    pub fn speed_kmh(&self) -> f32 {
        self.speed() * 3.6
    }

    /// Advance the whole system by `dt`.
    pub fn step(&mut self, input: &ControlInput, dt: f32) {
        let chassis = self.chassis();
        let output = solver::step(
            &self.config,
            &mut self.dynamics,
            &chassis,
            input,
            &self.ground,
            dt,
        );

        let mass = self.config.mass;
        let com = chassis.center_of_mass;

        let mut force = Vec3::NEG_Y * (mass * self.gravity);
        let mut torque = Vec3::ZERO;
        for applied in output.iter() {
            force += applied.force;
            torque += (applied.point - com).cross(applied.force);
        }

        // Linear.
        let acceleration = force / mass;
        self.body.linear_velocity += acceleration * dt;

        // Angular, including the gyroscopic term so fast yaw does not leak
        // energy into roll.
        let inertia = self.config.inertia;
        let rotation = self.body.rotation;
        let local_omega = rotation.inverse() * self.body.angular_velocity;
        let local_torque = rotation.inverse() * torque;
        let angular_momentum = local_omega * inertia;
        let local_alpha = (local_torque - local_omega.cross(angular_momentum)) / inertia;
        self.body.angular_velocity += rotation * local_alpha * dt;

        // Integrate the pose about the centre of mass.
        let new_com = com + self.body.linear_velocity * dt;
        let spin = self.body.angular_velocity * dt;
        let new_rotation = if spin.length_squared() > 0.0 {
            (Quat::from_scaled_axis(spin) * self.body.rotation).normalize()
        } else {
            self.body.rotation
        };
        self.body.rotation = new_rotation;
        self.body.translation = new_com - new_rotation * self.config.center_of_mass;

        self.distance += self.body.linear_velocity.length() * dt;
        self.time += dt;
    }

    /// Run for `duration` seconds with a constant input, calling `observer`
    /// after every step.
    pub fn run<F: FnMut(&Self)>(
        &mut self,
        input: &ControlInput,
        duration: f32,
        dt: f32,
        mut observer: F,
    ) {
        let steps = (duration / dt).ceil() as u32;
        for _ in 0..steps {
            self.step(input, dt);
            observer(self);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::presets::bugatti_veyron_super_sport;

    #[test]
    fn a_parked_car_stays_parked_and_level() {
        let mut testbed = VehicleTestbed::new(bugatti_veyron_super_sport());
        testbed.run(&ControlInput::default(), 5.0, 1.0 / 120.0, |_| {});
        assert!(
            testbed.speed().abs() < 0.05,
            "parked car drifted at {} m/s",
            testbed.speed()
        );
        assert!(
            testbed.dynamics.rpm() > 500.0,
            "the engine stalled while parked: {} rpm",
            testbed.dynamics.rpm()
        );
        let up = testbed.body.rotation * Vec3::Y;
        assert!(up.dot(Vec3::Y) > 0.999, "car is not level: {up:?}");
        assert!(
            testbed.body.translation.y.is_finite() && testbed.body.translation.y.abs() < 0.5,
            "ride height {}",
            testbed.body.translation.y
        );
    }

    #[test]
    fn all_four_wheels_carry_the_weight() {
        let mut testbed = VehicleTestbed::new(bugatti_veyron_super_sport());
        testbed.run(&ControlInput::default(), 3.0, 1.0 / 120.0, |_| {});
        let total: f32 = testbed.dynamics.wheels.iter().map(|w| w.normal_force).sum();
        let weight = testbed.config.mass * GRAVITY;
        assert!(
            (total - weight).abs() < weight * 0.06,
            "wheel loads sum to {total:.0} N, weight is {weight:.0} N"
        );
        for wheel in &testbed.dynamics.wheels {
            assert!(wheel.grounded, "{:?} is not touching the road", wheel.id);
            assert!(wheel.normal_force > 1000.0);
        }
    }
}
