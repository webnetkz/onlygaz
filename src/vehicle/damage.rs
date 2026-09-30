//! Localized crash damage for the authored Bugatti model. Each named assembly
//! remains a separate mesh group, so a detached panel is the original part,
//! carried by a small physical rigid body rather than a replacement primitive.

use std::collections::HashMap;

use avian3d::prelude::*;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;

use super::VehicleKind;

/// Debris hits the environment, but cannot kick the car or be mistaken for
/// ground by the ray-cast suspension. Pieces also ignore one another.
pub(crate) const DEBRIS_LAYER: u32 = 1 << 1;
use super::config::WheelId;
use super::model_rig::SceneNodes;
use super::physics::{Vehicle, VehicleModelRoot};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DamagePartKind {
    /// The roof, pillars, fenders and main exterior frame crumple in place.
    BodyShell,
    FrontBumper,
    Hood,
    RearDeck,
    DoorLeft,
    DoorRight,
    MirrorLeft,
    MirrorRight,
    SideGlassLeft,
    SideGlassRight,
    Windshield,
    RearGlass,
    HeadlightLeft,
    HeadlightRight,
    TaillightLeft,
    TaillightRight,
    Wing,
    Wheel(WheelId),
}

impl DamagePartKind {
    fn break_speed(self) -> f32 {
        match self {
            Self::BodyShell => 60.0,
            Self::MirrorLeft | Self::MirrorRight => 5.0,
            Self::SideGlassLeft | Self::SideGlassRight | Self::Windshield | Self::RearGlass => 7.0,
            Self::HeadlightLeft
            | Self::HeadlightRight
            | Self::TaillightLeft
            | Self::TaillightRight => 8.0,
            Self::FrontBumper => 14.0,
            Self::Hood | Self::RearDeck | Self::Wing => 19.0,
            Self::DoorLeft | Self::DoorRight => 23.0,
            Self::Wheel(_) => 28.0,
        }
    }

    fn reach(self) -> f32 {
        match self {
            Self::BodyShell => 4.0,
            Self::MirrorLeft | Self::MirrorRight => 0.95,
            Self::Wheel(_) => 1.00,
            Self::HeadlightLeft
            | Self::HeadlightRight
            | Self::TaillightLeft
            | Self::TaillightRight => 1.05,
            Self::SideGlassLeft | Self::SideGlassRight => 1.15,
            Self::DoorLeft | Self::DoorRight => 1.40,
            Self::Windshield | Self::RearGlass => 1.60,
            Self::Wing => 1.45,
            Self::Hood | Self::RearDeck => 1.65,
            Self::FrontBumper => 2.00,
        }
    }

    fn shatters(self) -> bool {
        matches!(
            self,
            Self::SideGlassLeft | Self::SideGlassRight | Self::Windshield | Self::RearGlass
        )
    }

    fn mass(self) -> f32 {
        match self {
            Self::Wheel(_) => 32.0,
            Self::DoorLeft | Self::DoorRight => 24.0,
            Self::Hood | Self::RearDeck => 16.0,
            Self::FrontBumper => 12.0,
            Self::Wing => 9.0,
            Self::MirrorLeft | Self::MirrorRight => 2.0,
            _ => 3.0,
        }
    }
}

/// One authored mesh assembly and its accumulated localized damage.
#[derive(Component)]
pub struct BreakablePart {
    pub vehicle: Entity,
    pub model_root: Entity,
    pub kind: DamagePartKind,
    pub center_vehicle: Vec3,
    pub size_metres: Vec3,
    pub base_local: Transform,
    pub damage: f32,
    pub detached: bool,
    pub reset_generation: u32,
}

/// Original part after it has left the model hierarchy.
#[derive(Component)]
pub struct DetachedDebris {
    age: f32,
    sensor_time: f32,
    parked: bool,
}

#[derive(Message)]
pub struct PartBroken {
    pub vehicle: Entity,
    pub kind: DamagePartKind,
}

#[derive(Message)]
struct MeshImpact {
    vehicle: Entity,
    point_vehicle: Vec3,
    inward_vehicle: Vec3,
    crush: CrushProfile,
}

/// An impact compresses a volume extending into the car, not just a small
/// sphere on its skin. Energy grows with normal impact speed squared; a
/// 500 km/h frontal stop can consume most of the available crumple depth.
#[derive(Clone, Copy)]
struct CrushProfile {
    depth: f32,
    reach: f32,
    radius: f32,
    limit: f32,
    buckling: f32,
}

impl CrushProfile {
    fn for_impact(normal_speed: f32, body_extent: f32, body_span: f32) -> Self {
        let energy = ((normal_speed - 8.0).max(0.0) / 140.0)
            .powi(2)
            .clamp(0.0, 1.0);
        Self {
            depth: (normal_speed * 0.006).min(0.12) + body_extent * 0.78 * energy,
            reach: 1.25 + body_extent * 1.3 * energy,
            radius: 1.1 + body_span * 0.75 * energy.sqrt(),
            limit: (body_extent * 0.85).max(0.3),
            buckling: energy * 0.1,
        }
    }

    fn in_mesh_space(self, scale: f32) -> Self {
        Self {
            depth: self.depth / scale,
            reach: self.reach / scale,
            radius: self.radius / scale,
            limit: self.limit / scale,
            ..self
        }
    }

    fn weight(self, point: Vec3, hit: Vec3, inward: Vec3) -> f32 {
        let delta = point - hit;
        let along = delta.dot(inward);
        let lateral = (delta - inward * along).length_squared();
        let across = (1.0 - lateral / (self.radius * self.radius)).clamp(0.0, 1.0);
        let inside = (1.0 - along.max(0.0) / self.reach).clamp(0.0, 1.0);
        across * inside.powf(1.35)
    }

    fn deform(self, rest: Vec3, current: Vec3, hit: Vec3, inward: Vec3) -> Vec3 {
        let weight = self.weight(rest, hit, inward);
        // A coherent fold field keeps adjacent vertices/panels together. No
        // random per-vertex jitter or per-frame deformation is involved.
        let fold = 1.0 + self.buckling * ((rest.x + rest.y) * 9.0 + rest.z * 7.0).sin();
        let moved = current + inward * (self.depth * weight * fold);
        rest + (moved - rest).clamp_length_max(self.limit)
    }
}

/// Short-lived glass chips; these are deliberately cheap, unlike the authored
/// panels and wheels which retain their full source geometry.
#[derive(Component)]
struct GlassShard {
    age: f32,
}

#[derive(Component)]
struct DamageMaterialsPrepared;

/// A private copy of an authored material, so a damaged panel darkens without
/// changing the same glTF material on unrelated panels or other cars.
#[derive(Component)]
struct DamageSurface {
    part: Entity,
    original_color: Color,
    original_roughness: f32,
    original_metallic: f32,
    written: f32,
    original_mesh: Handle<Mesh>,
    deformed: bool,
    reset_generation: u32,
}

#[derive(Resource)]
struct ShardAssets {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

pub struct DamagePlugin;

#[derive(Resource)]
pub struct DamageSettings {
    pub enabled: bool,
}

impl Default for DamageSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl Plugin for DamagePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DamageSettings>()
            .add_message::<PartBroken>()
            .add_message::<MeshImpact>()
            .add_systems(Startup, setup_shard_assets)
            .add_systems(
                PhysicsSchedule,
                capture_crash_damage
                    .after(PhysicsStepSystems::Last)
                    .before(super::physics::deflect_from_road_rail),
            )
            .add_systems(
                Update,
                (
                    repair_after_reset,
                    prepare_damage_materials,
                    deform_panels,
                    paint_damage,
                    age_debris,
                )
                    .chain(),
            );
    }
}

fn prepare_damage_materials(
    mut commands: Commands,
    unprepared: Query<(Entity, &Children), (With<BreakablePart>, Without<DamageMaterialsPrepared>)>,
    children: Query<&Children>,
    painted: Query<(&MeshMaterial3d<StandardMaterial>, &Mesh3d)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (part_entity, _) in &unprepared {
        let mut prepared = 0;
        for leaf in children.iter_descendants(part_entity) {
            let Ok((handle, mesh)) = painted.get(leaf) else {
                continue;
            };
            let Some(material) = materials.get(&handle.0).cloned() else {
                continue;
            };
            commands.entity(leaf).insert((
                MeshMaterial3d(materials.add(material.clone())),
                DamageSurface {
                    part: part_entity,
                    original_color: material.base_color,
                    original_roughness: material.perceptual_roughness,
                    original_metallic: material.metallic,
                    written: f32::NAN,
                    original_mesh: mesh.0.clone(),
                    deformed: false,
                    reset_generation: 0,
                },
            ));
            prepared += 1;
        }
        if prepared > 0 {
            commands.entity(part_entity).insert(DamageMaterialsPrepared);
        }
    }
}

/// Vertex dents are evaluated only on impacts. Each affected mesh gets a
/// private copy on its first hit, preserving other cars and the repair source.
fn deform_panels(
    mut impacts: MessageReader<MeshImpact>,
    parts: Query<&BreakablePart>,
    cars: Query<&GlobalTransform, With<Vehicle>>,
    mut surfaces: Query<(&mut DamageSurface, &mut Mesh3d, &GlobalTransform)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    for (mut surface, mut handle, _) in &mut surfaces {
        let Ok(part) = parts.get(surface.part) else {
            continue;
        };
        if surface.reset_generation != part.reset_generation {
            surface.reset_generation = part.reset_generation;
            handle.0 = surface.original_mesh.clone();
            surface.deformed = false;
        }
    }
    for impact in impacts.read() {
        let Ok(car_pose) = cars.get(impact.vehicle) else {
            continue;
        };
        // At 500 km/h physics can be several metres ahead of the rendered
        // hierarchy. Keep the hit in car space and project both from the same
        // visual pose, otherwise a genuine impact can miss every mesh.
        let hit_world = car_pose.transform_point(impact.point_vehicle);
        let inward_world = car_pose.affine().transform_vector3(impact.inward_vehicle);
        for (mut surface, mut handle, global) in &mut surfaces {
            let Ok(part) = parts.get(surface.part) else {
                continue;
            };
            if part.vehicle != impact.vehicle
                || part.kind.shatters()
                || matches!(part.kind, DamagePartKind::Wheel(_))
            {
                continue;
            }
            let inverse = global.affine().inverse();
            let point = inverse.transform_point3(hit_world);
            let inward = inverse.transform_vector3(inward_world).normalize_or_zero();
            let scale = global
                .to_scale_rotation_translation()
                .0
                .abs()
                .max_element()
                .max(0.001);
            let crush = impact.crush.in_mesh_space(scale);
            let Some(original) = meshes.get(&surface.original_mesh) else {
                continue;
            };
            let Some(VertexAttributeValues::Float32x3(rest)) =
                original.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                continue;
            };
            if !rest
                .iter()
                .any(|p| crush.weight(Vec3::from_array(*p), point, inward) > 0.0)
            {
                continue;
            }
            let rest = rest.clone();
            if !surface.deformed {
                let Some(mesh) = meshes.get(&handle.0).cloned() else {
                    continue;
                };
                handle.0 = meshes.add(mesh);
                surface.deformed = true;
            }
            let Some(mut mesh) = meshes.get_mut(&handle.0) else {
                continue;
            };
            if let Some(VertexAttributeValues::Float32x3(positions)) =
                mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
            {
                for (position, rest) in positions.iter_mut().zip(&rest) {
                    let rest = Vec3::from_array(*rest);
                    *position = crush
                        .deform(rest, Vec3::from_array(*position), point, inward)
                        .to_array();
                }
            }
            mesh.compute_smooth_normals();
        }
    }
}

fn paint_damage(
    parts: Query<&BreakablePart>,
    mut surfaces: Query<(&mut DamageSurface, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (mut surface, handle) in &mut surfaces {
        let Ok(part) = parts.get(surface.part) else {
            continue;
        };
        let amount = (part.damage / part.kind.break_speed()).clamp(0.0, 1.0);
        if (amount - surface.written).abs() < 0.02 {
            continue;
        }
        surface.written = amount;
        let Some(mut material) = materials.get_mut(&handle.0) else {
            continue;
        };
        let base = surface.original_color.to_linear();
        let char = 0.52 * amount;
        material.base_color = Color::linear_rgba(
            base.red * (1.0 - char) + 0.045 * char,
            base.green * (1.0 - char) + 0.045 * char,
            base.blue * (1.0 - char) + 0.045 * char,
            base.alpha,
        );
        material.perceptual_roughness = (surface.original_roughness + 0.38 * amount).min(1.0);
        material.metallic = surface.original_metallic * (1.0 - 0.35 * amount);
    }
}

fn setup_shard_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(ShardAssets {
        mesh: meshes.add(Cuboid::new(0.12, 0.012, 0.085)),
        material: materials.add(StandardMaterial {
            base_color: Color::srgba(0.62, 0.82, 0.91, 0.65),
            alpha_mode: AlphaMode::Blend,
            metallic: 0.25,
            perceptual_roughness: 0.15,
            double_sided: true,
            ..default()
        }),
    });
}

fn part_for_name(name: &str) -> Option<DamagePartKind> {
    let name = name.to_ascii_lowercase();
    if name.contains("_bumperfa_") {
        Some(DamagePartKind::FrontBumper)
    } else if name.contains("_doorcardlf_") || name.contains("_doorlf_") {
        Some(DamagePartKind::DoorLeft)
    } else if name.contains("_doorcardrf_") || name.contains("_doorrf_") {
        Some(DamagePartKind::DoorRight)
    } else if name.contains("_mirrorl_") {
        Some(DamagePartKind::MirrorLeft)
    } else if name.contains("_mirrorr_") {
        Some(DamagePartKind::MirrorRight)
    } else if name.contains("_hooda_") {
        Some(DamagePartKind::Hood)
    } else if name.contains("_boot_") {
        Some(DamagePartKind::RearDeck)
    } else if name.contains("_headlightl_") {
        Some(DamagePartKind::HeadlightLeft)
    } else if name.contains("_headlightr_") {
        Some(DamagePartKind::HeadlightRight)
    } else if name.contains("_taillightl_") {
        Some(DamagePartKind::TaillightLeft)
    } else if name.contains("_taillightr_") {
        Some(DamagePartKind::TaillightRight)
    } else if name.contains("_glasslf_window_") {
        Some(DamagePartKind::SideGlassLeft)
    } else if name.contains("_glassrf_window_") {
        Some(DamagePartKind::SideGlassRight)
    } else if name.contains("_glassf_window_") {
        Some(DamagePartKind::Windshield)
    } else if name.contains("_glassr_window_") {
        Some(DamagePartKind::RearGlass)
    } else if name.contains("_s_lod0_body_") || name.contains("_s_lod0_enginebay_") {
        Some(DamagePartKind::BodyShell)
    } else {
        None
    }
}

/// Group all the Veyron's separable bodywork by the semantic names in its glTF.
/// The rig's wheel and active-wing pivots are registered separately.
pub(crate) fn rig_veyron_panels(
    commands: &mut Commands,
    root: Entity,
    model: &VehicleModelRoot,
    scene: &SceneNodes,
) {
    if model.kind != VehicleKind::Veyron {
        return;
    }
    let mut groups: HashMap<DamagePartKind, Vec<Entity>> = HashMap::new();
    for entity in scene.descendants(root) {
        if let Some(kind) = scene.name(entity).and_then(part_for_name) {
            groups.entry(kind).or_default().push(entity);
        }
    }

    for (kind, candidates) in groups {
        let nodes: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|entity| !scene.has_matched_ancestor(*entity, root, &candidates))
            .collect();
        let mut bounds: Option<(Vec3, Vec3)> = None;
        for node in &nodes {
            for target in core::iter::once(*node).chain(scene.descendants(*node)) {
                let Some((min, max)) = scene.bounds_relative_to(target, root) else {
                    continue;
                };
                bounds = Some(match bounds {
                    Some((old_min, old_max)) => (old_min.min(min), old_max.max(max)),
                    None => (min, max),
                });
            }
        }
        let Some((min, max)) = bounds else {
            warn!("damage part {:?} has no mesh bounds", kind);
            continue;
        };
        let source_center = (min + max) * 0.5;
        let base_local = Transform::from_translation(source_center);
        let pivot = commands
            .spawn((
                Name::new(format!("Breakable {kind:?}")),
                BreakablePart {
                    vehicle: model.vehicle,
                    model_root: root,
                    kind,
                    center_vehicle: Quat::from_rotation_y(model.model_yaw)
                        * ((source_center - model.model_origin) * model.model_scale),
                    size_metres: ((max - min) * model.model_scale.abs()).max(Vec3::splat(0.08)),
                    base_local,
                    damage: 0.0,
                    detached: false,
                    reset_generation: 0,
                },
                base_local,
                Visibility::default(),
                ChildOf(root),
            ))
            .id();
        for node in nodes {
            let Some(matrix) = scene.transform_relative_to(node, root) else {
                continue;
            };
            let rebased = Mat4::from_translation(-source_center) * matrix;
            let (scale, rotation, translation) = rebased.to_scale_rotation_translation();
            commands.entity(node).insert((
                Transform {
                    translation,
                    rotation,
                    scale,
                },
                ChildOf(pivot),
            ));
        }
    }
}

pub(crate) fn wheel_part(
    model: &VehicleModelRoot,
    root: Entity,
    id: WheelId,
    hub_source: Vec3,
    center_vehicle: Vec3,
) -> BreakablePart {
    BreakablePart {
        vehicle: model.vehicle,
        model_root: root,
        kind: DamagePartKind::Wheel(id),
        center_vehicle,
        size_metres: Vec3::new(0.34, 0.73, 0.73),
        base_local: Transform::from_translation(hub_source),
        damage: 0.0,
        detached: false,
        reset_generation: 0,
    }
}

pub(crate) fn wing_part(
    model: &VehicleModelRoot,
    root: Entity,
    pivot_source: Vec3,
    size_source: Vec3,
) -> BreakablePart {
    BreakablePart {
        vehicle: model.vehicle,
        model_root: root,
        kind: DamagePartKind::Wing,
        center_vehicle: Quat::from_rotation_y(model.model_yaw)
            * ((pivot_source - model.model_origin) * model.model_scale),
        size_metres: (size_source * model.model_scale.abs()).max(Vec3::splat(0.08)),
        base_local: Transform::from_translation(pivot_source),
        damage: 0.0,
        detached: false,
        reset_generation: 0,
    }
}

/// The collision solver supplies both a contact point and a normal impulse.
/// Damage is distributed to nearby assemblies, so a door hit cannot tear off
/// the opposite mirror or a rear collision break the front bumper.
fn capture_crash_damage(
    mut commands: Commands,
    settings: Res<DamageSettings>,
    mut starts: MessageReader<CollisionStart>,
    contacts: Res<ContactGraph>,
    shards: Res<ShardAssets>,
    mut broken: MessageWriter<PartBroken>,
    mut impacts: MessageWriter<MeshImpact>,
    mut cars: Query<(Entity, &mut Vehicle, &Position, &Rotation, &LinearVelocity), With<Vehicle>>,
    mut parts: Query<(Entity, &mut BreakablePart, &mut Transform, &GlobalTransform)>,
    names: Query<&Name>,
    debris: Query<(), Or<(With<DetachedDebris>, With<GlassShard>)>>,
    mut spoilers: Query<&mut super::spoiler::ActiveSpoiler>,
) {
    if !settings.enabled {
        starts.clear();
        return;
    }
    for collision in starts.read() {
        let vehicle_entity = [collision.collider1, collision.collider2]
            .into_iter()
            .chain(collision.body1)
            .chain(collision.body2)
            .find(|entity| cars.contains(*entity));
        let Some(vehicle_entity) = vehicle_entity else {
            continue;
        };
        let Ok((_, mut vehicle, position, rotation, velocity)) = cars.get_mut(vehicle_entity)
        else {
            continue;
        };
        let other = if collision.collider1 == vehicle_entity {
            collision.collider2
        } else {
            collision.collider1
        };
        if debris.contains(other)
            || names
                .get(other)
                .is_ok_and(|name| matches!(name.as_str(), "RoadCollider" | "TerrainCollider"))
        {
            continue;
        }
        let Some((_, pair)) = contacts.get(collision.collider1, collision.collider2) else {
            continue;
        };
        let Some(strongest) = pair
            .manifolds
            .iter()
            .flat_map(|manifold| &manifold.points)
            .max_by(|a, b| a.normal_impulse.total_cmp(&b.normal_impulse))
        else {
            continue;
        };
        let severity = (-strongest.normal_speed)
            .max(pair.max_normal_impulse_magnitude() / vehicle.config.mass.max(1.0));
        if severity < 1.5 || !severity.is_finite() {
            continue;
        }
        // A box pressed sideways against a wall reports corners at its front
        // and rear, not a contact at the centre of the door. Their centroid is
        // the actual impacted side of the car and gives sensible door damage.
        let (point_sum, point_count) = pair
            .manifolds
            .iter()
            .flat_map(|manifold| &manifold.points)
            .fold((Vec3::ZERO, 0usize), |(sum, count), point| {
                (sum + point.point, count + 1)
            });
        let hit_point = point_sum / point_count as f32;
        let local_hit = rotation.0.inverse() * (hit_point - position.0);
        let impulse = pair.max_normal_impulse();
        let car_impulse = if pair.collider1 == vehicle_entity || pair.body1 == Some(vehicle_entity)
        {
            -impulse
        } else {
            impulse
        };
        let car_impulse_dir = car_impulse.normalize_or_zero();
        let (min, max) = vehicle.config.collider_boxes.iter().fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(min, max), (center, half)| (min.min(*center - *half), max.max(*center + *half)),
        );
        let size = (max - min).max(Vec3::splat(0.5));
        let local_normal = rotation.0.inverse() * car_impulse_dir;
        let body_extent = local_normal.abs().dot(size).max(0.5);
        impacts.write(MeshImpact {
            vehicle: vehicle_entity,
            point_vehicle: local_hit,
            inward_vehicle: local_normal,
            crush: CrushProfile::for_impact(severity, body_extent, size.max_element()),
        });

        for (part_entity, mut part, mut transform, global) in &mut parts {
            if part.vehicle != vehicle_entity || part.detached {
                continue;
            }
            let proximity = damage_proximity(local_hit, part.center_vehicle, part.kind.reach());
            if proximity <= 0.0 {
                continue;
            }
            part.damage += severity * proximity;
            if part.kind == DamagePartKind::BodyShell {
                // The shell stays on the chassis even when fully crushed.
                part.damage = part.damage.min(part.kind.break_speed() * 0.9);
                dent_part(
                    &part,
                    &mut transform,
                    rotation.0.inverse() * car_impulse_dir,
                );
                continue;
            }
            if part.damage < part.kind.break_speed() {
                dent_part(
                    &part,
                    &mut transform,
                    rotation.0.inverse() * car_impulse_dir,
                );
                continue;
            }
            part.detached = true;
            broken.write(PartBroken {
                vehicle: vehicle_entity,
                kind: part.kind,
            });
            if part.kind.shatters() {
                commands.entity(part_entity).insert(Visibility::Hidden);
                spawn_glass_shards(&mut commands, &shards, hit_point, velocity.0, severity);
                continue;
            }

            if let DamagePartKind::Wheel(id) = part.kind {
                vehicle.dynamics.detached_wheels[id.index()] = true;
            } else if part.kind == DamagePartKind::Wing {
                if let Ok(mut spoiler) = spoilers.get_mut(vehicle_entity) {
                    spoiler.wing_detached = true;
                }
            }

            let (scale, world_rotation, world_position) = global.to_scale_rotation_translation();
            let kick = -car_impulse_dir * (severity * 0.52).min(24.0)
                + Vec3::Y * (severity * 0.10).min(4.0);
            let size = part.size_metres.clamp(Vec3::splat(0.08), Vec3::splat(2.8));
            commands
                .entity(part_entity)
                .remove::<ChildOf>()
                .insert((
                    Transform {
                        translation: world_position,
                        rotation: world_rotation,
                        scale,
                    },
                    RigidBody::Dynamic,
                    Collider::cuboid(size.x, size.y, size.z),
                    Mass(part.kind.mass()),
                    AngularInertia::new(box_inertia(size, part.kind.mass())),
                    NoAutoMass,
                    NoAutoAngularInertia,
                ))
                .insert((
                    CollisionLayers::from_bits(DEBRIS_LAYER, !DEBRIS_LAYER),
                    Friction::new(0.55),
                    Restitution::new(0.22),
                    LinearVelocity(velocity.0 + kick),
                    AngularVelocity(Vec3::new(3.0, -2.2, 4.0) * proximity),
                    LinearDamping(0.25),
                    AngularDamping(0.9),
                    SweptCcd::default(),
                    Sensor,
                    DetachedDebris {
                        age: 0.0,
                        sensor_time: 0.18,
                        parked: false,
                    },
                ));
        }
    }
}

fn damage_proximity(hit: Vec3, center: Vec3, reach: f32) -> f32 {
    let distance = hit.distance(center);
    if distance >= reach {
        0.0
    } else {
        (1.0 - distance / reach).sqrt()
    }
}

fn box_inertia(size: Vec3, mass: f32) -> Vec3 {
    mass / 12.0
        * Vec3::new(
            size.y * size.y + size.z * size.z,
            size.x * size.x + size.z * size.z,
            size.x * size.x + size.y * size.y,
        )
        .max(Vec3::splat(0.001))
}

fn dent_part(part: &BreakablePart, transform: &mut Transform, inward_vehicle: Vec3) {
    if part.kind.shatters()
        || matches!(
            part.kind,
            DamagePartKind::Wheel(_) | DamagePartKind::BodyShell
        )
    {
        return;
    }
    let fraction = (part.damage / part.kind.break_speed()).clamp(0.0, 1.0);
    let local_inward =
        Vec3::new(-inward_vehicle.x, inward_vehicle.y, -inward_vehicle.z).normalize_or_zero();
    transform.translation = part.base_local.translation + local_inward * (0.11 * fraction);
    transform.rotation =
        part.base_local.rotation * Quat::from_rotation_y(local_inward.x * 0.12 * fraction);
}

fn spawn_glass_shards(
    commands: &mut Commands,
    assets: &ShardAssets,
    point: Vec3,
    car_velocity: Vec3,
    severity: f32,
) {
    for index in 0..8 {
        let angle = index as f32 * core::f32::consts::TAU / 8.0;
        let outward = Vec3::new(angle.cos(), 0.0, angle.sin());
        commands.spawn((
            Name::new("Shattered vehicle glass"),
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.material.clone()),
            Transform::from_translation(point + outward * 0.08)
                .with_rotation(Quat::from_rotation_y(angle)),
            RigidBody::Dynamic,
            Collider::cuboid(0.12, 0.012, 0.085),
            Mass(0.08),
            AngularInertia::new(box_inertia(Vec3::new(0.12, 0.012, 0.085), 0.08)),
            NoAutoMass,
            NoAutoAngularInertia,
            LinearVelocity(
                car_velocity * 0.4
                    + outward * (severity * 0.16).clamp(1.0, 5.0)
                    + Vec3::Y * (1.0 + index as f32 * 0.24),
            ),
            AngularVelocity(Vec3::new(index as f32 * 0.7, 3.0, -2.0)),
            (
                Sensor,
                CollisionLayers::from_bits(DEBRIS_LAYER, !DEBRIS_LAYER),
            ),
            GlassShard { age: 0.0 },
        ));
    }
}

fn age_debris(
    mut commands: Commands,
    time: Res<Time>,
    mut pieces: Query<(Entity, &mut DetachedDebris, &mut Visibility)>,
    mut shards: Query<(Entity, &mut GlassShard)>,
) {
    let dt = time.delta_secs();
    for (entity, mut piece, mut visibility) in &mut pieces {
        piece.age += dt;
        if piece.age >= piece.sensor_time && piece.age - dt < piece.sensor_time {
            commands.entity(entity).remove::<Sensor>();
        }
        if piece.age > 35.0 && !piece.parked {
            piece.parked = true;
            *visibility = Visibility::Hidden;
            commands
                .entity(entity)
                .remove::<(RigidBody, Collider, LinearVelocity, AngularVelocity)>();
        }
    }
    for (entity, mut shard) in &mut shards {
        shard.age += dt;
        if shard.age > 12.0 {
            commands.entity(entity).despawn();
        } else if shard.age >= 0.1 && shard.age - dt < 0.1 {
            commands.entity(entity).remove::<Sensor>();
        }
    }
}

fn repair_after_reset(
    mut commands: Commands,
    cars: Query<&Vehicle>,
    mut parts: Query<(Entity, &mut BreakablePart, &mut Transform, &mut Visibility)>,
    mut spoilers: Query<&mut super::spoiler::ActiveSpoiler>,
) {
    for (entity, mut part, mut transform, mut visibility) in &mut parts {
        let Ok(car) = cars.get(part.vehicle) else {
            continue;
        };
        if car.reset_generation == part.reset_generation {
            continue;
        }
        part.reset_generation = car.reset_generation;
        part.damage = 0.0;
        if part.detached {
            commands.entity(entity).remove::<(
                DetachedDebris,
                RigidBody,
                Collider,
                Mass,
                AngularInertia,
                NoAutoMass,
                NoAutoAngularInertia,
                Friction,
                Restitution,
                LinearVelocity,
                AngularVelocity,
                LinearDamping,
                AngularDamping,
                SweptCcd,
                Sensor,
            )>();
            commands
                .entity(entity)
                .remove::<(Position, Rotation, CollisionLayers)>();
            commands.entity(entity).insert(ChildOf(part.model_root));
            part.detached = false;
        }
        *transform = part.base_local;
        *visibility = Visibility::Inherited;
        if part.kind == DamagePartKind::Wing {
            if let Ok(mut spoiler) = spoilers.get_mut(part.vehicle) {
                spoiler.wing_detached = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_bugatti_groups_include_the_deformable_main_shell() {
        assert_eq!(
            part_for_name("bug_veyron_09_s_lod0_doorLF_body_"),
            Some(DamagePartKind::DoorLeft)
        );
        assert_eq!(
            part_for_name("bug_veyron_09_cockpit_s_mirrorR_body_"),
            Some(DamagePartKind::MirrorRight)
        );
        assert_eq!(
            part_for_name("bug_veyron_09_s_lod0_bumperFa_body_"),
            Some(DamagePartKind::FrontBumper)
        );
        assert_eq!(
            part_for_name("bug_veyron_09_s_lod0_body_body_"),
            Some(DamagePartKind::BodyShell)
        );
    }

    #[test]
    fn five_hundred_kmh_frontal_impact_crushes_the_shell_deeply() {
        let hit = Vec3::new(0.0, 0.5, -2.2);
        let front = Vec3::new(0.4, 0.8, -2.0);
        let gentle = CrushProfile::for_impact(15.0, 4.4, 4.4);
        let severe = CrushProfile::for_impact(520.0 / 3.6, 4.4, 4.4);
        let small_dent = gentle.deform(front, front, hit, Vec3::Z);
        let crushed = severe.deform(front, front, hit, Vec3::Z);
        assert!((small_dent - front).length() < 0.15);
        assert!((crushed - front).length() > 2.5);
        let roof = Vec3::new(0.0, 1.2, 0.0);
        assert!(severe.deform(roof, roof, hit, Vec3::Z).distance(roof) > 0.8);
        let far_away = Vec3::new(10.0, 0.5, -2.0);
        assert_eq!(severe.deform(far_away, far_away, hit, Vec3::Z), far_away);
    }

    #[test]
    fn repeated_extreme_impacts_keep_vertex_displacement_finite_and_bounded() {
        let profile = CrushProfile::for_impact(10_000.0, 2.0, 4.4);
        let rest = Vec3::new(-0.9, 0.8, 0.0);
        let mut current = rest;
        for _ in 0..100 {
            current = profile.deform(rest, current, Vec3::new(-1.0, 0.5, 0.0), Vec3::X);
        }
        assert!(current.is_finite());
        assert!(current.distance(rest) <= profile.limit + 0.0001);
    }

    #[test]
    fn hits_fall_off_by_distance_and_do_not_reach_opposite_parts() {
        assert_eq!(damage_proximity(Vec3::X * 2.0, Vec3::ZERO, 1.0), 0.0);
        assert!(damage_proximity(Vec3::ZERO, Vec3::ZERO, 1.0) > 0.9);
    }
}
