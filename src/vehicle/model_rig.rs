//! Wiring the supplied glTF model to the simulation.
//!
//! The model is a Sketchfab export of a real scan, so its nodes are named after
//! the artist's parts, not after anything a game engine understands, and every
//! wheel mesh has its pivot at the model origin rather than at the hub. Two
//! things therefore have to happen once the scene is spawned:
//!
//! 1. **Discovery.** Every node is classified into `body`, one of the four
//!    wheels, or the steering wheel. Classification is by name pattern first and
//!    by mesh position second, and the result is logged so a different model can
//!    be diagnosed quickly.
//! 2. **Re-pivoting.** Each wheel gets a pair of intermediate parent entities -
//!    a hub pivot that carries suspension travel and steering, and a spin pivot
//!    that carries wheel rotation - placed at the *measured* hub centre. The
//!    original nodes are re-parented under them with their transforms rebased,
//!    so they rotate about the axle instead of about the car's origin.
//!
//! The physical wheel positions come from [`VehicleConfig`] and are completely
//! independent of the visual mesh; the rig only decides where the *pictures* go.

use bevy::camera::primitives::{Aabb, MeshAabb};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;

use super::VehicleKind;
use super::config::WheelId;
use super::physics::{
    SteeringWheelPivot, Vehicle, VehicleModelRoot, WheelHubPivot, WheelSpinPivot, model_space_for,
};

/// Which rig slot a node belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    /// Rotates with the wheel: rim, tyre, brake disc.
    Spin(WheelId),
    /// Follows suspension and steering but does not spin: brake caliper.
    Hub(WheelId),
    /// The steering wheel in the cockpit.
    Steering,
}

/// Name fragments that identify a wheel, in the model's own left/right terms.
///
/// The glTF is authored with +X to the car's left and +Z forward, and the
/// artist's `lf` / `rf` / `lr` / `rr` suffixes follow that same convention, so
/// `l` really is the driver's side of this left hand drive car.
const ROTOR_PATTERNS: [(&str, WheelId); 4] = [
    ("rotorlf", WheelId::FrontLeft),
    ("rotorrf", WheelId::FrontRight),
    ("rotorlr", WheelId::RearLeft),
    ("rotorrr", WheelId::RearRight),
];

const CALIPER_PATTERNS: [(&str, WheelId); 4] = [
    ("caliperlf", WheelId::FrontLeft),
    ("caliperrf", WheelId::FrontRight),
    ("caliperlr", WheelId::RearLeft),
    ("caliperrr", WheelId::RearRight),
];

/// Rim group prefix. The trailing index selects the corner.
const RIM_PREFIX: &str = "bug_veyron_09_s_wheel_wheel_";

/// Semantic prefix written by `tools/split_steering_wheel.py` for models whose
/// steering wheel was originally welded into the complete cockpit mesh.
const STEERING_PREFIX: &str = "steeringwheel_";

/// Tyre nodes, identified by their generated surface names.
const TYRE_PATTERNS: [(&str, WheelId); 4] = [
    ("polysurface479", WheelId::RearRight),
    ("polysurface480", WheelId::RearLeft),
    ("polysurface481", WheelId::FrontRight),
    ("polysurface482", WheelId::FrontLeft),
];

/// How far a mesh centroid may be from a hub before the positional fallback
/// refuses to claim it [m].
const FALLBACK_RADIUS: f32 = 0.62;

/// Classify one node name. Returns `None` for anything that stays on the body.
fn classify(name: &str) -> Option<Slot> {
    let lower = name.to_ascii_lowercase();

    if lower.contains("steering_wheel") {
        return Some(Slot::Steering);
    }
    for (pattern, id) in CALIPER_PATTERNS {
        if lower.contains(pattern) {
            return Some(Slot::Hub(id));
        }
    }
    for (pattern, id) in ROTOR_PATTERNS {
        if lower.contains(pattern) {
            return Some(Slot::Spin(id));
        }
    }
    for (pattern, id) in TYRE_PATTERNS {
        if lower.contains(pattern) {
            return Some(Slot::Spin(id));
        }
    }
    if let Some(rest) = lower.strip_prefix(RIM_PREFIX) {
        // `..._outer_rim_`, `..._outer_rim_1`, `_2`, `_3` - the trailing index
        // selects the corner, and no index at all means the first one.
        let id = match rest.chars().next_back() {
            Some('1') => WheelId::FrontRight,
            Some('2') => WheelId::RearLeft,
            Some('3') => WheelId::RearRight,
            _ => WheelId::FrontLeft,
        };
        return Some(Slot::Spin(id));
    }
    None
}

/// The Mustang export uses generated node names. These exact leaf nodes are the
/// repeated wheel groups measured in the supplied, immutable glTF. Explicitly
/// listing them is intentional: a broad positional match could accidentally
/// attach the driver/pylon or an aero panel to a rotating wheel.
fn classify_mustang(name: &str) -> Option<Slot> {
    let number = name.strip_prefix("Object_")?.parse::<u16>().ok()?;
    let id = if [8, 23, 56, 80, 83, 185, 206, 209, 221, 245].contains(&number) {
        WheelId::FrontLeft
    } else if [11, 26, 59, 86, 89, 182, 200, 203, 218, 242].contains(&number) {
        WheelId::FrontRight
    } else if [14, 29, 62, 92, 98, 173, 194, 197, 215, 239].contains(&number) {
        WheelId::RearLeft
    } else if [17, 32, 65, 95, 101, 170, 188, 191, 212, 236].contains(&number) {
        WheelId::RearRight
    } else {
        return None;
    };
    Some(Slot::Spin(id))
}

/// Semantic nodes produced by `tools/split_combined_wheels.py` for archives
/// which originally stored all four tyres/rims in one mesh.
fn classify_split_wheel(name: &str) -> Option<Slot> {
    let lower = name.to_ascii_lowercase();
    for (prefix, constructor) in [
        ("wheelspin_", Slot::Spin as fn(WheelId) -> Slot),
        ("wheelhub_", Slot::Hub as fn(WheelId) -> Slot),
    ] {
        let Some(corner) = lower.strip_prefix(prefix) else {
            continue;
        };
        let id = if corner.starts_with("fl_") {
            WheelId::FrontLeft
        } else if corner.starts_with("fr_") {
            WheelId::FrontRight
        } else if corner.starts_with("rl_") {
            WheelId::RearLeft
        } else if corner.starts_with("rr_") {
            WheelId::RearRight
        } else {
            return None;
        };
        return Some(constructor(id));
    }
    None
}

/// Wheel and caliper parent groups measured in the supplied SLS hierarchy.
fn classify_sls(name: &str) -> Option<Slot> {
    match name.to_ascii_lowercase().as_str() {
        "wheel.001_12" => Some(Slot::Spin(WheelId::FrontLeft)),
        "wheel_13" => Some(Slot::Spin(WheelId::FrontRight)),
        "wheel.003_0" => Some(Slot::Spin(WheelId::RearLeft)),
        "wheel.002_11" => Some(Slot::Spin(WheelId::RearRight)),
        "calliper.001_15" => Some(Slot::Hub(WheelId::FrontLeft)),
        "calliper_16" => Some(Slot::Hub(WheelId::FrontRight)),
        "calliper.003_17" => Some(Slot::Hub(WheelId::RearLeft)),
        "calliper.002_18" => Some(Slot::Hub(WheelId::RearRight)),
        _ => None,
    }
}

fn classify_additional(kind: VehicleKind, name: &str) -> Option<Slot> {
    let lower = name.to_ascii_lowercase();
    let corner = |fl, fr, rl, rr| {
        if lower == fl {
            Some(WheelId::FrontLeft)
        } else if lower == fr {
            Some(WheelId::FrontRight)
        } else if lower == rl {
            Some(WheelId::RearLeft)
        } else if lower == rr {
            Some(WheelId::RearRight)
        } else {
            None
        }
    };
    let amg_rim = || {
        let direct = corner(
            "amgprojone_rim_19",
            "amgprojone_rim_003_61",
            "amgprojone_rim_002_49",
            "amgprojone_rim_001_55",
        );
        if direct.is_some() {
            return direct;
        }
        let suffix = lower.strip_prefix("amgprojone_rim_")?;
        let number = suffix.get(..3)?.parse::<u8>().ok()?;
        match number {
            4..=11 => Some(WheelId::FrontLeft),
            12..=19 => Some(WheelId::RearLeft),
            20..=27 => Some(WheelId::RearRight),
            28..=35 => Some(WheelId::FrontRight),
            _ => None,
        }
    };
    let wheel = match kind {
        VehicleKind::MercedesAmgOne => corner(
            "amgprojone_tire_7",
            "amgprojone_tire_003_58",
            "amgprojone_tire_001_46",
            "amgprojone_tire_002_52",
        )
        .or_else(amg_rim),
        VehicleKind::KoenigseggJesko => {
            corner("wheellf_26", "wheelrf_24", "wheellr_25", "wheelrr_23")
        }
        VehicleKind::LamborghiniRevuelto => corner(
            "_3dwheel_front_l_315",
            "_3dwheel_front_r_312",
            "_3dwheel_back_l_321",
            "_3dwheel_back_r_318",
        ),
        _ => None,
    };
    if let Some(id) = wheel {
        return Some(Slot::Spin(id));
    }
    if kind == VehicleKind::KoenigseggJesko {
        let caliper = corner(
            "caliperlf_27",
            "caliperrf_28",
            "caliperlr_29",
            "caliperrr_30",
        );
        if let Some(id) = caliper {
            return Some(Slot::Hub(id));
        }
    }
    if lower.contains("steering_wheel") || lower.starts_with("amgprojone_steer_") {
        return Some(Slot::Steering);
    }
    None
}

fn classify_for(kind: VehicleKind, name: &str) -> Option<Slot> {
    let lower = name.to_ascii_lowercase();
    if lower.starts_with(STEERING_PREFIX) {
        // The SLS wheel is painted by two separate extracted material layers.
        // Both belong to the same wheel and must share one steering pivot;
        // leaving Object_21 on the body makes it look like a second, stationary
        // wheel behind Object_20.  The unsplit Object_20/Object_21 body nodes
        // keep the column shroud and its indicator/light stalks in the authored
        // (stationary) cockpit hierarchy.
        return match kind {
            VehicleKind::MercedesSlsAmg
                if matches!(
                    lower.as_str(),
                    "steeringwheel_object_20" | "steeringwheel_object_21"
                ) =>
            {
                Some(Slot::Steering)
            }
            VehicleKind::MercedesSlsAmg => None,
            _ => Some(Slot::Steering),
        };
    }
    match kind {
        VehicleKind::Veyron => classify(name),
        VehicleKind::MustangGt3 => classify_mustang(name),
        VehicleKind::PaganiZondaCinque => classify_split_wheel(name),
        VehicleKind::MercedesSlsAmg => classify_sls(name),
        _ => classify_split_wheel(name).or_else(|| classify_additional(kind, name)),
    }
}

/// Read-only view of a spawned scene, grouped so the observer below stays
/// readable.
#[derive(SystemParam)]
pub struct SceneNodes<'w, 's> {
    children: Query<'w, 's, &'static Children>,
    names: Query<'w, 's, &'static Name>,
    transforms: Query<'w, 's, &'static Transform>,
    parents: Query<'w, 's, &'static ChildOf>,
    mesh_handles: Query<'w, 's, &'static Mesh3d>,
    aabbs: Query<'w, 's, &'static Aabb>,
    meshes: Res<'w, Assets<Mesh>>,
}

impl SceneNodes<'_, '_> {
    /// Every node below `root`, in no particular order.
    pub(crate) fn descendants(&self, root: Entity) -> impl Iterator<Item = Entity> + '_ {
        self.children.iter_descendants(root)
    }

    /// The artist's name for a node, if it has one.
    pub(crate) fn name(&self, entity: Entity) -> Option<&str> {
        self.names.get(entity).ok().map(Name::as_str)
    }

    /// Classify a rendered leaf by either its own name or a semantic parent.
    /// Sketchfab exports frequently put the useful name on two empty grouping
    /// nodes and leave the actual painted mesh as a generic `Object_123`.
    pub(crate) fn find_in_ancestry<T>(
        &self,
        entity: Entity,
        root: Entity,
        mut classify: impl FnMut(&str) -> Option<T>,
    ) -> Option<T> {
        let mut current = entity;
        loop {
            if let Some(found) = self.name(current).and_then(&mut classify) {
                return Some(found);
            }
            let parent = self.parents.get(current).ok()?.parent();
            if parent == root {
                return None;
            }
            current = parent;
        }
    }

    /// Accumulated transform of `entity` expressed in `root`'s frame.
    pub(crate) fn transform_relative_to(&self, entity: Entity, root: Entity) -> Option<Mat4> {
        let mut matrix = self.transforms.get(entity).ok()?.to_matrix();
        let mut current = entity;
        while let Ok(parent) = self.parents.get(current) {
            let parent_entity = parent.parent();
            if parent_entity == root {
                return Some(matrix);
            }
            let parent_transform = self.transforms.get(parent_entity).ok()?;
            matrix = parent_transform.to_matrix() * matrix;
            current = parent_entity;
        }
        None
    }

    /// True if `entity` has a matched ancestor below `root`, in which case
    /// re-parenting it separately would tear the group apart.
    pub(crate) fn has_matched_ancestor(
        &self,
        entity: Entity,
        root: Entity,
        matched: &[Entity],
    ) -> bool {
        let mut current = entity;
        while let Ok(parent) = self.parents.get(current) {
            let parent_entity = parent.parent();
            if parent_entity == root {
                return false;
            }
            if matched.contains(&parent_entity) {
                return true;
            }
            current = parent_entity;
        }
        false
    }

    /// Local-space bounding box of an entity's mesh.
    fn mesh_bounds(&self, entity: Entity) -> Option<(Vec3, Vec3)> {
        let aabb = match self.aabbs.get(entity) {
            Ok(aabb) => *aabb,
            Err(_) => {
                let handle = self.mesh_handles.get(entity).ok()?;
                self.meshes.get(&handle.0)?.compute_aabb()?
            }
        };
        let center = Vec3::from(aabb.center);
        let extents = Vec3::from(aabb.half_extents);
        Some((center - extents, center + extents))
    }

    /// Bounds of an entity's mesh expressed in `root`'s frame.
    pub(crate) fn bounds_relative_to(&self, entity: Entity, root: Entity) -> Option<(Vec3, Vec3)> {
        let matrix = self.transform_relative_to(entity, root)?;
        let (local_min, local_max) = self.mesh_bounds(entity)?;
        Some(transformed_bounds(&matrix, local_min, local_max))
    }
}

/// Observer that rigs the model as soon as its scene has been spawned.
pub fn rig_model_on_ready(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    roots: Query<&VehicleModelRoot>,
    vehicles: Query<&Vehicle>,
    scene: SceneNodes,
) {
    let root = ready.entity;
    let Ok(model_root) = roots.get(root) else {
        return;
    };
    let Ok(vehicle) = vehicles.get(model_root.vehicle) else {
        warn!("model root {root} has no vehicle");
        return;
    };

    // The active aero assembly is independent from wheels and instruments, but
    // shares this observer so all re-parenting is based on the same untouched
    // scene hierarchy before deferred commands are applied.
    super::spoiler::rig_veyron_spoiler(&mut commands, root, model_root, &scene);
    super::damage::rig_veyron_panels(&mut commands, root, model_root, &scene);

    // ---------------------------------------------------------------- discovery
    let mut assignments: Vec<(Entity, Slot)> = Vec::new();
    let mut unassigned_meshes: Vec<Entity> = Vec::new();
    let mut node_count = 0usize;

    for entity in scene.children.iter_descendants(root) {
        node_count += 1;
        let Ok(name) = scene.names.get(entity) else {
            continue;
        };
        match classify_for(model_root.kind, name.as_str()) {
            Some(slot) => assignments.push((entity, slot)),
            None => {
                if scene.mesh_handles.contains(entity) {
                    unassigned_meshes.push(entity);
                }
            }
        }
    }

    // Keep only the top-most node of each matched group; its children come along
    // for free when it is re-parented.
    let matched: Vec<Entity> = assignments.iter().map(|(entity, _)| *entity).collect();
    let group_roots: Vec<(Entity, Slot)> = assignments
        .iter()
        .copied()
        .filter(|(entity, _)| !scene.has_matched_ancestor(*entity, root, &matched))
        .collect();

    // --------------------------------------------------- measured hub centres
    // The visual hub is measured from the geometry itself rather than assumed,
    // so a differently authored model still lines up. The union of the bounding
    // boxes of the parts that *spin* is centred on the axle by construction -
    // a rim and a tyre are symmetric about it - whereas averaging the individual
    // mesh centres would be pulled off by the rim offset. Brake calipers are
    // deliberately excluded: they sit ahead of and above the axle.
    let mut hub_bounds: [Option<(Vec3, Vec3)>; 4] = [None; 4];
    let mut steering_bounds: Option<(Vec3, Vec3)> = None;

    for (entity, slot) in &group_roots {
        let slot_bounds = match slot {
            Slot::Spin(id) => &mut hub_bounds[id.index()],
            // Brake calipers are not centred on the axle, so they must not
            // influence where the hub is measured to be.
            Slot::Hub(_) => continue,
            Slot::Steering => &mut steering_bounds,
        };
        let group = core::iter::once(*entity).chain(scene.children.iter_descendants(*entity));
        for target in group {
            let Some(bounds) = scene.bounds_relative_to(target, root) else {
                continue;
            };
            *slot_bounds = Some(match *slot_bounds {
                Some((min, max)) => (min.min(bounds.0), max.max(bounds.1)),
                None => bounds,
            });
        }
    }

    let mut hubs = [Vec3::ZERO; 4];
    for id in WheelId::ALL {
        let index = id.index();
        let configured = || {
            let wheel = vehicle.config.wheel(id);
            model_root.model_origin
                + model_space_for(
                    wheel.anchor - Vec3::Y * wheel.suspension.rest_length,
                    model_root.model_scale,
                    model_root.model_yaw,
                )
        };
        // Split nodes share their original POSITION accessor, whose AABB still
        // encloses all four copies. Their semantic corner names are exact, so
        // the configured model measurement is the correct independent pivot.
        let uses_split_geometry = group_roots.iter().any(|(entity, slot)| {
            *slot == Slot::Spin(id)
                && scene
                    .name(*entity)
                    .is_some_and(|name| name.to_ascii_lowercase().starts_with("wheelspin_"))
        });
        hubs[index] = if model_root.kind == VehicleKind::PaganiZondaCinque || uses_split_geometry {
            configured()
        } else {
            match hub_bounds[index] {
                Some((min, max)) => (min + max) * 0.5,
                None => configured(),
            }
        };
    }

    // ------------------------------------------------- positional fallback
    // If a corner produced nothing, claim nearby unclaimed meshes by position.
    let mut extra: Vec<(Entity, Slot)> = Vec::new();
    for id in WheelId::ALL {
        if hub_bounds[id.index()].is_some() {
            continue;
        }
        warn!(
            "no mesh matched {:?} by name; falling back to positional matching",
            id
        );
        // The named Mustang table is deliberately fail-closed: if an archive
        // revision changes its nodes, keeping every static part (especially the
        // driver/pylon) is safer than guessing from proximity.
        if model_root.kind == VehicleKind::MustangGt3 {
            continue;
        }
        let hub = hubs[id.index()];
        for entity in &unassigned_meshes {
            let Some((min, max)) = scene.bounds_relative_to(*entity, root) else {
                continue;
            };
            // Bounds and pivots are expressed in the source glTF's units here.
            // Convert the world-space tolerance back to source space so tiny
            // centimetre-authored cars (Enzo) do not accidentally claim the
            // complete body as a wheel, while large exports (Countach) still
            // get the same useful search radius.
            let source_radius = FALLBACK_RADIUS / model_root.model_scale.abs().max(0.000_1);
            if ((min + max) * 0.5).distance(hub) < source_radius {
                extra.push((*entity, Slot::Spin(id)));
            }
        }
    }

    let configured_steering_center = model_root.model_origin
        + model_space_for(
            vehicle.config.steering_wheel_center,
            model_root.model_scale,
            model_root.model_yaw,
        );
    // Extracted semantic nodes deliberately reuse the source POSITION accessor
    // so all UVs, normals and tangents stay byte-for-byte intact. Its accessor
    // bounds still describe the original complete interior, therefore the
    // measured AABB cannot be used as the steering pivot for those nodes.
    let has_extracted_steering = group_roots.iter().any(|(entity, slot)| {
        *slot == Slot::Steering
            && scene
                .name(*entity)
                .is_some_and(|name| name.to_ascii_lowercase().starts_with(STEERING_PREFIX))
    });
    let steering_center = if has_extracted_steering {
        configured_steering_center
    } else {
        match steering_bounds {
            Some((min, max)) => (min + max) * 0.5,
            None => configured_steering_center,
        }
    };

    // ------------------------------------------------------------- build pivots
    let mut hub_pivots = [Entity::PLACEHOLDER; 4];
    let mut spin_pivots = [Entity::PLACEHOLDER; 4];
    for id in WheelId::ALL {
        let index = id.index();
        let hub_pivot = commands
            .spawn((
                Name::new(format!("WheelHub{}", id.short_name())),
                WheelHubPivot {
                    vehicle: model_root.vehicle,
                    id,
                    model_origin: model_root.model_origin,
                    model_scale: model_root.model_scale,
                    model_yaw: model_root.model_yaw,
                },
                super::damage::wheel_part(
                    model_root,
                    root,
                    id,
                    hubs[index],
                    vehicle.config.wheel(id).anchor
                        - Vec3::Y * vehicle.config.wheel(id).suspension.rest_length,
                ),
                Transform::from_translation(hubs[index]),
                Visibility::default(),
                ChildOf(root),
            ))
            .id();
        let spin_pivot = commands
            .spawn((
                Name::new(format!("WheelSpin{}", id.short_name())),
                WheelSpinPivot {
                    vehicle: model_root.vehicle,
                    id,
                },
                Transform::IDENTITY,
                Visibility::default(),
                ChildOf(hub_pivot),
            ))
            .id();
        hub_pivots[index] = hub_pivot;
        spin_pivots[index] = spin_pivot;
    }

    let steering_pivot = commands
        .spawn((
            Name::new("SteeringWheelPivot"),
            SteeringWheelPivot {
                vehicle: model_root.vehicle,
                model_origin: model_root.model_origin,
                model_scale: model_root.model_scale,
                model_yaw: model_root.model_yaw,
                visual_steering: 0.0,
            },
            Transform::from_translation(steering_center),
            Visibility::default(),
            ChildOf(root),
        ))
        .id();

    // ------------------------------------------------------------- re-parenting
    let mut moved = 0usize;
    for (entity, slot) in group_roots.iter().copied().chain(extra) {
        let Some(matrix) = scene.transform_relative_to(entity, root) else {
            continue;
        };
        let (pivot, pivot_translation) = match slot {
            Slot::Spin(id) => (spin_pivots[id.index()], hubs[id.index()]),
            Slot::Hub(id) => (hub_pivots[id.index()], hubs[id.index()]),
            Slot::Steering => (steering_pivot, steering_center),
        };
        // Rebase: the pivots carry translation only at rig time, so undoing it
        // is a subtraction.
        let rebased = Mat4::from_translation(-pivot_translation) * matrix;
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

    // ----------------------------------------------------------------- report
    report(
        vehicle,
        model_root.model_origin,
        model_root.model_scale,
        model_root.model_yaw,
        &hubs,
        steering_center,
        node_count,
        moved,
    );
}

/// World-space bounds of a local box, by transforming all eight corners.
fn transformed_bounds(matrix: &Mat4, local_min: Vec3, local_max: Vec3) -> (Vec3, Vec3) {
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for index in 0..8 {
        let corner = Vec3::new(
            if index & 1 == 0 {
                local_min.x
            } else {
                local_max.x
            },
            if index & 2 == 0 {
                local_min.y
            } else {
                local_max.y
            },
            if index & 4 == 0 {
                local_min.z
            } else {
                local_max.z
            },
        );
        let point = matrix.transform_point3(corner);
        min = min.min(point);
        max = max.max(point);
    }
    (min, max)
}

/// Log what was found and sanity check it against the configured geometry.
fn report(
    vehicle: &Vehicle,
    model_origin: Vec3,
    model_scale: f32,
    model_yaw: f32,
    hubs: &[Vec3; 4],
    steering: Vec3,
    nodes: usize,
    moved: usize,
) {
    info!("vehicle model: {nodes} nodes scanned, {moved} node groups re-pivoted");
    let source_to_vehicle =
        |point: Vec3| Quat::from_rotation_y(model_yaw) * ((point - model_origin) * model_scale);
    let mut measured_vehicle = [Vec3::ZERO; 4];
    for id in WheelId::ALL {
        let measured = source_to_vehicle(hubs[id.index()]);
        measured_vehicle[id.index()] = measured;
        let wheel = vehicle.config.wheel(id);
        let expected = wheel.anchor - Vec3::Y * wheel.suspension.rest_length;
        let lateral = (measured.x - expected.x).abs();
        let longitudinal = (measured.z - expected.z).abs();
        info!(
            "  {} visual hub ({:.3}, {:.3}, {:.3}) vs physical anchor ({:.3}, {:.3}, {:.3})",
            id.short_name(),
            measured.x,
            measured.y,
            measured.z,
            expected.x,
            expected.y,
            expected.z
        );
        if lateral > 0.20 || longitudinal > 0.20 {
            warn!(
                "  {} visual hub is {:.3} m / {:.3} m away from the configured anchor - \
                 check VehicleConfig::wheels or the model scale",
                id.short_name(),
                lateral,
                longitudinal
            );
        }
    }

    // Cross check the measured geometry against the configuration.
    let front = (measured_vehicle[WheelId::FrontLeft.index()]
        + measured_vehicle[WheelId::FrontRight.index()])
        * 0.5;
    let rear = (measured_vehicle[WheelId::RearLeft.index()]
        + measured_vehicle[WheelId::RearRight.index()])
        * 0.5;
    let measured_wheelbase = (front.z - rear.z).abs();
    let measured_track = (measured_vehicle[WheelId::FrontLeft.index()].x
        - measured_vehicle[WheelId::FrontRight.index()].x)
        .abs();
    info!(
        "  measured wheelbase {:.3} m (config {:.3} m), front track {:.3} m (config {:.3} m)",
        measured_wheelbase,
        vehicle.config.wheel_base,
        measured_track,
        vehicle.config.track_width_front
    );
    if (measured_wheelbase - vehicle.config.wheel_base).abs() > 0.25 {
        warn!("  model scale looks wrong: wheelbase differs by more than 0.25 m");
    }
    info!(
        "  steering wheel pivot at ({:.3}, {:.3}, {:.3})",
        steering.x, steering.y, steering.z
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rim_nodes_map_to_the_right_corner() {
        assert_eq!(
            classify("bug_veyron_09_s_wheel_wheel_outer_rim_"),
            Some(Slot::Spin(WheelId::FrontLeft))
        );
        assert_eq!(
            classify("bug_veyron_09_s_wheel_wheel_outer_rim_1"),
            Some(Slot::Spin(WheelId::FrontRight))
        );
        assert_eq!(
            classify("bug_veyron_09_s_wheel_wheel_inner_rim_2"),
            Some(Slot::Spin(WheelId::RearLeft))
        );
        assert_eq!(
            classify("bug_veyron_09_s_wheel_wheel_rim_bump_metal_3"),
            Some(Slot::Spin(WheelId::RearRight))
        );
    }

    #[test]
    fn tyres_rotors_and_calipers_are_recognised() {
        assert_eq!(
            classify(
                "polySurface482_Bugatti_VeyronSuperSport_2012_Wheel1A_3D_3DWheel1A_Material_0"
            ),
            Some(Slot::Spin(WheelId::FrontLeft))
        );
        assert_eq!(
            classify("bug_veyron_09_rotorlr_s_lod0_rotorLR_brake_chrome_"),
            Some(Slot::Spin(WheelId::RearLeft))
        );
        // Calipers must not spin with the wheel.
        assert_eq!(
            classify("bug_veyron_09_caliperrf_s_lod0_caliperRF_brake_smooth_metal_"),
            Some(Slot::Hub(WheelId::FrontRight))
        );
    }

    #[test]
    fn the_steering_wheel_is_its_own_slot() {
        assert_eq!(
            classify("bug_veyron_09_cockpit_s_steering_wheel_bump_leather_"),
            Some(Slot::Steering)
        );
    }

    #[test]
    fn body_panels_stay_on_the_body() {
        assert_eq!(classify("bug_veyron_09_s_lod0_body_carbon_fiber_"), None);
        assert_eq!(classify("bug_veyron_09_s_lod0_doorLF_rubber_trim_"), None);
        assert_eq!(classify("group2"), None);
    }

    #[test]
    fn mustang_only_rigs_the_explicit_wheel_meshes() {
        assert_eq!(
            classify_for(VehicleKind::MustangGt3, "Object_8"),
            Some(Slot::Spin(WheelId::FrontLeft))
        );
        assert_eq!(
            classify_for(VehicleKind::MustangGt3, "Object_212"),
            Some(Slot::Spin(WheelId::RearRight))
        );
        // Large body/aero groups and every unlisted node — including the
        // supplied driver/pylon — remain untouched on the body hierarchy.
        assert_eq!(classify_for(VehicleKind::MustangGt3, "Object_41"), None);
        assert_eq!(classify_for(VehicleKind::MustangGt3, "Object_116"), None);
        assert_eq!(classify_for(VehicleKind::MustangGt3, "pilot"), None);
    }

    #[test]
    fn processed_wheel_layers_keep_their_exact_corner() {
        assert_eq!(
            classify_for(
                VehicleKind::PaganiZondaCinque,
                "WheelHub_RR_Object_56.caliper.001"
            ),
            Some(Slot::Hub(WheelId::RearRight))
        );
    }

    #[test]
    fn both_extracted_sls_wheel_layers_move_but_column_controls_stay_fixed() {
        assert_eq!(
            classify_for(VehicleKind::MercedesSlsAmg, "SteeringWheel_Object_20"),
            Some(Slot::Steering)
        );
        assert_eq!(
            classify_for(VehicleKind::MercedesSlsAmg, "SteeringWheel_Object_21"),
            Some(Slot::Steering)
        );
        // The remaining source meshes contain the column shroud and stalks;
        // only the explicitly extracted wheel geometry is allowed to rotate.
        assert_eq!(classify_for(VehicleKind::MercedesSlsAmg, "Object_20"), None);
        assert_eq!(classify_for(VehicleKind::MercedesSlsAmg, "Object_21"), None);
    }

    #[test]
    fn extracted_zonda_steering_nodes_are_rigged() {
        assert_eq!(
            classify_for(VehicleKind::PaganiZondaCinque, "SteeringWheel_Object_17"),
            Some(Slot::Steering)
        );
    }

    #[test]
    fn added_hypercar_wheels_and_fixed_calipers_have_semantic_slots() {
        assert_eq!(
            classify_for(VehicleKind::MercedesAmgOne, "amgprojone_rim_025_429"),
            Some(Slot::Spin(WheelId::RearRight))
        );
        assert_eq!(
            classify_for(VehicleKind::KoenigseggJesko, "CaliperLF_27"),
            Some(Slot::Hub(WheelId::FrontLeft))
        );
        assert_eq!(
            classify_for(VehicleKind::LamborghiniRevuelto, "_3DWheel_back_L_321"),
            Some(Slot::Spin(WheelId::RearLeft))
        );
    }

    #[test]
    fn sls_authored_wheel_groups_map_to_the_measured_corners() {
        assert_eq!(
            classify_for(VehicleKind::MercedesSlsAmg, "wheel.001_12"),
            Some(Slot::Spin(WheelId::FrontLeft))
        );
        assert_eq!(
            classify_for(VehicleKind::MercedesSlsAmg, "calliper.002_18"),
            Some(Slot::Hub(WheelId::RearRight))
        );
    }

    #[test]
    fn model_space_is_its_own_inverse() {
        let v = Vec3::new(0.86, 0.34, -1.29);
        assert_eq!(
            super::super::physics::model_space(super::super::physics::model_space(v)),
            v
        );
    }
}
