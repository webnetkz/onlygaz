//! The track: a straight 33 km road with six lanes, a double solid centre line,
//! lane markings, shoulders and kilometre boards.
//!
//! Geometry conventions used throughout the world:
//!
//! ```text
//!   distance 0 starts at the beginning of the road
//!   +Z is the direction of travel and distance along the road
//!   +Y is up
//!   +X is the driver's left; negative X is the driver's right carriageway
//! ```

use avian3d::prelude::*;
use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;

use super::mesh_builder::{MeshBuilder, seven_segment};

/// Flat asphalt and its seamless collider continue this far beyond the centre
/// of each guard rail. This removes the narrow terrain step that could catch
/// an outside wheel during a hard impact.
pub const SURFACE_BEYOND_BARRIER: f32 = 1.0;

/// Extra authored road on both sides of the 33 km simulation loop. The player
/// wraps exactly at the nominal start/finish, while this continuation remains
/// in the camera frustum (and in rear-view mirrors), hiding the physical seam.
pub const LOOP_VISUAL_EXTENSION: f32 = 4_000.0;

/// Dimensions of the track. Everything else in `world` derives from this.
#[derive(Debug, Clone, Resource, Reflect)]
#[reflect(Resource)]
pub struct RoadSpec {
    /// Exact road length [m].
    pub length: f32,
    /// Lanes per direction.
    pub lanes_per_direction: usize,
    /// Width of one lane [m]. Motorway practice is 3.5 - 3.75 m.
    pub lane_width: f32,
    /// Paved shoulder outside the outermost lane line [m].
    pub shoulder_width: f32,
    /// Length of one visual chunk [m].
    pub chunk_length: f32,
    /// Width of a painted line [m].
    pub line_width: f32,
    /// Gap between the two lines of the centre double solid [m].
    pub double_line_gap: f32,
    /// Painted dash length and gap for the lane separators [m].
    pub dash_length: f32,
    pub dash_gap: f32,
    /// Height of the road surface [m].
    pub surface_y: f32,
    /// Markings are lifted slightly to avoid z-fighting [m].
    pub marking_y: f32,
    /// Lateral distance from the centre line to the guard rail [m].
    pub barrier_offset: f32,
    /// Spacing of the kilometre boards [m].
    pub marker_spacing: f32,
}

impl Default for RoadSpec {
    fn default() -> Self {
        Self {
            length: 33_000.0,
            lanes_per_direction: 3,
            lane_width: 3.6,
            shoulder_width: 1.6,
            chunk_length: 100.0,
            line_width: 0.15,
            double_line_gap: 0.20,
            dash_length: 3.0,
            dash_gap: 9.0,
            surface_y: 0.0,
            marking_y: 0.006,
            barrier_offset: 12.6,
            marker_spacing: 1000.0,
        }
    }
}

impl RoadSpec {
    /// Map any longitudinal coordinate back onto the 33 km road loop.
    /// `rem_euclid` also handles reversing through the start line.
    #[inline]
    pub fn wrap_distance(&self, distance: f32) -> f32 {
        if self.length > f32::EPSILON {
            distance.rem_euclid(self.length)
        } else {
            0.0
        }
    }

    /// Shortest signed longitudinal separation from `origin` to `target` on
    /// the loop. Positive values are towards world +Z.
    #[inline]
    pub fn signed_loop_delta(&self, origin: f32, target: f32) -> f32 {
        if self.length <= f32::EPSILON {
            return target - origin;
        }
        let forward = (target - origin).rem_euclid(self.length);
        if forward > self.length * 0.5 {
            forward - self.length
        } else {
            forward
        }
    }

    /// Distance to another point in the direction a traffic vehicle travels.
    #[inline]
    pub fn forward_loop_gap(&self, from: f32, to: f32, direction_sign: f32) -> f32 {
        if self.length <= f32::EPSILON {
            return (to - from) * direction_sign.signum();
        }
        if direction_sign >= 0.0 {
            (to - from).rem_euclid(self.length)
        } else {
            (from - to).rem_euclid(self.length)
        }
    }

    /// Unit tangent in the direction driven by the player.
    #[inline]
    pub fn tangent_at(&self, _distance: f32) -> Vec3 {
        Vec3::Z
    }

    /// Unit vector pointing to the driver's left.
    #[inline]
    pub fn left_at(&self, _distance: f32) -> Vec3 {
        Vec3::X
    }

    /// World position at a travelled distance and signed lateral offset.
    pub fn point_at(&self, distance: f32, lateral: f32, y: f32) -> Vec3 {
        Vec3::new(lateral, y, distance)
    }

    /// Rotation of a road-aligned object whose local +Z points forward.
    #[inline]
    pub fn frame_rotation(&self, _distance: f32) -> Quat {
        Quat::IDENTITY
    }

    /// Rotation of the vehicle chassis, whose local forward axis is -Z.
    #[inline]
    pub fn vehicle_rotation(&self, _distance: f32) -> Quat {
        Quat::from_rotation_y(core::f32::consts::PI)
    }

    /// Convert a world point to `(distance, lateral offset)` on the road.
    pub fn local_coordinates(&self, position: Vec3) -> (f32, f32) {
        (position.z.clamp(0.0, self.length), position.x)
    }

    /// Half width of the painted carriageway (centre line to outer lane line).
    #[inline]
    pub fn carriageway_half_width(&self) -> f32 {
        self.lanes_per_direction as f32 * self.lane_width
    }

    /// Half width of the paved surface including the shoulders.
    #[inline]
    pub fn paved_half_width(&self) -> f32 {
        self.carriageway_half_width() + self.shoulder_width
    }

    /// Full flat driving surface, including the safety apron behind the rails.
    #[inline]
    pub fn protected_surface_half_width(&self) -> f32 {
        self.barrier_offset + SURFACE_BEYOND_BARRIER
    }

    pub fn chunk_count(&self) -> usize {
        (self.length / self.chunk_length).ceil() as usize
    }

    /// Centre of a lane. `index` counts outward from the centre line, and
    /// `oncoming` selects the far carriageway.
    ///
    /// Traffic keeps right, so our own lanes have a negative lateral offset and
    /// occupy the negative-X half of the road.
    pub fn lane_center_x(&self, index: usize, oncoming: bool) -> f32 {
        let offset = (index as f32 + 0.5) * self.lane_width;
        if oncoming { offset } else { -offset }
    }

    /// Lane centre closest to `lateral` on our own carriageway.
    pub fn nearest_own_lane_x(&self, lateral: f32) -> f32 {
        let mut best = self.lane_center_x(1, false);
        let mut best_distance = f32::MAX;
        for index in 0..self.lanes_per_direction {
            let candidate = self.lane_center_x(index, false);
            let distance = (candidate - lateral).abs();
            if distance < best_distance {
                best_distance = distance;
                best = candidate;
            }
        }
        best
    }

    /// Where the car starts: middle right-hand lane, 30 m after the start line.
    pub fn spawn_transform(&self) -> Transform {
        let middle = self.lanes_per_direction / 2;
        let distance = 30.0;
        Transform::from_translation(self.point_at(
            distance,
            self.lane_center_x(middle, false),
            self.surface_y,
        ))
        .with_rotation(self.vehicle_rotation(distance))
    }

    /// A safe respawn pose near `position`, aligned with the road.
    pub fn respawn_transform(&self, position: Vec3) -> Transform {
        let (distance, lateral) = self.local_coordinates(position);
        Transform::from_translation(self.point_at(
            distance,
            self.nearest_own_lane_x(lateral),
            self.surface_y + 0.05,
        ))
        .with_rotation(self.vehicle_rotation(distance))
    }

    /// Distance from the beginning of the road, for the HUD and streaming.
    #[inline]
    pub fn distance_along(&self, position: Vec3) -> f32 {
        self.local_coordinates(position).0
    }
}

/// Marks a piece of static track geometry, so the whole track can be despawned
/// and rebuilt if the spec ever changes at runtime.
#[derive(Component)]
pub struct TrackPiece;

/// Materials shared by every chunk. One material instance per look keeps the
/// number of pipeline switches down.
#[derive(Resource)]
pub struct RoadMaterials {
    pub asphalt: Handle<StandardMaterial>,
    pub marking: Handle<StandardMaterial>,
    pub shoulder: Handle<StandardMaterial>,
    pub metal: Handle<StandardMaterial>,
    pub sign_face: Handle<StandardMaterial>,
    pub sign_text: Handle<StandardMaterial>,
    pub grass: Handle<StandardMaterial>,
    pub hill: Handle<StandardMaterial>,
}

const ASPHALT_DIFFUSE_PATH: &str = "environment/asphalt/asphalt_track_diff_2k.jpg";
const ASPHALT_NORMAL_PATH: &str = "environment/asphalt/asphalt_track_nor_gl_2k.jpg";
const ASPHALT_ARM_PATH: &str = "environment/asphalt/asphalt_track_arm_2k.jpg";
/// The Poly Haven scan represents a two-metre square of real asphalt.
const ASPHALT_TILE_METRES: f32 = 2.0;

/// Anisotropic filtering of the road textures. The phone build has
/// mipmaps (see `texture_mips`), where 16 samples per pixel would cost a
/// budget GPU dearly; 4 keeps the lane texture crisp to the horizon.
#[cfg(feature = "mobile")]
const ROAD_ANISOTROPY: u16 = 4;
#[cfg(not(feature = "mobile"))]
const ROAD_ANISOTROPY: u16 = 16;

fn repeating_texture(
    asset_server: &AssetServer,
    path: &'static str,
    is_srgb: bool,
) -> Handle<Image> {
    asset_server
        .load_builder()
        .with_settings(move |settings: &mut ImageLoaderSettings| {
            settings.is_srgb = is_srgb;
            let mut sampler = ImageSamplerDescriptor::linear();
            sampler
                .set_address_mode(ImageAddressMode::Repeat)
                .set_anisotropic_filter(ROAD_ANISOTROPY);
            settings.sampler = ImageSampler::Descriptor(sampler);
        })
        .load(path)
}

pub fn create_materials(
    asset_server: &AssetServer,
    materials: &mut Assets<StandardMaterial>,
) -> RoadMaterials {
    let asphalt_diffuse = repeating_texture(asset_server, ASPHALT_DIFFUSE_PATH, true);
    let asphalt_normal = repeating_texture(asset_server, ASPHALT_NORMAL_PATH, false);
    let asphalt_arm = repeating_texture(asset_server, ASPHALT_ARM_PATH, false);

    RoadMaterials {
        // The CC0 Poly Haven scan supplies colour, micro-normal, baked ambient
        // occlusion and roughness. Scalar values remain at one so the packed
        // ARM map controls the physical response without being attenuated.
        asphalt: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(asphalt_diffuse),
            normal_map_texture: Some(asphalt_normal),
            metallic_roughness_texture: Some(asphalt_arm.clone()),
            occlusion_texture: Some(asphalt_arm),
            perceptual_roughness: 1.0,
            reflectance: 0.28,
            metallic: 1.0,
            ..default()
        }),
        marking: materials.add(StandardMaterial {
            base_color: Color::srgb(0.86, 0.87, 0.86),
            perceptual_roughness: 0.58,
            reflectance: 0.32,
            metallic: 0.0,
            ..default()
        }),
        shoulder: materials.add(StandardMaterial {
            base_color: Color::srgb(0.11, 0.108, 0.105),
            perceptual_roughness: 0.78,
            ..default()
        }),
        metal: materials.add(StandardMaterial {
            base_color: Color::srgb(0.58, 0.60, 0.63),
            perceptual_roughness: 0.38,
            metallic: 0.9,
            ..default()
        }),
        sign_face: materials.add(StandardMaterial {
            base_color: Color::srgb(0.06, 0.28, 0.14),
            perceptual_roughness: 0.6,
            ..default()
        }),
        sign_text: materials.add(StandardMaterial {
            base_color: Color::srgb(0.95, 0.95, 0.95),
            unlit: false,
            perceptual_roughness: 0.6,
            ..default()
        }),
        // Real grass reflects about 15 % of the light that hits it; anything
        // brighter turns to neon under a physically bright sun.
        grass: materials.add(StandardMaterial {
            base_color: Color::srgb(0.14, 0.20, 0.085),
            perceptual_roughness: 0.95,
            reflectance: 0.12,
            ..default()
        }),
        hill: materials.add(StandardMaterial {
            base_color: Color::srgb(0.15, 0.21, 0.115),
            perceptual_roughness: 1.0,
            reflectance: 0.1,
            ..default()
        }),
    }
}

/// Add one flat strip between two travelled distances.
fn strip_quad(
    builder: &mut MeshBuilder,
    spec: &RoadSpec,
    lateral_min: f32,
    lateral_max: f32,
    distance_start: f32,
    distance_end: f32,
    y: f32,
) {
    builder.quad(
        spec.point_at(distance_start, lateral_min, y),
        spec.point_at(distance_end, lateral_min, y),
        spec.point_at(distance_end, lateral_max, y),
        spec.point_at(distance_start, lateral_max, y),
        Vec3::Y,
        Vec2::new(lateral_max - lateral_min, distance_end - distance_start),
    );
}

/// Asphalt surface of one chunk, as a single quad.
pub fn build_surface_chunk(
    spec: &RoadSpec,
    distance_start: f32,
    distance_end: f32,
) -> Option<Mesh> {
    let mut builder = MeshBuilder::new();
    let half = spec.protected_surface_half_width();
    builder.quad(
        spec.point_at(distance_start, -half, spec.surface_y),
        spec.point_at(distance_end, -half, spec.surface_y),
        spec.point_at(distance_end, half, spec.surface_y),
        spec.point_at(distance_start, half, spec.surface_y),
        Vec3::Y,
        Vec2::new(
            (half * 2.0) / ASPHALT_TILE_METRES,
            (distance_end - distance_start) / ASPHALT_TILE_METRES,
        ),
    );
    Some(
        builder
            .build()?
            .with_generated_tangents()
            .expect("asphalt quad has valid positions, normals and UVs"),
    )
}

/// Every painted line inside one chunk, merged into a single mesh.
///
/// Contains, from the centre outwards: the double solid centre line, the dashed
/// lane separators of both carriageways, and the solid outer edge lines.
pub fn build_markings_chunk(
    spec: &RoadSpec,
    distance_start: f32,
    distance_end: f32,
) -> Option<Mesh> {
    let mut builder = MeshBuilder::new();
    let y = spec.marking_y;
    let half_line = spec.line_width * 0.5;

    // Double solid centre line - two continuous lines either side of x = 0.
    for side in [-1.0f32, 1.0] {
        let center = side * (spec.double_line_gap * 0.5 + half_line);
        strip_quad(
            &mut builder,
            spec,
            center - half_line,
            center + half_line,
            distance_start,
            distance_end,
            y,
        );
    }

    // Dashed separators between lanes of the same carriageway.
    let period = spec.dash_length + spec.dash_gap;
    for oncoming in [false, true] {
        for index in 1..spec.lanes_per_direction {
            let x = if oncoming {
                index as f32 * spec.lane_width
            } else {
                -(index as f32) * spec.lane_width
            };
            let mut dash_start = (distance_start / period).floor() * period;
            while dash_start < distance_end {
                let start = dash_start.max(distance_start);
                let end = (dash_start + spec.dash_length).min(distance_end);
                if end > start {
                    strip_quad(
                        &mut builder,
                        spec,
                        x - half_line,
                        x + half_line,
                        start,
                        end,
                        y,
                    );
                }
                dash_start += period;
            }
        }
    }

    // Solid edge lines at the outside of the outermost lanes.
    let edge = spec.carriageway_half_width();
    for side in [-1.0f32, 1.0] {
        let x = side * edge;
        strip_quad(
            &mut builder,
            spec,
            x - half_line,
            x + half_line,
            distance_start,
            distance_end,
            y,
        );
    }

    builder.build()
}

/// A kilometre board: post, plate and the number in seven segment digits.
///
/// Returns the structure and the digits as separate meshes so they can use
/// different materials. `None` if the number produced no geometry at all.
pub fn build_distance_marker(kilometre: u32) -> Option<(Mesh, Mesh)> {
    let mut structure = MeshBuilder::new();
    // Post.
    structure.box_at(Vec3::new(0.0, 0.9, 0.0), Vec3::new(0.05, 0.9, 0.05));
    // Plate.
    structure.box_at(Vec3::new(0.0, 1.95, 0.0), Vec3::new(0.55, 0.42, 0.04));

    let mut digits = MeshBuilder::new();
    let text = kilometre.to_string();
    let digit_width = 0.22;
    let digit_height = 0.46;
    let spacing = 0.32;
    let total = spacing * text.len() as f32 - (spacing - digit_width);
    let mut x = -total * 0.5;
    for character in text.chars() {
        if let Some(value) = character.to_digit(10) {
            seven_segment(
                &mut digits,
                value as u8,
                Vec3::new(x, 1.95 - digit_height * 0.5, -0.05),
                Vec2::new(digit_width, digit_height),
                0.07,
            );
        }
        x += spacing;
    }

    Some((structure.build()?, digits.build()?))
}

/// One planar ground strip around a road chunk. These strips tile into a broad
/// corridor and are independently frustum culled.
pub fn build_terrain_chunk(
    spec: &RoadSpec,
    distance_start: f32,
    distance_end: f32,
    half_width: f32,
) -> Option<Mesh> {
    let mut builder = MeshBuilder::new();
    strip_quad(
        &mut builder,
        spec,
        -half_width,
        half_width,
        distance_start,
        distance_end,
        spec.surface_y - 0.14,
    );
    builder.build()
}

/// Continuous static driving surface. The caller spans the full 33 km road so
/// swept collision detection cannot encounter internal chunk faces.
pub fn road_collider(
    spec: &RoadSpec,
    distance_start: f32,
    distance_end: f32,
) -> (Collider, Transform) {
    let thickness = 2.0;
    let start = spec.point_at(distance_start, 0.0, spec.surface_y - thickness * 0.5);
    let end = spec.point_at(distance_end, 0.0, spec.surface_y - thickness * 0.5);
    let chord_length = start.distance(end);
    let midpoint = (distance_start + distance_end) * 0.5;
    let collider = Collider::cuboid(
        spec.protected_surface_half_width() * 2.0,
        thickness,
        chord_length + 0.4,
    );
    let transform = Transform::from_translation((start + end) * 0.5)
        .with_rotation(spec.frame_rotation(midpoint));
    (collider, transform)
}

/// Infinite flat ground slightly below the asphalt. One half-space is cheaper
/// and more robust than thousands of terrain colliders, while the visible
/// terrain remains the 1.2 km-wide corridor generated above.
pub fn terrain_collider(spec: &RoadSpec) -> (Collider, Transform) {
    (
        Collider::half_space(Vec3::Y),
        Transform::from_xyz(0.0, spec.surface_y - 0.14, 0.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_lanes_of_realistic_width() {
        let spec = RoadSpec::default();
        assert_eq!(spec.lanes_per_direction * 2, 6);
        assert!((3.5..=3.75).contains(&spec.lane_width));
        assert!((spec.length - 33_000.0).abs() < 1.0);
    }

    #[test]
    fn our_lanes_are_on_the_right_hand_side() {
        let spec = RoadSpec::default();
        for index in 0..spec.lanes_per_direction {
            assert!(spec.lane_center_x(index, false) < 0.0);
            assert!(spec.lane_center_x(index, true) > 0.0);
        }
    }

    #[test]
    fn spawn_is_in_the_middle_lane_facing_along_the_road() {
        let spec = RoadSpec::default();
        let pose = spec.spawn_transform();
        let (distance, lateral) = spec.local_coordinates(pose.translation);
        assert!((distance - 30.0).abs() < 0.01);
        assert!((lateral - spec.lane_center_x(1, false)).abs() < 0.01);
        // The car's own forward is -Z, so a half turn points it at +Z.
        let forward = pose.rotation * Vec3::NEG_Z;
        assert!(
            forward.dot(spec.tangent_at(distance)) > 0.9999,
            "{forward:?}"
        );
    }

    #[test]
    fn nearest_lane_snaps_to_a_lane_centre() {
        let spec = RoadSpec::default();
        let snapped = spec.nearest_own_lane_x(-5.0);
        assert!((snapped - spec.lane_center_x(1, false)).abs() < 1.0e-4);
        // Even from the wrong side of the road we come back to our own lanes.
        assert!(spec.nearest_own_lane_x(7.0) < 0.0);
    }

    #[test]
    fn chunks_cover_the_whole_track() {
        let spec = RoadSpec::default();
        assert_eq!(spec.chunk_count(), 330);
        let mesh = build_surface_chunk(&spec, 0.0, spec.chunk_length).expect("road mesh");
        assert!(
            mesh.attribute(Mesh::ATTRIBUTE_TANGENT).is_some(),
            "the asphalt normal map needs tangent-space geometry"
        );
    }

    #[test]
    fn flat_surface_reaches_one_metre_beyond_both_guard_rails() {
        let spec = RoadSpec::default();
        assert!(
            spec.protected_surface_half_width() >= spec.barrier_offset + 1.0,
            "surface edge {}, barrier {}",
            spec.protected_surface_half_width(),
            spec.barrier_offset
        );
        assert!(spec.protected_surface_half_width() > spec.paved_half_width());
    }

    #[test]
    fn road_is_straight_and_longitudinal_coordinates_wrap() {
        let spec = RoadSpec::default();
        let start = spec.point_at(0.0, 0.0, 0.0);
        let finish = spec.point_at(spec.length, 0.0, 0.0);
        assert_eq!(start, Vec3::ZERO);
        assert_eq!(finish, Vec3::new(0.0, 0.0, 33_000.0));
        assert_eq!(spec.tangent_at(16_500.0), Vec3::Z);
        assert_eq!(spec.frame_rotation(16_500.0), Quat::IDENTITY);
        assert!((spec.wrap_distance(spec.length + 17.5) - 17.5).abs() < 1.0e-4);
        assert!((spec.wrap_distance(-12.0) - (spec.length - 12.0)).abs() < 1.0e-4);
        assert!((spec.signed_loop_delta(spec.length - 10.0, 15.0) - 25.0).abs() < 1.0e-4);
        assert!((spec.forward_loop_gap(15.0, spec.length - 10.0, -1.0) - 25.0).abs() < 1.0e-4);
    }

    #[test]
    fn world_and_road_coordinates_round_trip() {
        let spec = RoadSpec::default();
        for distance in [0.0, 12_345.0, 32_999.0] {
            for lateral in [-9.0, 0.0, 11.0] {
                let point = spec.point_at(distance, lateral, 0.0);
                let (measured_distance, measured_lateral) = spec.local_coordinates(point);
                assert!((measured_distance - distance).abs() < 0.02);
                assert!((measured_lateral - lateral).abs() < 0.01);
            }
        }
    }

    #[test]
    fn markings_contain_the_double_solid_centre_line() {
        let spec = RoadSpec::default();
        // A stretch inside a dash gap still has the centre and edge lines.
        let mesh = build_markings_chunk(&spec, 4.0, 8.0).expect("markings");
        // 2 centre lines + 2 edge lines = at least 4 quads = 16 vertices.
        assert!(mesh.count_vertices() >= 16, "{}", mesh.count_vertices());
    }

    #[test]
    fn distance_markers_render_two_digit_numbers() {
        let (structure, digits) = build_distance_marker(10).expect("board");
        assert!(structure.count_vertices() > 0);
        assert!(digits.count_vertices() > 0);
    }
}
