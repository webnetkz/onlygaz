//! Steel W-beam guard rails down both edges of the road.
//!
//! Each 100 m section gets one merged mesh (beam plus posts) and one box
//! collider. Adjacent sections overlap slightly to avoid collision seams.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::mesh_builder::MeshBuilder;
use super::road::RoadSpec;

/// Height of the beam's centre above the road [m].
pub const BEAM_CENTER_Y: f32 = 0.60;
/// Vertical size of the beam [m].
pub const BEAM_HEIGHT: f32 = 0.34;
/// Thickness of the beam [m].
pub const BEAM_THICKNESS: f32 = 0.07;
/// Spacing between posts [m].
pub const POST_SPACING: f32 = 4.0;
/// Total height of the collider, generous enough that nothing hops over [m].
pub const COLLIDER_HEIGHT: f32 = 1.1;
/// Invisible containment plane sits just behind the visible steel beam [m].
pub const CONTAINMENT_CLEARANCE: f32 = 0.10;

/// Physical beam that should turn a glancing impact back toward the road.
/// The invisible containment wall is deliberately excluded.
#[derive(Component)]
pub struct RoadRail {
    pub side: f32,
}

/// Sliding steel should not consume all of the car's forward momentum during
/// a glancing impact. The rail owns the combination rule so the vehicle's
/// general-purpose body material does not dilute this response.
pub fn rail_friction() -> Friction {
    Friction::new(0.15).with_combine_rule(CoefficientCombine::Min)
}

/// A guard rail must return a meaningful lateral component even when the car
/// reaches it without throttle or nitro.
pub fn rail_restitution() -> Restitution {
    Restitution::new(0.35).with_combine_rule(CoefficientCombine::Max)
}

/// Merged beam and posts for one chunk of one side.
///
/// `side` is -1 for the driver's-right rail and +1 for the far one.
pub fn build_barrier_chunk(
    spec: &RoadSpec,
    side: f32,
    distance_start: f32,
    distance_end: f32,
) -> Option<Mesh> {
    let mut builder = MeshBuilder::new();
    let lateral = side * spec.barrier_offset;
    let midpoint = (distance_start + distance_end) * 0.5;
    let rotation = spec.frame_rotation(midpoint);
    let beam_start = spec.point_at(distance_start, lateral, BEAM_CENTER_Y);
    let beam_end = spec.point_at(distance_end, lateral, BEAM_CENTER_Y);
    let beam_center = (beam_start + beam_end) * 0.5;
    let beam_half_length = beam_start.distance(beam_end) * 0.5;

    builder.box_transformed(
        beam_center,
        Vec3::new(BEAM_THICKNESS * 0.5, BEAM_HEIGHT * 0.5, beam_half_length),
        rotation,
    );

    // A second, thinner rib gives the beam its characteristic profile without
    // paying for real W-beam geometry.
    builder.box_transformed(
        beam_center - spec.left_at(midpoint) * side * BEAM_THICKNESS * 0.4,
        Vec3::new(BEAM_THICKNESS * 0.25, BEAM_HEIGHT * 0.16, beam_half_length),
        rotation,
    );

    let mut distance = (distance_start / POST_SPACING).ceil() * POST_SPACING;
    while distance < distance_end {
        builder.box_transformed(
            spec.point_at(distance, lateral, BEAM_CENTER_Y * 0.5),
            Vec3::new(0.055, BEAM_CENTER_Y * 0.5 + BEAM_HEIGHT * 0.3, 0.055),
            spec.frame_rotation(distance),
        );
        distance += POST_SPACING;
    }

    builder.build()
}

/// Collider for one chunk of one side.
pub fn barrier_collider(
    spec: &RoadSpec,
    side: f32,
    distance_start: f32,
    distance_end: f32,
) -> (Collider, Transform) {
    let lateral = side * spec.barrier_offset;
    let start = spec.point_at(distance_start, lateral, COLLIDER_HEIGHT * 0.5);
    let end = spec.point_at(distance_end, lateral, COLLIDER_HEIGHT * 0.5);
    let midpoint = (distance_start + distance_end) * 0.5;
    let collider = Collider::cuboid(0.12, COLLIDER_HEIGHT, start.distance(end) + 0.3);
    let transform = Transform::from_translation((start + end) * 0.5)
        .with_rotation(spec.frame_rotation(midpoint));
    (collider, transform)
}

/// Seamless, infinitely tall backstop immediately outside a guard rail.
///
/// The visible rail keeps its realistic collision height and response. This
/// second plane only catches extreme impacts that would otherwise launch a car
/// over it.
pub fn containment_collider(spec: &RoadSpec, side: f32) -> (Collider, Transform) {
    debug_assert!(side == -1.0 || side == 1.0);
    let lateral = side * (spec.barrier_offset + BEAM_THICKNESS * 0.5 + CONTAINMENT_CLEARANCE);
    let inward_normal = Vec3::X * -side;
    (
        Collider::half_space(inward_normal),
        Transform::from_translation(spec.point_at(0.0, lateral, spec.surface_y)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn barriers_sit_outside_the_paved_surface() {
        let spec = RoadSpec::default();
        assert!(spec.barrier_offset > spec.paved_half_width());
    }

    #[test]
    fn a_chunk_has_a_beam_and_posts() {
        let spec = RoadSpec::default();
        let mesh = build_barrier_chunk(&spec, -1.0, 0.0, 100.0).expect("mesh");
        // Two beam boxes plus 25 posts, 24 vertices each.
        assert!(mesh.count_vertices() >= (2 + 25) * 24 - 24);
    }

    #[test]
    fn collider_spans_the_chunk() {
        let spec = RoadSpec::default();
        let (_, transform) = barrier_collider(&spec, 1.0, 100.0, 200.0);
        let (distance, lateral) = spec.local_coordinates(transform.translation);
        assert!((distance - 150.0).abs() < 0.01);
        assert!((lateral - spec.barrier_offset).abs() < 0.1);
        assert!((transform.rotation * Vec3::Z).dot(spec.tangent_at(distance)) > 0.9999);
    }

    #[test]
    fn containment_planes_face_inward_and_sit_behind_the_rails() {
        let spec = RoadSpec::default();
        for side in [-1.0f32, 1.0] {
            let (_, transform) = containment_collider(&spec, side);
            assert!(transform.translation.x.abs() > spec.barrier_offset);
            assert!(transform.translation.x.abs() < spec.protected_surface_half_width());
        }
    }
}
