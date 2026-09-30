//! Physics visualisation, toggled with `F4`.
//!
//! Draws the body collider, the suspension ray casts, the contact points and the
//! three force vectors at each contact patch, plus the centre of mass. This is
//! the fastest way to see why a car is misbehaving.

use bevy::prelude::*;

use crate::vehicle::config::WheelId;
use crate::vehicle::physics::Vehicle;

/// Newtons per metre of drawn arrow.
const FORCE_SCALE: f32 = 1.0 / 3_000.0;

const COLOR_COLLIDER: Color = Color::srgb(0.35, 0.85, 1.0);
const COLOR_RAY: Color = Color::srgb(1.0, 0.85, 0.2);
const COLOR_CONTACT: Color = Color::srgb(1.0, 1.0, 1.0);
const COLOR_NORMAL: Color = Color::srgb(0.3, 1.0, 0.4);
const COLOR_TRACTION: Color = Color::srgb(1.0, 0.45, 0.2);
const COLOR_LATERAL: Color = Color::srgb(0.7, 0.4, 1.0);
const COLOR_COM: Color = Color::srgb(1.0, 0.2, 0.55);

/// Whether the gizmos are drawn. Toggled with `F4`.
#[derive(Resource, Default)]
pub struct PhysicsGizmoOverlay {
    pub visible: bool,
}

pub fn draw_physics_gizmos(
    overlay: Res<PhysicsGizmoOverlay>,
    mut gizmos: Gizmos,
    vehicles: Query<(&GlobalTransform, &Vehicle)>,
) {
    if !overlay.visible {
        return;
    }

    for (global, vehicle) in &vehicles {
        let transform = global.compute_transform();
        let config = &vehicle.config;

        // Body collider boxes.
        for (center, half_extents) in &config.collider_boxes {
            gizmos.cube(
                Transform {
                    translation: transform.transform_point(*center),
                    rotation: transform.rotation,
                    scale: *half_extents * 2.0,
                },
                COLOR_COLLIDER,
            );
        }

        // Centre of mass.
        let com = transform.transform_point(config.center_of_mass);
        gizmos.sphere(com, 0.07, COLOR_COM);
        gizmos.cross(com, 0.25, COLOR_COM);

        for id in WheelId::ALL {
            let wheel_cfg = config.wheel(id);
            let wheel = &vehicle.dynamics.wheels[id.index()];

            let anchor = transform.transform_point(wheel_cfg.anchor);
            let hub =
                transform.transform_point(wheel_cfg.anchor - Vec3::Y * wheel.suspension_length);

            // Suspension ray and travel.
            gizmos.line(anchor, hub, COLOR_RAY);
            gizmos.sphere(hub, 0.04, COLOR_RAY);
            gizmos.circle(
                Isometry3d::new(
                    hub,
                    transform.rotation
                        * Quat::from_rotation_y(-wheel.steer_angle)
                        * Quat::from_rotation_z(core::f32::consts::FRAC_PI_2),
                ),
                wheel_cfg.tire.radius,
                COLOR_RAY,
            );

            if !wheel.grounded {
                continue;
            }

            // Contact point and the three forces acting there.
            gizmos.sphere(wheel.contact_point, 0.05, COLOR_CONTACT);
            gizmos.arrow(
                wheel.contact_point,
                wheel.contact_point + wheel.contact_normal * (wheel.normal_force * FORCE_SCALE),
                COLOR_NORMAL,
            );

            let forward =
                transform.rotation * Quat::from_rotation_y(-wheel.steer_angle) * Vec3::NEG_Z;
            let right = forward.cross(wheel.contact_normal).normalize_or(Vec3::X);
            gizmos.arrow(
                wheel.contact_point,
                wheel.contact_point + forward * (wheel.forces.longitudinal * FORCE_SCALE),
                COLOR_TRACTION,
            );
            gizmos.arrow(
                wheel.contact_point,
                wheel.contact_point + right * (wheel.forces.lateral * FORCE_SCALE),
                COLOR_LATERAL,
            );
        }
    }
}
