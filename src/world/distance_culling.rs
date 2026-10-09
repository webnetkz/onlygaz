//! Distance culling of everything beyond the selected view distance.
//!
//! Bevy's CPU frustum culling deliberately ignores the far plane, so without
//! this every road chunk, guard rail and lamp along the 33 km loop that lies
//! inside the camera's field of view is submitted each frame - thousands of
//! draw calls whose pixels the GPU then clips away at the far plane. A phone
//! cannot afford that, and a desktop gains nothing from it either.
//!
//! Every mesh gets an abrupt [`VisibilityRange`] (no cross-fade, so no extra
//! shader work) ending at the view distance plus the mesh's own bounding
//! radius. Anything with a single pixel left in front of the far plane is
//! therefore still drawn: the picture is exactly the same, only the work for
//! invisible meshes disappears.

use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;

use super::lighting::LightingSettings;

/// Furthest any mirror camera looks [m] (see `camera::mirrors`).
const MIRROR_FAR: f32 = 1_500.0;
/// Bounding radii are rounded up to this step [m], so the many meshes share a
/// few distinct ranges.
const RADIUS_STEP: f32 = 25.0;

/// Bounding radius of a distance-culled mesh [m], rounded up to
/// [`RADIUS_STEP`].
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct DistanceCulled {
    pub radius: f32,
}

/// Opt-out for a mesh that must be drawn regardless of distance.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct NoDistanceCulling;

pub struct DistanceCullingPlugin;

impl Plugin for DistanceCullingPlugin {
    fn build(&self, app: &mut App) {
        // After `calculate_bounds` (PostUpdate) has given new meshes their
        // bounds; the range then applies from the following frame on.
        app.add_systems(Update, (cull_new_meshes, follow_view_distance).chain());
    }
}

/// Furthest distance any camera can show [m].
pub fn culling_distance(settings: &LightingSettings) -> f32 {
    let mirrors = if settings.mirrors_enabled() {
        MIRROR_FAR
    } else {
        0.0
    };
    settings.view_distance.meters().max(mirrors)
}

fn range_for(radius: f32, distance: f32) -> VisibilityRange {
    VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: (distance + radius)..(distance + radius),
        // Measure from the centre of the bounds, not the entity's origin:
        // road chunks are built in world coordinates around the origin.
        use_aabb: true,
    }
}

fn rounded_radius(aabb: &Aabb, transform: &GlobalTransform) -> f32 {
    let radius = transform.radius_vec3a(aabb.half_extents);
    ((radius / RADIUS_STEP).ceil() * RADIUS_STEP).max(RADIUS_STEP)
}

#[allow(clippy::type_complexity)]
fn cull_new_meshes(
    mut commands: Commands,
    settings: Res<LightingSettings>,
    meshes: Query<
        (Entity, &Aabb, &GlobalTransform),
        (
            With<Mesh3d>,
            Without<DistanceCulled>,
            Without<NoDistanceCulling>,
        ),
    >,
) {
    let distance = culling_distance(&settings);
    for (entity, aabb, transform) in &meshes {
        let radius = rounded_radius(aabb, transform);
        commands
            .entity(entity)
            .try_insert((DistanceCulled { radius }, range_for(radius, distance)));
    }
}

fn follow_view_distance(
    settings: Res<LightingSettings>,
    mut last: Local<Option<f32>>,
    mut meshes: Query<(&DistanceCulled, &mut VisibilityRange)>,
) {
    let distance = culling_distance(&settings);
    if *last == Some(distance) {
        return;
    }
    *last = Some(distance);
    for (culled, mut range) in &mut meshes {
        let wanted = range_for(culled.radius, distance);
        if *range != wanted {
            *range = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::lighting::ViewDistance;

    #[test]
    fn a_mesh_stays_visible_while_any_part_is_inside_the_view_distance() {
        let aabb = Aabb::from_min_max(Vec3::new(-5.0, 0.0, -50.0), Vec3::new(5.0, 1.0, 50.0));
        let radius = rounded_radius(&aabb, &GlobalTransform::IDENTITY);
        assert!(radius >= 50.0, "{radius}");
        assert_eq!(radius % RADIUS_STEP, 0.0);
        let range = range_for(radius, 1_200.0);
        assert!(range.is_abrupt());
        assert!(range.is_visible_at_all(1_200.0 + 49.0));
        assert!(!range.is_visible_at_all(1_200.0 + radius + 1.0));
    }

    #[test]
    fn mirrors_extend_the_culling_distance() {
        let mut settings = LightingSettings {
            view_distance: ViewDistance::Near,
            rearview_mirrors: false,
            ..default()
        };
        assert_eq!(culling_distance(&settings), ViewDistance::Near.meters());
        settings.rearview_mirrors = true;
        assert_eq!(culling_distance(&settings), MIRROR_FAR);
        settings.view_distance = ViewDistance::Ultra;
        assert_eq!(culling_distance(&settings), ViewDistance::Ultra.meters());
    }
}
