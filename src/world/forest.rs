//! Streamed fir forest used by the Cinematic graphics preset.
//!
//! The supplied asset contains three trees at four authored LODs. The road is
//! 33 kilometres long, so keeping every possible tree alive would waste CPU
//! time even when most of them are behind the camera. Instead, deterministic
//! 300 metre chunks follow the player. The closest rows use full geometry,
//! progressively deeper rows use the lighter authored LODs, and the back rows
//! use the crossed-plane billboard supplied by the artist.

use std::collections::HashSet;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::vehicle::physics::VehicleTag;

use super::lighting::{LightingQuality, LightingSettings};
use super::road::RoadSpec;

const FOREST_ASSET: &str = "environment/fir_trees/scene.gltf";
const CHUNK_LENGTH: f32 = 300.0;
/// Eight chunks in each direction keep forest visible for 2.4 km behind the
/// car and 2.7 km ahead while bounding the number of live scene instances.
const STREAM_RADIUS: i32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TreeLod {
    Lod0,
    Lod1,
    Lod2,
    Billboard,
}

impl TreeLod {
    const fn scene_offset(self) -> usize {
        match self {
            Self::Lod0 => 0,
            Self::Lod1 => 1,
            Self::Lod2 => 2,
            Self::Billboard => 3,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ForestRow {
    /// Absolute distance from the centre of the road.
    x: f32,
    /// Average longitudinal distance between trees.
    spacing: f32,
    lod: TreeLod,
}

// Dense close rows give a convincing wall of trees at driving speed. Each
// deeper row can be sparser and use a lower LOD because it is screened by the
// rows in front of it.
const ROWS: [ForestRow; 10] = [
    ForestRow {
        x: 20.0,
        spacing: 15.0,
        lod: TreeLod::Lod0,
    },
    ForestRow {
        x: 28.0,
        spacing: 18.0,
        lod: TreeLod::Lod1,
    },
    ForestRow {
        x: 38.0,
        spacing: 21.0,
        lod: TreeLod::Lod1,
    },
    ForestRow {
        x: 50.0,
        spacing: 24.0,
        lod: TreeLod::Lod2,
    },
    ForestRow {
        x: 65.0,
        spacing: 29.0,
        lod: TreeLod::Lod2,
    },
    ForestRow {
        x: 85.0,
        spacing: 35.0,
        lod: TreeLod::Lod2,
    },
    ForestRow {
        x: 111.0,
        spacing: 43.0,
        lod: TreeLod::Billboard,
    },
    ForestRow {
        x: 145.0,
        spacing: 53.0,
        lod: TreeLod::Billboard,
    },
    ForestRow {
        x: 190.0,
        spacing: 69.0,
        lod: TreeLod::Billboard,
    },
    ForestRow {
        x: 250.0,
        spacing: 91.0,
        lod: TreeLod::Billboard,
    },
];

/// One streamed section of the Cinematic-only forest.
#[derive(Component, Debug, Clone, Copy)]
pub struct CinematicForestChunk {
    pub index: i32,
}

/// Root of one imported tree scene. It records the chosen authored LOD so
/// asynchronously-created billboard meshes can be excluded from shadow maps.
#[derive(Component, Debug, Clone, Copy)]
pub struct CinematicForestTree {
    lod: TreeLod,
}

/// Renderable mesh produced below a [`CinematicForestTree`] scene root.
#[derive(Component, Debug, Clone, Copy)]
pub struct CinematicForestMesh;

#[derive(Debug, Clone, Copy)]
struct TreePlacement {
    position: Vec3,
    yaw: f32,
    scale: f32,
    variant: usize,
    lod: TreeLod,
}

/// Creates and removes forest chunks as the graphics preset or player position
/// changes. Crucially, no tree asset handle is requested on Performance or
/// Balanced, so those presets keep the original inexpensive ground plane.
pub fn stream_cinematic_forest(
    mut commands: Commands,
    settings: Res<LightingSettings>,
    road: Res<RoadSpec>,
    asset_server: Res<AssetServer>,
    vehicles: Query<&Transform, With<VehicleTag>>,
    chunks: Query<(Entity, &CinematicForestChunk)>,
) {
    if settings.quality != LightingQuality::Cinematic {
        if !chunks.is_empty() {
            for (entity, _) in &chunks {
                commands.entity(entity).despawn();
            }
            info!("cinematic forest unloaded; flat terrain retained");
        }
        return;
    }

    let focus_distance = vehicles
        .iter()
        .next()
        .map(|transform| road.distance_along(transform.translation))
        .unwrap_or(30.0);
    let centre = (focus_distance / CHUNK_LENGTH).floor() as i32;
    let wanted: HashSet<i32> = (-STREAM_RADIUS..=STREAM_RADIUS)
        .map(|offset| centre + offset)
        .collect();

    let mut present = HashSet::with_capacity(chunks.iter().len());
    let forest_was_empty = chunks.is_empty();
    for (entity, chunk) in &chunks {
        if !wanted.contains(&chunk.index) {
            commands.entity(entity).despawn();
        } else {
            present.insert(chunk.index);
        }
    }

    let mut missing: Vec<i32> = wanted.difference(&present).copied().collect();
    missing.sort_unstable();
    for index in missing {
        if !present.contains(&index) {
            spawn_chunk(&mut commands, &asset_server, &road, index);
        }
    }

    if forest_was_empty {
        info!(
            "cinematic fir forest enabled: {} straight-road chunks around {}, authored LOD0-LOD3",
            wanted.len(),
            centre
        );
    }
}

fn spawn_chunk(commands: &mut Commands, asset_server: &AssetServer, road: &RoadSpec, index: i32) {
    let root = commands
        .spawn((
            Name::new(format!("CinematicForestChunk{index}")),
            CinematicForestChunk { index },
            Transform::IDENTITY,
            Visibility::default(),
        ))
        .id();

    for placement in placements_for_chunk(index, road) {
        // Scene order in the adapted source glTF is variant-major, then LOD0
        // through LOD3. AssetServer deduplicates these twelve labelled handles.
        let scene = placement.variant * 4 + placement.lod.scene_offset();
        let upright = Quat::from_rotation_z(core::f32::consts::PI);
        let rotation = Quat::from_rotation_y(placement.yaw) * upright;
        let transform = Transform::from_translation(placement.position)
            .with_rotation(rotation)
            .with_scale(Vec3::splat(placement.scale));

        commands.spawn((
            CinematicForestTree { lod: placement.lod },
            transform,
            Visibility::default(),
            WorldAssetRoot(
                asset_server.load(GltfAssetLabel::Scene(scene).from_asset(FOREST_ASSET)),
            ),
            ChildOf(root),
        ));
    }
}

fn placements_for_chunk(index: i32, road: &RoadSpec) -> Vec<TreePlacement> {
    let z0 = index as f32 * CHUNK_LENGTH;
    let z1 = z0 + CHUNK_LENGTH;
    let chunk_count = (road.length / CHUNK_LENGTH).ceil().max(1.0) as i32;
    // The same deterministic tree layout is repeated one road length away.
    // World-space `index` may be negative or beyond the nominal final chunk,
    // while the seed remains periodic across the start/finish transition.
    let seed_index = index.rem_euclid(chunk_count);

    let mut placements = Vec::with_capacity(224);
    for (row_index, row) in ROWS.iter().enumerate() {
        let slots = ((z1 - z0) / row.spacing).ceil() as usize;
        for side_index in 0..2usize {
            let side = if side_index == 0 { -1.0 } else { 1.0 };
            for slot in 0..slots {
                let seed = (seed_index as u32).wrapping_mul(0x9e37_79b9)
                    ^ (row_index as u32).wrapping_mul(0x85eb_ca6b)
                    ^ (side_index as u32).wrapping_mul(0xc2b2_ae35)
                    ^ (slot as u32).wrapping_mul(0x27d4_eb2d);
                let along_jitter = (random01(seed) - 0.5) * row.spacing * 0.68;
                let lateral_jitter = (random01(seed ^ 0x68bc_21eb) - 0.5) * 5.2;
                let z = (z0 + (slot as f32 + 0.5) * row.spacing + along_jitter)
                    .clamp(z0 + 0.25, z1 - 0.25);
                let x = side * (row.x + lateral_jitter);
                let variant = (mix(seed ^ 0xa511_e9b3) % 3) as usize;
                // The three source variants are authored at very different
                // heights (roughly 9.3, 4.2 and 1.6 m). Normalising them into a
                // mature 8-12 m band preserves their silhouette variety while
                // making the result read as forest rather than scattered
                // saplings.
                let variant_scale = [1.0, 2.15, 4.8][variant];
                let scale = (0.86 + random01(seed ^ 0x3c6e_f372) * 0.42) * variant_scale;
                let yaw = random01(seed ^ 0xbb67_ae85) * core::f32::consts::TAU;

                placements.push(TreePlacement {
                    position: road.point_at(z, x, road.surface_y),
                    yaw,
                    scale,
                    variant,
                    lod: row.lod,
                });
            }
        }
    }
    placements
}

#[inline]
fn random01(seed: u32) -> f32 {
    (mix(seed) as f32) / (u32::MAX as f32)
}

#[inline]
fn mix(mut value: u32) -> u32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^ (value >> 16)
}

/// Imported scene children appear asynchronously. Billboards are visual
/// impostors and should never spend shadow-map draw calls or cast crossed-card
/// silhouettes onto the road.
pub fn configure_new_forest_meshes(
    mut commands: Commands,
    new_meshes: Query<Entity, Added<Mesh3d>>,
    ancestry: Query<(Option<&ChildOf>, Option<&CinematicForestTree>)>,
) {
    for mesh in &new_meshes {
        let mut current = mesh;
        while let Ok((parent, tree)) = ancestry.get(current) {
            if let Some(tree) = tree {
                commands.entity(mesh).insert(CinematicForestMesh);
                if tree.lod == TreeLod::Billboard {
                    commands.entity(mesh).insert(NotShadowCaster);
                }
                break;
            }
            let Some(parent) = parent else {
                break;
            };
            current = parent.parent();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tree_stays_clear_of_the_guard_rail_and_inside_its_chunk() {
        let road = RoadSpec::default();
        let placements = placements_for_chunk(7, &road);
        let z0 = 7.0 * CHUNK_LENGTH;
        let z1 = z0 + CHUNK_LENGTH;

        assert!(placements.len() > 190, "the forest should read as dense");
        for tree in placements {
            let (distance, lateral) = road.local_coordinates(tree.position);
            assert!(lateral.abs() > road.barrier_offset + 4.0);
            assert!(distance >= z0 && distance <= z1);
            assert_eq!(tree.position.y, road.surface_y);
            assert!((0..3).contains(&tree.variant));
        }
    }

    #[test]
    fn forest_layout_repeats_across_the_road_loop() {
        let road = RoadSpec::default();
        let chunk_count = (road.length / CHUNK_LENGTH) as i32;
        let first = placements_for_chunk(0, &road);
        let repeated = placements_for_chunk(chunk_count, &road);
        assert_eq!(first.len(), repeated.len());
        for (a, b) in first.iter().zip(&repeated) {
            assert!((a.position.x - b.position.x).abs() < 1.0e-4);
            assert!((a.position.z + road.length - b.position.z).abs() < 1.0e-3);
            assert!((a.yaw - b.yaw).abs() < 1.0e-5);
            assert!((a.scale - b.scale).abs() < 1.0e-5);
        }
    }

    #[test]
    fn forest_rows_use_every_authored_lod() {
        let road = RoadSpec::default();
        let placements = placements_for_chunk(0, &road);
        for lod in [
            TreeLod::Lod0,
            TreeLod::Lod1,
            TreeLod::Lod2,
            TreeLod::Billboard,
        ] {
            assert!(placements.iter().any(|tree| tree.lod == lod));
        }
    }

    #[test]
    fn placement_is_deterministic_between_streaming_passes() {
        let road = RoadSpec::default();
        let first = placements_for_chunk(12, &road);
        let second = placements_for_chunk(12, &road);
        assert_eq!(first.len(), second.len());
        for (a, b) in first.iter().zip(&second) {
            assert_eq!(a.position, b.position);
            assert_eq!(a.variant, b.variant);
            assert_eq!(a.lod, b.lod);
        }
    }
}
