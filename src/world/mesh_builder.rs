//! A minimal procedural mesh accumulator.
//!
//! Everything static in the world - road surface, lane markings, guard rails,
//! distance markers - is generated at start-up and merged into as few meshes as
//! possible. Chunking and merging keep the draw call count for the 33 km road
//! manageable while allowing distant geometry to be frustum culled.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

/// Accumulates triangles and turns them into a single [`Mesh`].
#[derive(Default)]
pub struct MeshBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl MeshBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Add a quad given in counter-clockwise order when seen from `normal`.
    pub fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, normal: Vec3, uv_scale: Vec2) {
        let base = self.positions.len() as u32;
        // Project onto the two largest axes of the quad for a stable UV layout.
        let corners = [a, b, c, d];
        let uvs = [
            Vec2::ZERO,
            Vec2::new(uv_scale.x, 0.0),
            uv_scale,
            Vec2::new(0.0, uv_scale.y),
        ];
        for (corner, uv) in corners.iter().zip(uvs.iter()) {
            self.positions.push([corner.x, corner.y, corner.z]);
            self.normals.push([normal.x, normal.y, normal.z]);
            self.uvs.push([uv.x, uv.y]);
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Horizontal quad lying in the XZ plane at height `y`, facing up.
    pub fn horizontal_rect(&mut self, min_x: f32, max_x: f32, min_z: f32, max_z: f32, y: f32) {
        let uv = Vec2::new((max_x - min_x).abs(), (max_z - min_z).abs());
        self.quad(
            Vec3::new(min_x, y, min_z),
            Vec3::new(min_x, y, max_z),
            Vec3::new(max_x, y, max_z),
            Vec3::new(max_x, y, min_z),
            Vec3::Y,
            uv,
        );
    }

    /// Axis aligned box given by its centre and half extents.
    pub fn box_at(&mut self, center: Vec3, half: Vec3) {
        self.box_transformed(center, half, Quat::IDENTITY);
    }

    /// Box transformed around its centre. This keeps curved-road guard rails
    /// merged per chunk without forcing their chords to stay world-axis aligned.
    pub fn box_transformed(&mut self, center: Vec3, half: Vec3, rotation: Quat) {
        let min = -half;
        let max = half;
        let uv = Vec2::ONE;
        let point = |value: Vec3| center + rotation * value;
        // +Y
        self.quad(
            point(Vec3::new(min.x, max.y, min.z)),
            point(Vec3::new(min.x, max.y, max.z)),
            point(Vec3::new(max.x, max.y, max.z)),
            point(Vec3::new(max.x, max.y, min.z)),
            rotation * Vec3::Y,
            uv,
        );
        // -Y
        self.quad(
            point(Vec3::new(min.x, min.y, min.z)),
            point(Vec3::new(max.x, min.y, min.z)),
            point(Vec3::new(max.x, min.y, max.z)),
            point(Vec3::new(min.x, min.y, max.z)),
            rotation * Vec3::NEG_Y,
            uv,
        );
        // +X
        self.quad(
            point(Vec3::new(max.x, min.y, min.z)),
            point(Vec3::new(max.x, max.y, min.z)),
            point(Vec3::new(max.x, max.y, max.z)),
            point(Vec3::new(max.x, min.y, max.z)),
            rotation * Vec3::X,
            uv,
        );
        // -X
        self.quad(
            point(Vec3::new(min.x, min.y, min.z)),
            point(Vec3::new(min.x, min.y, max.z)),
            point(Vec3::new(min.x, max.y, max.z)),
            point(Vec3::new(min.x, max.y, min.z)),
            rotation * Vec3::NEG_X,
            uv,
        );
        // +Z
        self.quad(
            point(Vec3::new(min.x, min.y, max.z)),
            point(Vec3::new(max.x, min.y, max.z)),
            point(Vec3::new(max.x, max.y, max.z)),
            point(Vec3::new(min.x, max.y, max.z)),
            rotation * Vec3::Z,
            uv,
        );
        // -Z
        self.quad(
            point(Vec3::new(min.x, min.y, min.z)),
            point(Vec3::new(min.x, max.y, min.z)),
            point(Vec3::new(max.x, max.y, min.z)),
            point(Vec3::new(max.x, min.y, min.z)),
            rotation * Vec3::NEG_Z,
            uv,
        );
    }

    /// Build the final mesh, or `None` if nothing was added.
    pub fn build(self) -> Option<Mesh> {
        if self.indices.is_empty() {
            return None;
        }
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        mesh.insert_indices(Indices::U32(self.indices));
        Some(mesh)
    }
}

/// Seven-segment digit drawn as flat quads, used for the kilometre boards.
/// Digits keep the world free of font assets while still being readable at
/// 400 km/h.
pub fn seven_segment(builder: &mut MeshBuilder, digit: u8, origin: Vec3, size: Vec2, stroke: f32) {
    const SEGMENTS: [[bool; 7]; 10] = [
        // a, b, c, d, e, f, g
        [true, true, true, true, true, true, false], // 0
        [false, true, true, false, false, false, false], // 1
        [true, true, false, true, true, false, true], // 2
        [true, true, true, true, false, false, true], // 3
        [false, true, true, false, false, true, true], // 4
        [true, false, true, true, false, true, true], // 5
        [true, false, true, true, true, true, true], // 6
        [true, true, true, false, false, false, false], // 7
        [true, true, true, true, true, true, true],  // 8
        [true, true, true, true, false, true, true], // 9
    ];

    let Some(segments) = SEGMENTS.get(digit as usize) else {
        return;
    };
    let (w, h) = (size.x, size.y);
    let half = h * 0.5;

    // Each entry is (centre, half extents) in the digit's local XY plane.
    let boxes = [
        (Vec2::new(w * 0.5, h), Vec2::new(w * 0.5, stroke * 0.5)), // a top
        (
            Vec2::new(w, h - half * 0.5),
            Vec2::new(stroke * 0.5, half * 0.5),
        ), // b top right
        (
            Vec2::new(w, half * 0.5),
            Vec2::new(stroke * 0.5, half * 0.5),
        ), // c bottom right
        (Vec2::new(w * 0.5, 0.0), Vec2::new(w * 0.5, stroke * 0.5)), // d bottom
        (
            Vec2::new(0.0, half * 0.5),
            Vec2::new(stroke * 0.5, half * 0.5),
        ), // e bottom left
        (
            Vec2::new(0.0, h - half * 0.5),
            Vec2::new(stroke * 0.5, half * 0.5),
        ), // f top left
        (Vec2::new(w * 0.5, half), Vec2::new(w * 0.5, stroke * 0.5)), // g middle
    ];

    for (on, (center, extents)) in segments.iter().zip(boxes.iter()) {
        if !*on {
            continue;
        }
        let min = origin + Vec3::new(center.x - extents.x, center.y - extents.y, 0.0);
        let max = origin + Vec3::new(center.x + extents.x, center.y + extents.y, 0.0);
        builder.quad(
            Vec3::new(min.x, min.y, min.z),
            Vec3::new(max.x, min.y, min.z),
            Vec3::new(max.x, max.y, min.z),
            Vec3::new(min.x, max.y, min.z),
            Vec3::NEG_Z,
            Vec2::ONE,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_builder_produces_nothing() {
        assert!(MeshBuilder::new().build().is_none());
    }

    #[test]
    fn a_box_has_twelve_triangles() {
        let mut builder = MeshBuilder::new();
        builder.box_at(Vec3::ZERO, Vec3::ONE);
        assert_eq!(builder.vertex_count(), 24);
        let mesh = builder.build().expect("mesh");
        assert_eq!(mesh.indices().map(|i| i.len()), Some(36));
    }

    #[test]
    fn digits_generate_the_right_segment_count() {
        let mut one = MeshBuilder::new();
        seven_segment(&mut one, 1, Vec3::ZERO, Vec2::new(0.5, 1.0), 0.1);
        let mut eight = MeshBuilder::new();
        seven_segment(&mut eight, 8, Vec3::ZERO, Vec2::new(0.5, 1.0), 0.1);
        assert_eq!(one.vertex_count(), 2 * 4);
        assert_eq!(eight.vertex_count(), 7 * 4);
    }
}
