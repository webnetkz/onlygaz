//! Active rear wing for the Bugatti Veyron.
//!
//! The supplied Bugatti scene contains the complete `winga` assembly as
//! separate meshes.  At load time those meshes are re-parented under two
//! pivots, allowing the original bodywork and hydraulic linkage to move rather
//! than drawing a replacement wing on top of the car.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::VehicleKind;
use super::model_rig::SceneNodes;
use super::physics::{Vehicle, VehicleModelRoot};

/// Automatic Veyron handling mode begins above this road speed.
pub const DEPLOY_SPEED_KMH: f32 = 220.0;
/// Hysteresis prevents the wing repeatedly opening and closing near 220 km/h.
pub const RETRACT_SPEED_KMH: f32 = 140.0;
/// The real airbrake is available above 200 km/h while handling mode is active.
pub const AIR_BRAKE_SPEED_KMH: f32 = 200.0;
const AIR_BRAKE_PEDAL: f32 = 0.55;

const DEPLOY_RATE: f32 = 0.92;
const RETRACT_RATE: f32 = 0.68;
/// 1 / 0.4 s: the production car reaches its airbrake position in 0.4 seconds.
const AIR_BRAKE_RATE: f32 = 2.5;
const AIR_BRAKE_RETURN_RATE: f32 = 1.65;

const WING_RISE_METRES: f32 = 0.22;
const LINKAGE_RISE_METRES: f32 = 0.10;
const WING_REAR_TRAVEL_METRES: f32 = 0.035;
const HANDLING_ANGLE_RADIANS: f32 = 12.0_f32.to_radians();
// Bugatti specifies 113 degrees to the direction of travel. In the model the
// retracted blade is not parallel to that direction, so roughly 55 degrees of
// pivot travel reproduces the specified final attitude.
const AIR_BRAKE_TRAVEL_RADIANS: f32 = 55.0_f32.to_radians();

/// Runtime hydraulic state. This lives on the car, so visual systems derive
/// the same smooth animation from their interpolated speed and brake input.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ActiveSpoiler {
    pub extension: f32,
    pub air_brake: f32,
    pub wing_detached: bool,
    handling_latched: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisualPart {
    Wing,
    Linkage,
}

/// A pivot in the glTF scene carrying either the aerofoil or its mechanism.
#[derive(Component)]
pub struct SpoilerVisualPivot {
    vehicle: Entity,
    base_translation: Vec3,
    part: VisualPart,
}

#[inline]
fn move_towards(current: f32, target: f32, max_delta: f32) -> f32 {
    current + (target - current).clamp(-max_delta, max_delta)
}

#[inline]
fn controller_targets(speed_kmh: f32, brake: f32, was_handling: bool) -> (bool, bool) {
    let handling = if speed_kmh >= DEPLOY_SPEED_KMH {
        true
    } else if speed_kmh <= RETRACT_SPEED_KMH {
        false
    } else {
        was_handling
    };
    let air_brake =
        handling && speed_kmh >= AIR_BRAKE_SPEED_KMH && brake.clamp(0.0, 1.0) >= AIR_BRAKE_PEDAL;
    (handling, air_brake)
}

/// Advance the hydraulic controller from road speed and brake pressure.
pub fn update_active_spoiler_state(
    time: Res<Time>,
    mut spoilers: Query<(&Vehicle, &mut ActiveSpoiler)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    for (vehicle, mut spoiler) in &mut spoilers {
        let speed = vehicle.speed_kmh().abs();
        let (handling, air_brake_target) =
            controller_targets(speed, vehicle.input.brake, spoiler.handling_latched);
        spoiler.handling_latched = handling;
        let extension_target = spoiler.handling_latched || air_brake_target;
        let extension_rate = if extension_target {
            DEPLOY_RATE
        } else {
            RETRACT_RATE
        };
        spoiler.extension = move_towards(
            spoiler.extension,
            f32::from(extension_target),
            extension_rate * dt,
        );
        let brake_rate = if air_brake_target {
            AIR_BRAKE_RATE
        } else {
            AIR_BRAKE_RETURN_RATE
        };
        spoiler.air_brake = move_towards(
            spoiler.air_brake,
            f32::from(air_brake_target),
            brake_rate * dt,
        );
    }
}

/// Move the original glTF meshes immediately before transform propagation.
pub fn animate_active_spoiler(
    spoilers: Query<&ActiveSpoiler>,
    mut pivots: Query<
        (&SpoilerVisualPivot, &mut Transform, &mut Visibility),
        Without<super::damage::DetachedDebris>,
    >,
) {
    for (pivot, mut transform, mut visibility) in &mut pivots {
        let Ok(spoiler) = spoilers.get(pivot.vehicle) else {
            continue;
        };
        *visibility = if spoiler.wing_detached {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        match pivot.part {
            VisualPart::Wing => {
                transform.translation = pivot.base_translation
                    + Vec3::new(
                        0.0,
                        WING_RISE_METRES * spoiler.extension,
                        -WING_REAR_TRAVEL_METRES * spoiler.extension,
                    );
                let handling = HANDLING_ANGLE_RADIANS * spoiler.extension;
                let angle = handling + (AIR_BRAKE_TRAVEL_RADIANS - handling) * spoiler.air_brake;
                transform.rotation = Quat::from_rotation_x(angle);
            }
            VisualPart::Linkage => {
                transform.translation =
                    pivot.base_translation + Vec3::Y * (LINKAGE_RISE_METRES * spoiler.extension);
                transform.rotation = Quat::IDENTITY;
            }
        }
    }
}

/// Add the drag and rear-axle load created by the deployed wing. The existing
/// base aerodynamic model remains untouched; this is only the active increment.
pub fn apply_active_spoiler_aero(mut query: Query<(&Vehicle, &ActiveSpoiler, Forces)>) {
    for (vehicle, spoiler, mut forces) in &mut query {
        if spoiler.wing_detached {
            continue;
        }
        let velocity = forces.linear_velocity();
        let speed = velocity.length();
        if speed < 1.0 || (spoiler.extension <= 0.0 && spoiler.air_brake <= 0.0) {
            continue;
        }

        let rotation = forces.rotation().0;
        let speed_factor = (speed / (AIR_BRAKE_SPEED_KMH / 3.6))
            .powi(2)
            .clamp(0.0, 1.0);
        let downforce = speed_factor * (1_150.0 * spoiler.extension + 2_943.0 * spoiler.air_brake);
        let extra_drag = speed_factor
            * (420.0 * spoiler.extension + vehicle.config.mass * 2.5 * spoiler.air_brake);

        let rear_point = forces.position().0 + rotation * Vec3::new(0.0, 0.85, 1.55);
        forces.apply_force_at_point(-(rotation * Vec3::Y) * downforce, rear_point);
        forces.apply_force_at_point(
            -velocity.normalize_or_zero() * extra_drag,
            forces.position().0,
        );
    }
}

fn part_for_name(name: &str) -> Option<VisualPart> {
    let lower = name.to_ascii_lowercase();
    if !lower.contains("_winga_") {
        return None;
    }
    if lower.contains("winga_body") || lower.contains("winga_metal_") {
        Some(VisualPart::Wing)
    } else {
        Some(VisualPart::Linkage)
    }
}

/// Re-parent every authored Veyron wing mesh under animation pivots while
/// preserving its world-relative transform.
pub(crate) fn rig_veyron_spoiler(
    commands: &mut Commands,
    root: Entity,
    model_root: &VehicleModelRoot,
    scene: &SceneNodes,
) {
    if model_root.kind != VehicleKind::Veyron {
        return;
    }

    let mut assignments: Vec<(Entity, VisualPart)> = scene
        .descendants(root)
        .filter_map(|entity| {
            scene
                .name(entity)
                .and_then(part_for_name)
                .map(|part| (entity, part))
        })
        .collect();
    let matched: Vec<Entity> = assignments.iter().map(|(entity, _)| *entity).collect();
    assignments.retain(|(entity, _)| !scene.has_matched_ancestor(*entity, root, &matched));
    if assignments.is_empty() {
        warn!("Bugatti active spoiler meshes were not found in the model");
        return;
    }

    let bounds_for = |part| {
        let mut bounds: Option<(Vec3, Vec3)> = None;
        for (entity, candidate) in &assignments {
            if *candidate != part {
                continue;
            }
            for target in core::iter::once(*entity).chain(scene.descendants(*entity)) {
                let Some((min, max)) = scene.bounds_relative_to(target, root) else {
                    continue;
                };
                bounds = Some(match bounds {
                    Some((old_min, old_max)) => (old_min.min(min), old_max.max(max)),
                    None => (min, max),
                });
            }
        }
        bounds
    };

    let wing_bounds = bounds_for(VisualPart::Wing);
    let linkage_bounds = bounds_for(VisualPart::Linkage);
    let pivot_position = |part| match part {
        // Hinge the blade around its forward/lower edge instead of its centre.
        VisualPart::Wing => wing_bounds
            .map(|(min, max)| Vec3::new((min.x + max.x) * 0.5, min.y, max.z))
            .unwrap_or(Vec3::new(0.0, 0.88, -1.69)),
        VisualPart::Linkage => linkage_bounds
            .map(|(min, max)| (min + max) * 0.5)
            .unwrap_or(Vec3::new(0.0, 0.67, -1.73)),
    };

    let spawn_pivot = |commands: &mut Commands, part: VisualPart| {
        let base = pivot_position(part);
        commands
            .spawn((
                Name::new(match part {
                    VisualPart::Wing => "BugattiActiveWingPivot",
                    VisualPart::Linkage => "BugattiWingLinkagePivot",
                }),
                SpoilerVisualPivot {
                    vehicle: model_root.vehicle,
                    base_translation: base,
                    part,
                },
                Transform::from_translation(base),
                Visibility::default(),
                ChildOf(root),
            ))
            .id()
    };
    let wing_pivot = spawn_pivot(commands, VisualPart::Wing);
    let linkage_pivot = spawn_pivot(commands, VisualPart::Linkage);
    let wing_base = pivot_position(VisualPart::Wing);
    let wing_size = wing_bounds
        .map(|(min, max)| max - min)
        .unwrap_or(Vec3::new(1.8, 0.12, 0.4));
    commands.entity(wing_pivot).insert(super::damage::wing_part(
        model_root, root, wing_base, wing_size,
    ));

    let mut moved = 0usize;
    for (entity, part) in assignments {
        let Some(matrix) = scene.transform_relative_to(entity, root) else {
            continue;
        };
        let (pivot, position) = match part {
            VisualPart::Wing => (wing_pivot, pivot_position(VisualPart::Wing)),
            VisualPart::Linkage => (linkage_pivot, pivot_position(VisualPart::Linkage)),
        };
        let rebased = Mat4::from_translation(-position) * matrix;
        let (scale, rotation, translation) = rebased.to_scale_rotation_translation();
        commands.entity(entity).insert((
            Transform {
                translation,
                rotation,
                scale,
            },
            ChildOf(pivot),
        ));
        moved += 1;
    }
    info!("Bugatti active spoiler: {moved} authored mesh groups rigged");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_the_authored_bugatti_wing_parts() {
        assert_eq!(
            part_for_name("bug_veyron_09_s_lod0_winga_body_"),
            Some(VisualPart::Wing)
        );
        assert_eq!(
            part_for_name("bug_veyron_09_s_lod0_winga_piston_metal_"),
            Some(VisualPart::Linkage)
        );
        assert_eq!(part_for_name("bug_veyron_09_body_door_"), None);
    }

    #[test]
    fn hydraulic_motion_is_frame_rate_independent_in_duration() {
        let mut extension = 0.0;
        for _ in 0..60 {
            extension = move_towards(extension, 1.0, DEPLOY_RATE / 60.0);
        }
        assert!((extension - DEPLOY_RATE).abs() < 1.0e-5);

        let mut air_brake = 0.0;
        for _ in 0..24 {
            air_brake = move_towards(air_brake, 1.0, AIR_BRAKE_RATE / 60.0);
        }
        assert!((air_brake - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn automatic_controller_uses_real_speed_hysteresis_and_airbrake_gate() {
        assert_eq!(controller_targets(219.9, 0.0, false), (false, false));
        assert_eq!(controller_targets(220.0, 0.0, false), (true, false));
        assert_eq!(controller_targets(180.0, 0.0, true), (true, false));
        assert_eq!(controller_targets(140.0, 1.0, true), (false, false));
        assert_eq!(controller_targets(250.0, 0.54, true), (true, false));
        assert_eq!(controller_targets(250.0, 0.55, true), (true, true));
    }
}
