//! Bridge between the pure [`solver`](super::solver) and Avian's rigid bodies.
//!
//! The car is a *single* dynamic rigid body. Everything that makes it a car -
//! suspension, tyres, engine, gearbox - is expressed as forces applied to that
//! body at specific world points. The body's [`Transform`] is never written by
//! gameplay code; it is a pure output of the physics solver.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::VehicleKind;
use super::config::{VehicleConfig, WheelId};
use super::solver::{self, ChassisState, ControlInput, VehicleDynamics};
use super::wheels::{GroundHit, GroundProbe};

/// The car. Owns its own configuration so several different cars can coexist.
#[derive(Component)]
pub struct Vehicle {
    pub config: VehicleConfig,
    pub dynamics: VehicleDynamics,
    pub input: ControlInput,
    /// Where `R` puts the car back to if the road spec cannot be consulted.
    pub home: Transform,
    /// Monotonic reset counter used to repair detached and deformed parts.
    pub reset_generation: u32,
}

impl Vehicle {
    pub fn new(config: VehicleConfig, home: Transform) -> Self {
        let dynamics = VehicleDynamics::new(&config);
        Self {
            config,
            dynamics,
            input: ControlInput::default(),
            home,
            reset_generation: 0,
        }
    }

    /// Forward speed in km/h, signed.
    pub fn speed_kmh(&self) -> f32 {
        self.dynamics.forward_speed * 3.6
    }

    /// Effective nitro level after the safety conditions shared by physics and
    /// visuals. Keeping the flame and blur on this value means they never claim
    /// thrust is active while the car is airborne or still in reverse.
    pub fn nitro_strength(&self) -> f32 {
        let grounded = self
            .dynamics
            .wheels
            .iter()
            .filter(|wheel| wheel.grounded)
            .count();
        if self.dynamics.transmission.gear >= 0
            && self.dynamics.forward_speed >= -1.0
            && grounded >= 2
        {
            self.input.nitro.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// Root entity of the visual model, a child of the rigid body.
#[derive(Component)]
pub struct VehicleModelRoot {
    pub vehicle: Entity,
    pub kind: VehicleKind,
    /// Simulation origin expressed in the source model's local coordinates.
    pub model_origin: Vec3,
    /// Uniform source-model scale applied by the visual root.
    pub model_scale: f32,
    /// Source-model yaw applied by the visual root.
    pub model_yaw: f32,
}

/// Per-wheel pivot carrying suspension travel and steering. Brake calipers hang
/// off this one because they do not spin with the wheel.
#[derive(Component)]
pub struct WheelHubPivot {
    pub vehicle: Entity,
    pub id: WheelId,
    pub model_origin: Vec3,
    pub model_scale: f32,
    pub model_yaw: f32,
}

/// Child of [`WheelHubPivot`] that spins with the wheel.
#[derive(Component)]
pub struct WheelSpinPivot {
    pub vehicle: Entity,
    pub id: WheelId,
}

/// Pivot for the steering wheel inside the cockpit.
#[derive(Component)]
pub struct SteeringWheelPivot {
    pub vehicle: Entity,
    pub model_origin: Vec3,
    pub model_scale: f32,
    pub model_yaw: f32,
    /// Display-only steering value. It never feeds back into the vehicle input,
    /// road-wheel angles or the physics solver.
    pub visual_steering: f32,
}

/// Response rate of the steering-wheel animation [1/s]. The road wheels keep
/// using the existing input and return rates from `SteeringConfig`.
const STEERING_WHEEL_VISUAL_RESPONSE: f32 = 9.0;

/// Ground probe backed by Avian's spatial query acceleration structure.
struct AvianGround<'a, 'w, 's> {
    query: &'a SpatialQuery<'w, 's>,
    filter: SpatialQueryFilter,
}

impl GroundProbe for AvianGround<'_, '_, '_> {
    fn cast(&self, origin: Vec3, direction: Dir3, max_distance: f32) -> Option<GroundHit> {
        self.query
            .cast_ray(origin, direction, max_distance, true, &self.filter)
            .map(|hit| GroundHit {
                distance: hit.distance,
                normal: hit.normal,
                point: origin + *direction * hit.distance,
            })
    }
}

/// Build the rigid body for a car and return its entity.
///
/// The body collider is a compound of simple convex boxes - never the visual
/// mesh - and the mass properties are set explicitly so that the centre of mass
/// sits low, which is what keeps the car settled at 400 km/h.
pub fn spawn_vehicle(commands: &mut Commands, config: VehicleConfig, pose: Transform) -> Entity {
    let collider = Collider::compound(
        config
            .collider_boxes
            .iter()
            .map(|(center, half_extents)| {
                (
                    *center,
                    Quat::IDENTITY,
                    Collider::cuboid(
                        half_extents.x * 2.0,
                        half_extents.y * 2.0,
                        half_extents.z * 2.0,
                    ),
                )
            })
            .collect::<Vec<_>>(),
    );

    let mass = config.mass;
    let inertia = config.inertia;
    let center_of_mass = config.center_of_mass;

    commands
        .spawn((
            Name::new("Vehicle"),
            VehicleTag,
            Vehicle::new(config, pose),
            super::controller::DriverInput::default(),
            pose,
            Visibility::default(),
            (
                RigidBody::Dynamic,
                collider,
                CollisionLayers::from_bits(1, !super::damage::DEBRIS_LAYER),
            ),
            // Explicit mass properties: the colliders must not be allowed to
            // guess them, or the centre of mass would sit at the box centroid.
            (
                Mass(mass),
                AngularInertia::new(inertia),
                CenterOfMass(center_of_mass),
                NoAutoMass,
                NoAutoAngularInertia,
                NoAutoCenterOfMass,
            ),
            // A little angular damping stands in for the yaw damping of a real
            // car's compliance; linear damping stays at zero because drag is
            // modelled properly in `aero`.
            (
                LinearDamping(0.0),
                AngularDamping(0.15),
                Friction::new(0.4),
                Restitution::new(0.05),
                SweptCcd::default(),
                SleepingDisabled,
                // The camera impact effect listens only to the player body's
                // contact-start messages and derives severity from the solved
                // normal impulse.
                CollisionEventsEnabled,
                // Physics runs at 120 Hz and the screen usually does not.
                // Without interpolation the car's transform only changes on
                // physics ticks, and every camera rigidly attached to it - the
                // bumper, bonnet and cockpit views especially - inherits that
                // stutter.
                TransformInterpolation,
            ),
        ))
        .id()
}

/// Marker so other modules can find the player car without knowing its type.
#[derive(Component)]
pub struct VehicleTag;

/// Keep the normal component of a glancing impact from being consumed by tire
/// forces while the body is still scraping along a steel guard rail. The
/// contact graph limits this to genuine rail contacts; proximity alone must
/// not tug a car that is driving along the shoulder.
pub fn deflect_from_road_rail(
    collisions: Collisions,
    rails: Query<&crate::world::barriers::RoadRail>,
    mut vehicles: Query<(Entity, &mut LinearVelocity), With<VehicleTag>>,
) {
    for (entity, mut velocity) in &mut vehicles {
        let along_rail = velocity.z.abs();
        if along_rail < 2.0 {
            continue;
        }
        for contact in collisions.collisions_with(entity) {
            let other = if contact.collider1 == entity {
                contact.collider2
            } else {
                contact.collider1
            };
            let Ok(rail) = rails.get(other) else {
                continue;
            };
            let inward = -rail.side;
            let minimum_rebound = (along_rail * 0.10).clamp(1.5, 4.5);
            if velocity.x * inward < minimum_rebound {
                velocity.x = inward * minimum_rebound;
            }
        }
    }
}

/// Emergency depth below the mathematically flat road at which suspension
/// rays can no longer recover the chassis on their own. Normal suspension and
/// collider penetration are only millimetres, so this never participates in
/// ordinary driving or jumps.
const ROAD_ESCAPE_DEPTH: f32 = 0.08;
const ROAD_RECOVERY_HEIGHT: f32 = 0.04;

/// Keep an escaped chassis above the continuous road plane without touching
/// its longitudinal/lateral velocity. This is a final numerical safety net for
/// 800+ km/h; swept CCD and the suspension remain the normal contact path.
pub fn prevent_road_tunneling(
    road: Res<crate::world::road::RoadSpec>,
    mut vehicles: Query<(&mut Position, &mut LinearVelocity), With<VehicleTag>>,
) {
    for (mut position, mut velocity) in &mut vehicles {
        if rescue_road_height(&mut position.0, &mut velocity.0, road.surface_y) {
            warn!("vehicle road safety net corrected a below-surface physics escape");
        }
    }
}

fn rescue_road_height(position: &mut Vec3, velocity: &mut Vec3, surface_y: f32) -> bool {
    if !position.y.is_finite() || position.y >= surface_y - ROAD_ESCAPE_DEPTH {
        return false;
    }
    position.y = surface_y + ROAD_RECOVERY_HEIGHT;
    if velocity.y < 0.0 || !velocity.y.is_finite() {
        velocity.y = 0.0;
    }
    true
}

/// The main vehicle system. Runs inside Avian's [`PhysicsSchedule`] so that the
/// forces it accumulates are consumed by the very next solver step.
pub fn vehicle_physics(
    time: Res<Time>,
    spatial: SpatialQuery,
    mut query: Query<(Entity, &mut Vehicle, Forces, &ComputedCenterOfMass)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    for (entity, mut vehicle, mut forces, com) in &mut query {
        let position = forces.position().0;
        let rotation = forces.rotation().0;
        let chassis = ChassisState {
            translation: position,
            rotation,
            linear_velocity: forces.linear_velocity(),
            angular_velocity: forces.angular_velocity(),
            center_of_mass: position + rotation * com.0,
        };

        let ground = AvianGround {
            query: &spatial,
            filter: SpatialQueryFilter::from_mask(!super::damage::DEBRIS_LAYER)
                .with_excluded_entities([entity]),
        };

        let input = vehicle.input;
        let Vehicle {
            config, dynamics, ..
        } = &mut *vehicle;
        let output = solver::step(config, dynamics, &chassis, &input, &ground, dt);

        for applied in output.iter() {
            forces.apply_force_at_point(applied.force, applied.point);
        }
    }
}

/// Query for the cockpit steering wheel pivot, disjoint from the wheel pivots.
type SteeringPivotQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut SteeringWheelPivot, &'static mut Transform),
    (Without<WheelHubPivot>, Without<WheelSpinPivot>),
>;

/// Move the visual wheel and steering wheel pivots to match the simulation.
///
/// The pivots live in the *model's* coordinate system, which is Y-up with +Z
/// forward, i.e. the vehicle frame rotated 180 degrees about Y. Converting a
/// vehicle-local point is therefore just negating X and Z.
pub fn update_visual_rig(
    time: Res<Time>,
    vehicles: Query<&Vehicle>,
    mut hubs: Query<
        (&WheelHubPivot, &mut Transform),
        (
            Without<WheelSpinPivot>,
            Without<super::damage::DetachedDebris>,
        ),
    >,
    mut spins: Query<(&WheelSpinPivot, &mut Transform), Without<WheelHubPivot>>,
    mut steering: SteeringPivotQuery,
) {
    for (pivot, mut transform) in &mut hubs {
        let Ok(vehicle) = vehicles.get(pivot.vehicle) else {
            continue;
        };
        let wheel_cfg = vehicle.config.wheel(pivot.id);
        let state = &vehicle.dynamics.wheels[pivot.id.index()];
        let hub = wheel_cfg.anchor - Vec3::Y * state.suspension_length;
        transform.translation =
            pivot.model_origin + model_space_for(hub, pivot.model_scale, pivot.model_yaw);
        // A rotation about Y is unaffected by the 180 degree model flip, so the
        // steering angle carries over unchanged.
        transform.rotation = Quat::from_rotation_y(-state.steer_angle);
    }

    for (pivot, mut transform) in &mut spins {
        let Ok(vehicle) = vehicles.get(pivot.vehicle) else {
            continue;
        };
        if vehicle.dynamics.detached_wheels[pivot.id.index()] {
            continue;
        }
        let state = &vehicle.dynamics.wheels[pivot.id.index()];
        transform.rotation = Quat::from_rotation_x(state.spin_angle);
    }

    let dt = time.delta_secs();
    for (mut pivot, mut transform) in &mut steering {
        let Ok(vehicle) = vehicles.get(pivot.vehicle) else {
            continue;
        };
        let axis = model_space_for(vehicle.config.steering_wheel_axis, 1.0, pivot.model_yaw)
            .normalize_or(Vec3::Y);
        pivot.visual_steering =
            smooth_visual_steering(pivot.visual_steering, vehicle.input.steering, dt);
        let angle = -pivot.visual_steering * vehicle.config.steering.steering_wheel_rotation;
        transform.translation = pivot.model_origin
            + model_space_for(
                vehicle.config.steering_wheel_center,
                pivot.model_scale,
                pivot.model_yaw,
            );
        transform.rotation = Quat::from_axis_angle(axis, angle);
    }
}

/// Frame-rate-independent smoothing used only by the steering-wheel mesh.
#[inline]
fn smooth_visual_steering(current: f32, target: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        return current;
    }
    let blend = 1.0 - (-STEERING_WHEEL_VISUAL_RESPONSE * dt).exp();
    current + (target - current) * blend
}

/// Convert a vehicle-local point into the glTF model's own frame.
#[inline]
pub fn model_space(local: Vec3) -> Vec3 {
    Vec3::new(-local.x, local.y, -local.z)
}

/// Convert metres in the simulation frame into a particular source model.
#[inline]
pub fn model_space_for(local: Vec3, scale: f32, yaw: f32) -> Vec3 {
    Quat::from_rotation_y(-yaw) * local / scale
}

/// Teleport the car back onto the road, cleanly.
pub fn respawn(commands: &mut Commands, entity: Entity, vehicle: &mut Vehicle, pose: Transform) {
    vehicle.dynamics.reset(&vehicle.config);
    vehicle.input = ControlInput::default();
    vehicle.reset_generation = vehicle.reset_generation.wrapping_add(1);
    commands.entity(entity).insert((
        Position(pose.translation),
        Rotation(pose.rotation),
        LinearVelocity(Vec3::ZERO),
        AngularVelocity(Vec3::ZERO),
        pose,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visual_steering_moves_smoothly_without_changing_the_target() {
        let first = smooth_visual_steering(0.0, 1.0, 1.0 / 60.0);
        assert!(first > 0.0 && first < 1.0);

        let mut visual = 0.0;
        for _ in 0..120 {
            visual = smooth_visual_steering(visual, 1.0, 1.0 / 60.0);
        }
        assert!((visual - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn road_safety_net_preserves_800_kmh_horizontal_motion() {
        let mut position = Vec3::new(-1.8, -0.42, 12_000.0);
        let mut velocity = Vec3::new(3.0, -90.0, -800.0 / 3.6);
        assert!(rescue_road_height(&mut position, &mut velocity, 0.0));
        assert!((position.y - ROAD_RECOVERY_HEIGHT).abs() < f32::EPSILON);
        assert_eq!(velocity.x, 3.0);
        assert!((velocity.z + 800.0 / 3.6).abs() < 1.0e-5);
        assert_eq!(velocity.y, 0.0);
    }

    #[test]
    fn road_safety_net_never_modifies_normal_suspension_travel() {
        let mut position = Vec3::new(-1.8, -0.02, 100.0);
        let mut velocity = Vec3::new(0.0, -1.0, -50.0);
        assert!(!rescue_road_height(&mut position, &mut velocity, 0.0));
        assert_eq!(position.y, -0.02);
        assert_eq!(velocity.y, -1.0);
    }

    #[test]
    fn road_safety_system_corrects_the_live_avian_position_components() {
        let mut app = App::new();
        app.insert_resource(crate::world::road::RoadSpec::default())
            .add_systems(Update, prevent_road_tunneling);
        let entity = app
            .world_mut()
            .spawn((
                VehicleTag,
                Position(Vec3::new(-1.8, -1.2, 8_000.0)),
                LinearVelocity(Vec3::new(0.0, -60.0, -1_000.0 / 3.6)),
            ))
            .id();
        app.update();

        let position = app.world().get::<Position>(entity).unwrap().0;
        let velocity = app.world().get::<LinearVelocity>(entity).unwrap().0;
        assert!((position.y - ROAD_RECOVERY_HEIGHT).abs() < f32::EPSILON);
        assert!((velocity.z + 1_000.0 / 3.6).abs() < 1.0e-5);
        assert_eq!(velocity.y, 0.0);
    }
}
