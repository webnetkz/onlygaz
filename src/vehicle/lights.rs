//! Vehicle lighting: brake lamps and automatic headlights.
//!
//! The glTF's rear light clusters are ordinary meshes wearing ordinary
//! materials, so lighting them up takes three things:
//!
//! 1. **Discovery.** The two `taillight{L,R}` groups and the high mounted centre
//!    strip are picked out by name once the scene is spawned - the same way
//!    [`model_rig`](super::model_rig) finds the wheels - and the centre of each
//!    cluster is *measured* from the geometry rather than written down here, so
//!    a differently authored model still lights up in the right place.
//! 2. **Private materials.** Every lamp surface gets its own clone of the
//!    material it was authored with. The clusters share `lights_chrome` and
//!    `detail_glass_red` with the headlights, the mirrors and the rear side
//!    markers, and writing emissive into a shared asset would light all of
//!    those up as well.
//! 3. **A lamp curve, not a pedal curve.** A brake light switch is a switch: it
//!    closes in the first millimetres of pedal travel and the lamp is at full
//!    output a few tens of milliseconds later, however gently or violently the
//!    car is being stopped. The glow therefore runs on its own timebase instead
//!    of following the deliberately much slower pedal travel in
//!    [`ControlInput`](super::solver::ControlInput).
//!
//! Nothing outside the lamp housings is touched, so the light stays where a real
//! brake light is: the red lens, the lit element behind it, the strip under the
//! rear screen, and a little spill onto the diffuser from a point light at the
//! measured centre of each cluster.

use super::damage::{BreakablePart, DamagePartKind};
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;

use super::VehicleKind;
use super::model_rig::SceneNodes;
use super::physics::{Vehicle, VehicleModelRoot};
use super::solver::ControlInput;
use crate::world::lighting::LightingSettings;

/// Hue of a lit lamp, as a multiplier on its luminance. A red automotive lens
/// passes almost no green and no blue at all; the trace of green is what keeps
/// the bloom around the lamp from tipping over into magenta.
const LAMP_TINT: [f32; 3] = [1.0, 0.028, 0.008];

/// Luminance of the red lens at full brake [cd/m2].
///
/// These are *physical* luminances, exposed by the camera like every other
/// surface in the scene - see `emissive_exposure_weight` below - so they hold up
/// across the lighting tiers and across `VEYRON_EXPOSURE` instead of being
/// tuned to one grading. For scale: sunlit asphalt under this sun is around
/// 2 000 cd/m2 and a sunlit white lane marking around 18 000, and a real brake
/// lamp lens sits between the two, which is exactly why brake lights are hard
/// to pick out in bright sun. These are at the generous end of real, so that the
/// lamps still read at a distance. Setting any of the three to zero takes that
/// surface out of the effect.
const LENS_LUMINANCE: f32 = 8_000.0;
/// Luminance of the lit element inside the housing. Brighter than the lens,
/// because it is the source and the lens is only what the source shines through.
const ELEMENT_LUMINANCE: f32 = 24_000.0;
/// Luminance of the high mounted centre brake light.
const CENTRE_LUMINANCE: f32 = 12_000.0;

/// Luminous power thrown out of one cluster at full brake [lm].
///
/// This is the light that lands on the diffuser and the tarmac, not the light
/// you see coming out of the lens, and in daylight it is a hint rather than a
/// pool. Set to zero to drop the two point lights entirely.
const SPILL_LUMENS: f32 = 900.0;
/// Cut-off radius of that spill [m]. Small, so the lamps stay a local effect and
/// cost the clustered forward renderer almost nothing.
const SPILL_RANGE: f32 = 2.2;
/// How far behind the measured cluster centre the spill light sits [m], so it
/// lights the bodywork around the lamp instead of the inside of the housing.
const SPILL_STANDOFF: f32 = 0.06;

/// How quickly the lamp reaches full output and goes dark again [1/s].
///
/// The Veyron's rear lamps are LEDs, so both are near instant; a couple of
/// frames of ramp is only there to stop the switch from looking digital.
const GLOW_RISE_RATE: f32 = 26.0;
const GLOW_FALL_RATE: f32 = 16.0;

/// Pedal and lever travel at which the brake light switch closes.
///
/// Real cars trip theirs within the first few millimetres of pedal movement,
/// well before any pressure builds in the lines.
const SWITCH_TRAVEL: f32 = 0.02;

/// Change in glow below which the material and the light are left alone, so a
/// steady lamp costs nothing per frame.
const WRITE_EPSILON: f32 = 1.0e-3;

const HEADLIGHT_TINT: [f32; 3] = [1.0, 0.93, 0.78];
const HEADLIGHT_LUMINANCE: f32 = 65_000.0;
/// Luminous output of one complete xenon headlamp assembly [lm]. The renderer
/// treats this as flux inside the cone, so unlike the old omnidirectional point
/// light it is concentrated onto the road ahead.
const HEADLIGHT_SPILL_LUMENS: f32 = 90_000.0;
/// Long cut-off keeps the far end of the beam visible at motorway speed. The
/// light is still allocation-free during daytime because its intensity is zero.
const HEADLIGHT_RANGE: f32 = 140.0;
const HEADLIGHT_INNER_ANGLE: f32 = 11.0_f32.to_radians();
const HEADLIGHT_OUTER_ANGLE: f32 = 18.0_f32.to_radians();
/// State of one car's brake lamps. Lives on the vehicle entity.
#[derive(Debug, Clone, Copy, Default, Component, Reflect)]
#[reflect(Component)]
pub struct BrakeLamps {
    /// 0 when the lamps are dark, 1 when they are at full output.
    pub glow: f32,
}

/// One lit surface of a rear light cluster, carrying its own material clone.
#[derive(Component)]
pub struct BrakeLampSurface {
    vehicle: Entity,
    side: Option<usize>,
    /// Luminance this surface reaches at full brake.
    peak: f32,
    /// Glow the material was last written with.
    written: f32,
}

/// The point light that spills out of one cluster.
#[derive(Component)]
pub struct BrakeLampSpill {
    vehicle: Entity,
    side: usize,
    /// Luminous power at full brake [lm].
    peak: f32,
    /// Glow the light was last written with.
    written: f32,
}

/// One emissive surface belonging to a front headlight.
#[derive(Component)]
pub struct HeadlightSurface {
    vehicle: Entity,
    side: Option<usize>,
    peak: f32,
    written: f32,
}

/// Forward spill from one headlight. It is disabled in daytime rather than
/// despawned, so changing the time-of-day preset is immediate and allocation-free.
#[derive(Component)]
pub struct HeadlightSpill {
    vehicle: Entity,
    side: usize,
    peak: f32,
    written: f32,
}

/// Which lamp a mesh belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lamp {
    /// One of the two rear clusters. `0` is the car's left, `1` its right.
    Cluster(usize),
    /// One authored mesh spans both rear clusters.
    BothClusters,
    /// The high mounted centre brake light under the rear screen.
    Centre,
}

/// Number of rear clusters a car has.
const CLUSTERS: usize = 2;

/// Classify one node name into the lamp it lights and the luminance it reaches.
///
/// Everything else in the housing - the reflector, the reverse light, the chrome
/// surround, the black cup, the clear outer cover - is left exactly as authored,
/// and so are the red side markers on the rear wings, which look like part of
/// the cluster by name but sit half a metre away from it and do not light up
/// with the brakes.
fn classify(name: &str) -> Option<(Lamp, f32)> {
    let lower = name.to_ascii_lowercase();

    let cluster = if lower.contains("taillightl") {
        Some(0)
    } else if lower.contains("taillightr") {
        Some(1)
    } else {
        None
    };

    if let Some(index) = cluster {
        if lower.contains("tail_light") {
            return Some((Lamp::Cluster(index), ELEMENT_LUMINANCE));
        }
        if lower.contains("detail_glass_red") {
            return Some((Lamp::Cluster(index), LENS_LUMINANCE));
        }
        return None;
    }

    if lower.contains("body_detail_glass_red") {
        return Some((Lamp::Centre, CENTRE_LUMINANCE));
    }
    None
}

fn classify_for(kind: VehicleKind, name: &str) -> Option<(Lamp, f32)> {
    match kind {
        VehicleKind::Veyron => classify(name),
        VehicleKind::MustangGt3 => {
            // Bevy names a primitive after `<mesh>.<material>`. Mesh 81 is on
            // the driver's left and mesh 80 on the right in this archive.
            if name.starts_with("Object_81.") {
                Some((Lamp::Cluster(0), ELEMENT_LUMINANCE))
            } else if name.starts_with("Object_80.") {
                Some((Lamp::Cluster(1), ELEMENT_LUMINANCE))
            } else {
                None
            }
        }
        VehicleKind::MercedesSlsAmg if name.starts_with("Object_24.") => {
            Some((Lamp::BothClusters, ELEMENT_LUMINANCE))
        }
        VehicleKind::MercedesSlsAmg if name.starts_with("Object_7.") => {
            Some((Lamp::BothClusters, LENS_LUMINANCE))
        }
        VehicleKind::MercedesSlsAmg if name.starts_with("Object_25.") => {
            Some((Lamp::Centre, CENTRE_LUMINANCE))
        }
        VehicleKind::PaganiZondaCinque if name.starts_with("Object_40.") => {
            Some((Lamp::BothClusters, ELEMENT_LUMINANCE))
        }
        VehicleKind::PaganiZondaCinque
            if name.starts_with("Object_34.") || name.starts_with("Object_38.") =>
        {
            Some((Lamp::Centre, CENTRE_LUMINANCE))
        }
        VehicleKind::MercedesAmgOne => {
            let lower = name.to_ascii_lowercase();
            if lower.contains("taillight") || lower.contains("bumper_r_010") {
                Some((Lamp::BothClusters, ELEMENT_LUMINANCE))
            } else {
                None
            }
        }
        VehicleKind::LamborghiniRevuelto if name.starts_with("Object_2362") => {
            Some((Lamp::Cluster(1), ELEMENT_LUMINANCE))
        }
        VehicleKind::LamborghiniRevuelto if name.starts_with("Object_2365") => {
            Some((Lamp::Cluster(0), ELEMENT_LUMINANCE))
        }
        VehicleKind::LamborghiniRevuelto if name.starts_with("Object_2371") => {
            Some((Lamp::Cluster(1), LENS_LUMINANCE))
        }
        VehicleKind::LamborghiniRevuelto if name.starts_with("Object_2374") => {
            Some((Lamp::Cluster(0), LENS_LUMINANCE))
        }
        VehicleKind::LamborghiniRevuelto if name.starts_with("Object_2389") => {
            Some((Lamp::Centre, CENTRE_LUMINANCE))
        }
        _ if {
            let lower = name.to_ascii_lowercase();
            lower.contains("taillight")
                || lower.contains("tail_light")
                || lower.contains("rear light")
                || lower.contains("tail_lig")
                || lower.contains("red_light")
                || lower.contains("back_main_light")
        } =>
        {
            Some((Lamp::BothClusters, ELEMENT_LUMINANCE))
        }
        _ => None,
    }
}

fn classify_headlight(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("headlightl_head_light")
        || lower.contains("headlightr_head_light")
        || lower.contains("headlightl_detail_glass_clear")
        || lower.contains("headlightr_detail_glass_clear")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeadlightSides {
    One(usize),
    Both,
}

fn headlight_sides(kind: VehicleKind, name: &str) -> Option<HeadlightSides> {
    match kind {
        VehicleKind::Veyron if classify_headlight(name) => Some(HeadlightSides::One(
            if name.to_ascii_lowercase().contains("headlightl") {
                0
            } else {
                1
            },
        )),
        // The two cool-white front lamp meshes in this exact archive. The
        // other mesh sharing that material is interior/body trim and must not
        // be made emissive.
        VehicleKind::MustangGt3 if name.starts_with("Object_83.") => Some(HeadlightSides::One(0)),
        VehicleKind::MustangGt3 if name.starts_with("Object_82.") => Some(HeadlightSides::One(1)),
        // Each of these source meshes contains both lamp assemblies. Their
        // bounds are split around the centre line below to place two beams.
        VehicleKind::MercedesSlsAmg if name.starts_with("Object_19.") => Some(HeadlightSides::Both),
        VehicleKind::PaganiZondaCinque if name.starts_with("Object_4.") => {
            Some(HeadlightSides::Both)
        }
        VehicleKind::MercedesAmgOne => {
            let lower = name.to_ascii_lowercase();
            if lower.contains("headlightframe_r") {
                Some(HeadlightSides::One(1))
            } else if lower.contains("headlightframe_l") {
                Some(HeadlightSides::One(0))
            } else if lower.contains("bumper_f") && lower.contains("runninglight") {
                Some(HeadlightSides::Both)
            } else {
                None
            }
        }
        VehicleKind::LamborghiniRevuelto if name.starts_with("Object_2359") => {
            Some(HeadlightSides::Both)
        }
        _ if {
            let lower = name.to_ascii_lowercase();
            !lower.contains("tail")
                && (lower.contains("headlight")
                    || lower.contains("head_light")
                    || lower.contains("front light")
                    || lower.contains("front_light")
                    || lower.contains("runninglight")
                    || lower.contains("head_lig"))
        } =>
        {
            Some(HeadlightSides::Both)
        }
        _ => None,
    }
}

fn accumulate_bounds(slot: &mut Option<(Vec3, Vec3)>, bounds: (Vec3, Vec3)) {
    *slot = Some(match *slot {
        Some((min, max)) => (min.min(bounds.0), max.max(bounds.1)),
        None => bounds,
    });
}

/// Split the AABB of a symmetric combined lamp mesh into the source model's
/// left (+X) and right (-X) halves.
fn accumulate_cluster_bounds(
    slots: &mut [Option<(Vec3, Vec3)>; CLUSTERS],
    bounds: (Vec3, Vec3),
    both: bool,
    index: usize,
) {
    if both {
        let middle = (bounds.0.x + bounds.1.x) * 0.5;
        let half_width = (bounds.1.x - bounds.0.x) * 0.5;
        let left_x = middle + half_width * 0.72;
        let right_x = middle - half_width * 0.72;
        accumulate_bounds(
            &mut slots[0],
            (
                Vec3::new(left_x, bounds.0.y, bounds.0.z),
                Vec3::new(left_x, bounds.1.y, bounds.1.z),
            ),
        );
        accumulate_bounds(
            &mut slots[1],
            (
                Vec3::new(right_x, bounds.0.y, bounds.0.z),
                Vec3::new(right_x, bounds.1.y, bounds.1.z),
            ),
        );
    } else {
        accumulate_bounds(&mut slots[index], bounds);
    }
}

/// Observer that wires the lamps up as soon as the car's scene is spawned.
///
/// Runs alongside [`model_rig::rig_model_on_ready`](super::model_rig::rig_model_on_ready)
/// and is independent of it: the lamps stay on the body, so nothing here is
/// re-parented.
pub fn wire_brake_lamps_on_ready(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    roots: Query<&VehicleModelRoot>,
    vehicles: Query<&Vehicle>,
    scene: SceneNodes,
    painted: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let root = ready.entity;
    let Ok(model_root) = roots.get(root) else {
        return;
    };
    let vehicle = model_root.vehicle;

    let mut lit = 0usize;
    let mut cluster_bounds: [Option<(Vec3, Vec3)>; CLUSTERS] = [None; CLUSTERS];

    for entity in scene.descendants(root) {
        let Some((lamp, peak)) =
            scene.find_in_ancestry(entity, root, |name| classify_for(model_root.kind, name))
        else {
            continue;
        };
        if peak <= 0.0 {
            continue;
        }
        // Only the nodes that actually carry a mesh have a material to clone;
        // their parent groups match the same name and are skipped here.
        let Ok(current) = painted.get(entity) else {
            continue;
        };
        let Some(mut lamp_material) = materials.get(&current.0).cloned() else {
            continue;
        };

        lamp_material.emissive = LinearRgba::BLACK;
        // A brake lamp is a real light source, so the camera should meter it
        // like one: a weight of 1 puts the emissive luminances below through the
        // same exposure as the rest of the scene, which is what keeps the lamps
        // consistent when the lighting tier or `VEYRON_EXPOSURE` moves the
        // exposure by several stops.
        lamp_material.emissive_exposure_weight = 1.0;

        commands.entity(entity).insert((
            MeshMaterial3d(materials.add(lamp_material)),
            BrakeLampSurface {
                vehicle,
                side: match lamp {
                    Lamp::Cluster(index) => Some(index),
                    _ => None,
                },
                peak,
                // Forces the first frame to write, whatever the glow is.
                written: f32::NAN,
            },
        ));
        lit += 1;

        if let Some(bounds) = scene.bounds_relative_to(entity, root) {
            match lamp {
                Lamp::Cluster(index) => {
                    accumulate_cluster_bounds(&mut cluster_bounds, bounds, false, index)
                }
                Lamp::BothClusters => {
                    accumulate_cluster_bounds(&mut cluster_bounds, bounds, true, 0)
                }
                Lamp::Centre => {}
            }
        }
    }

    if lit == 0 {
        warn!("no emissive brake-lamp surface found; using two geometry-derived rear spill lights");
        if let Ok(vehicle) = vehicles.get(model_root.vehicle) {
            for (index, side) in [-1.0_f32, 1.0].into_iter().enumerate() {
                let vehicle_point = Vec3::new(
                    side * vehicle.config.track_width_rear * 0.38,
                    0.52,
                    vehicle.config.wheel_base * 0.72,
                );
                let source_point = model_root.model_origin
                    + super::physics::model_space_for(
                        vehicle_point,
                        model_root.model_scale,
                        model_root.model_yaw,
                    );
                cluster_bounds[index] = Some((source_point, source_point));
            }
        }
    }

    let mut spills = 0usize;
    if SPILL_LUMENS > 0.0 {
        for (index, bounds) in cluster_bounds.iter().enumerate() {
            let Some((min, max)) = *bounds else {
                continue;
            };
            let source_rear = Quat::from_rotation_y(-model_root.model_yaw) * Vec3::Z;
            let centre = (min + max) * 0.5 + source_rear * SPILL_STANDOFF;
            commands.spawn((
                Name::new(format!(
                    "BrakeLampSpill{}",
                    if index == 0 { "L" } else { "R" }
                )),
                BrakeLampSpill {
                    vehicle,
                    side: index,
                    peak: SPILL_LUMENS,
                    written: f32::NAN,
                },
                PointLight {
                    color: Color::linear_rgb(LAMP_TINT[0], LAMP_TINT[1], LAMP_TINT[2]),
                    intensity: 0.0,
                    range: SPILL_RANGE,
                    // Roughly the size of the lit area, which is what sets how
                    // soft its specular highlight is.
                    radius: 0.07,
                    // A brake lamp casting shadow maps would cost six cube faces
                    // for a glow you cannot see past the bumper.
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::from_translation(centre),
                ChildOf(root),
            ));
            spills += 1;
            info!(
                "  brake lamp {} centred at ({:.3}, {:.3}, {:.3})",
                if index == 0 { "left" } else { "right" },
                centre.x,
                centre.y,
                centre.z
            );
        }
    }
    info!("brake lamps: {lit} surfaces lit, {spills} spill lights");
}

/// Clone the front lamp materials and create one local spill light per side.
pub fn wire_headlights_on_ready(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    roots: Query<&VehicleModelRoot>,
    vehicles: Query<&Vehicle>,
    scene: SceneNodes,
    painted: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let root = ready.entity;
    let Ok(model_root) = roots.get(root) else {
        return;
    };
    let mut lit = 0usize;
    let mut bounds: [Option<(Vec3, Vec3)>; CLUSTERS] = [None; CLUSTERS];

    for entity in scene.descendants(root) {
        let Some(sides) =
            scene.find_in_ancestry(entity, root, |name| headlight_sides(model_root.kind, name))
        else {
            continue;
        };
        let Ok(current) = painted.get(entity) else {
            continue;
        };
        let Some(mut lamp_material) = materials.get(&current.0).cloned() else {
            continue;
        };
        lamp_material.emissive = LinearRgba::BLACK;
        lamp_material.emissive_exposure_weight = 1.0;
        commands.entity(entity).insert((
            MeshMaterial3d(materials.add(lamp_material)),
            HeadlightSurface {
                vehicle: model_root.vehicle,
                side: match sides {
                    HeadlightSides::One(index) => Some(index),
                    _ => None,
                },
                peak: HEADLIGHT_LUMINANCE,
                written: f32::NAN,
            },
        ));
        if let Some(local_bounds) = scene.bounds_relative_to(entity, root) {
            match sides {
                HeadlightSides::One(index) => {
                    accumulate_cluster_bounds(&mut bounds, local_bounds, false, index)
                }
                HeadlightSides::Both => {
                    accumulate_cluster_bounds(&mut bounds, local_bounds, true, 0)
                }
            }
        }
        lit += 1;
    }

    // Even a poorly named archive gets real road illumination. Visible model
    // surfaces are used when discoverable; otherwise beam origins are derived
    // from the physical track and wheelbase and transformed back into the
    // source glTF's coordinate frame.
    if (bounds.iter().all(Option::is_none) || model_root.kind == VehicleKind::LamborghiniRevuelto)
        && let Ok(vehicle) = vehicles.get(model_root.vehicle)
    {
        for (index, side) in [-1.0_f32, 1.0].into_iter().enumerate() {
            let vehicle_point = Vec3::new(
                side * vehicle.config.track_width_front * 0.38,
                0.52,
                -vehicle.config.wheel_base * 0.72,
            );
            let source_point = model_root.model_origin
                + super::physics::model_space_for(
                    vehicle_point,
                    model_root.model_scale,
                    model_root.model_yaw,
                );
            bounds[index] = Some((source_point, source_point));
        }
    }

    let source_forward = Quat::from_rotation_y(-model_root.model_yaw) * Vec3::NEG_Z;
    let source_direction = (source_forward + Vec3::NEG_Y * 0.022).normalize();
    let mut spills = 0usize;
    for (index, local_bounds) in bounds.into_iter().enumerate() {
        let Some(local_bounds) = local_bounds else {
            continue;
        };
        // The normalised model looks along +Z. Put the light just outside the
        // cover and aim it slightly down so its hot spot lands on the tarmac.
        // Using the *front edge* rather than the bounds centre is important
        // when shadows are enabled: a light inside the reflector would be
        // occluded by its own glass and bodywork.
        let centre = (local_bounds.0 + local_bounds.1) * 0.5 + source_forward * 0.25;
        commands.spawn((
            Name::new(format!(
                "HeadlightSpill{}",
                if index == 0 { "L" } else { "R" }
            )),
            HeadlightSpill {
                vehicle: model_root.vehicle,
                side: index,
                peak: HEADLIGHT_SPILL_LUMENS,
                written: f32::NAN,
            },
            SpotLight {
                color: Color::linear_rgb(HEADLIGHT_TINT[0], HEADLIGHT_TINT[1], HEADLIGHT_TINT[2]),
                intensity: 0.0,
                range: HEADLIGHT_RANGE,
                radius: 0.08,
                // The light sits only centimetres ahead of a very complex
                // scanned headlamp/bumper. A shadow map makes that geometry
                // self-occlude the entire beam on the upper quality presets.
                // The sun still supplies world shadows; the low beams stay
                // allocation-light and reliably illuminate the road.
                shadow_maps_enabled: false,
                inner_angle: HEADLIGHT_INNER_ANGLE,
                outer_angle: HEADLIGHT_OUTER_ANGLE,
                ..default()
            },
            Transform::from_translation(centre).looking_to(source_direction, Vec3::Y),
            ChildOf(root),
        ));
        spills += 1;
        info!(
            "  headlight {} at ({:.3}, {:.3}, {:.3})",
            if index == 0 { "left" } else { "right" },
            centre.x,
            centre.y,
            centre.z
        );
    }
    info!("headlights: {lit} surfaces lit, {spills} spill lights");
}

/// Turn the headlights on at night, including both the visible
/// lens emissive and the local forward light spill.
pub fn apply_headlights(
    damage: Query<&BreakablePart>,
    settings: Res<LightingSettings>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut surfaces: Query<(&mut HeadlightSurface, &MeshMaterial3d<StandardMaterial>)>,
    mut spills: Query<(&mut HeadlightSpill, &mut SpotLight)>,
) {
    let surface_glow = if settings.time_of_day.headlights_enabled() {
        1.0
    } else {
        0.0
    };
    let beam_strength = settings.time_of_day.headlight_intensity_multiplier();
    for (mut surface, material) in &mut surfaces {
        let surface_glow = if lamp_broken(&damage, surface.vehicle, surface.side, true) {
            0.0
        } else {
            surface_glow
        };
        if settled(surface.written, surface_glow) {
            continue;
        }
        let Some(mut target) = materials.get_mut(&material.0) else {
            continue;
        };
        target.emissive = LinearRgba::rgb(
            HEADLIGHT_TINT[0] * surface.peak * surface_glow,
            HEADLIGHT_TINT[1] * surface.peak * surface_glow,
            HEADLIGHT_TINT[2] * surface.peak * surface_glow,
        );
        surface.written = surface_glow;
    }
    for (mut spill, mut light) in &mut spills {
        let beam_strength = if lamp_broken(&damage, spill.vehicle, Some(spill.side), true) {
            0.0
        } else {
            beam_strength
        };
        if settled(spill.written, beam_strength) {
            continue;
        }
        light.intensity = spill.peak * beam_strength;
        spill.written = beam_strength;
    }
}

/// Ramp the glow towards whatever the brake light switch is doing.
pub fn drive_brake_lamps(time: Res<Time>, mut query: Query<(&Vehicle, &mut BrakeLamps)>) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for (vehicle, mut lamps) in &mut query {
        let target = step_glow(lamps.glow, switch_closed(&vehicle.input), dt);
        if target != lamps.glow {
            lamps.glow = target;
        }
    }
}

/// Push the current glow into the lamp materials and the spill lights.
pub fn apply_brake_lamps(
    damage: Query<&BreakablePart>,
    lamps: Query<&BrakeLamps>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut surfaces: Query<(&mut BrakeLampSurface, &MeshMaterial3d<StandardMaterial>)>,
    mut spills: Query<(&mut BrakeLampSpill, &mut PointLight)>,
) {
    for (mut surface, material) in &mut surfaces {
        let Ok(state) = lamps.get(surface.vehicle) else {
            continue;
        };
        let glow = if lamp_broken(&damage, surface.vehicle, surface.side, false) {
            0.0
        } else {
            state.glow
        };
        if settled(surface.written, glow) {
            continue;
        }
        let Some(mut target) = materials.get_mut(&material.0) else {
            continue;
        };
        target.emissive = lamp_emissive(surface.peak * glow);
        surface.written = glow;
    }

    for (mut spill, mut light) in &mut spills {
        let Ok(state) = lamps.get(spill.vehicle) else {
            continue;
        };
        let glow = if lamp_broken(&damage, spill.vehicle, Some(spill.side), false) {
            0.0
        } else {
            state.glow
        };
        if settled(spill.written, glow) {
            continue;
        }
        light.intensity = spill.peak * glow;
        spill.written = glow;
    }
}

fn lamp_broken(
    parts: &Query<&BreakablePart>,
    vehicle: Entity,
    side: Option<usize>,
    front: bool,
) -> bool {
    let kind = match (front, side) {
        (true, Some(0)) => DamagePartKind::HeadlightLeft,
        (true, Some(_)) => DamagePartKind::HeadlightRight,
        (false, Some(0)) => DamagePartKind::TaillightLeft,
        (false, Some(_)) => DamagePartKind::TaillightRight,
        (_, None) => return false,
    };
    parts
        .iter()
        .any(|part| part.vehicle == vehicle && part.kind == kind && part.detached)
}

/// True while the brake light switch is made, by pedal or by lever.
///
/// A real handbrake does not light the brake lamps; this one does, because it is
/// the only way a driver watching from behind can tell the car is being pulled
/// up on the lever.
#[inline]
fn switch_closed(input: &ControlInput) -> bool {
    input.brake > SWITCH_TRAVEL || input.handbrake > SWITCH_TRAVEL
}

/// Advance the glow one step towards the switch position.
#[inline]
fn step_glow(glow: f32, closed: bool, dt: f32) -> f32 {
    let (target, rate) = if closed {
        (1.0, GLOW_RISE_RATE)
    } else {
        (0.0, GLOW_FALL_RATE)
    };
    let delta = target - glow;
    let step = rate * dt;
    if delta.abs() <= step {
        target
    } else {
        glow + delta.signum() * step
    }
}

/// Emissive colour of a lamp at a given luminance.
#[inline]
fn lamp_emissive(luminance: f32) -> LinearRgba {
    LinearRgba::rgb(
        LAMP_TINT[0] * luminance,
        LAMP_TINT[1] * luminance,
        LAMP_TINT[2] * luminance,
    )
}

/// True if the last written glow is close enough to the current one to skip the
/// write. A `NAN` in `written` never settles, which is how the first frame is
/// forced through.
#[inline]
fn settled(written: f32, glow: f32) -> bool {
    (glow - written).abs() < WRITE_EPSILON
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lit_element_and_the_lens_are_the_only_parts_that_glow() {
        assert_eq!(
            classify("bug_veyron_09_s_lod0_taillightL_tail_light__lights_chrome_0"),
            Some((Lamp::Cluster(0), ELEMENT_LUMINANCE))
        );
        assert_eq!(
            classify("bug_veyron_09_s_lod0_taillightR_detail_glass_red__detail_glass_red_0"),
            Some((Lamp::Cluster(1), LENS_LUMINANCE))
        );
        for dark in [
            "bug_veyron_09_s_lod0_taillightR_reverse_light__lights_chrome_0",
            "bug_veyron_09_s_lod0_taillightR_textured_reflector__lights_chrome_0",
            "bug_veyron_09_s_lod0_taillightL_lights_chrome__lights_chrome_0",
            "bug_veyron_09_s_lod0_taillightL_black__black_0",
            "bug_veyron_09_s_lod0_taillightL_detail_glass_clear__lights_glass_0",
        ] {
            assert_eq!(classify(dark), None, "{dark}");
        }
    }

    #[test]
    fn the_centre_strip_is_its_own_lamp() {
        assert_eq!(
            classify("bug_veyron_09_s_lod0_body_detail_glass_red__detail_glass_red_0"),
            Some((Lamp::Centre, CENTRE_LUMINANCE))
        );
    }

    #[test]
    fn nothing_outside_the_rear_lamps_is_claimed() {
        // The rear side markers are red glass named after the tail light group,
        // but they sit on the wings and stay dark.
        assert_eq!(
            classify("bug_veyron_09_s_lod0_glassLTL_detail_glass_red__detail_glass_red_0"),
            None
        );
        // The headlights share the `lights_chrome` material with the clusters.
        assert_eq!(
            classify("bug_veyron_09_s_lod0_headlightR_head_light__lights_chrome_0"),
            None
        );
        assert_eq!(classify("bug_veyron_09_s_lod0_body_carbon_fiber_"), None);
    }

    #[test]
    fn headlights_only_claim_the_emitting_elements() {
        assert!(classify_headlight(
            "bug_veyron_09_s_lod0_headlightL_head_light__lights_chrome_0"
        ));
        assert!(classify_headlight(
            "bug_veyron_09_s_lod0_headlightR_detail_glass_clear__lights_chrome_0"
        ));
        assert!(!classify_headlight(
            "bug_veyron_09_s_lod0_headlightL_textured_reflector__lights_chrome_0"
        ));
    }

    #[test]
    fn added_cars_claim_only_their_authored_lamp_meshes() {
        assert_eq!(
            classify_for(VehicleKind::MercedesSlsAmg, "Object_24.tail_run"),
            Some((Lamp::BothClusters, ELEMENT_LUMINANCE))
        );
        assert_eq!(
            classify_for(VehicleKind::PaganiZondaCinque, "Object_40.taillight2S.001"),
            Some((Lamp::BothClusters, ELEMENT_LUMINANCE))
        );
        assert_eq!(
            headlight_sides(VehicleKind::MercedesSlsAmg, "Object_19.head"),
            Some(HeadlightSides::Both)
        );
        assert_eq!(
            headlight_sides(VehicleKind::PaganiZondaCinque, "Object_4.xenonhead.001"),
            Some(HeadlightSides::Both)
        );
        assert_eq!(
            classify_for(
                VehicleKind::MercedesAmgOne,
                "amgprojone_bumper_R_007_amgprojone_taillight_0_215"
            ),
            Some((Lamp::BothClusters, ELEMENT_LUMINANCE))
        );
        assert_eq!(
            headlight_sides(
                VehicleKind::MercedesAmgOne,
                "amgprojone_headlightframe_L_10"
            ),
            Some(HeadlightSides::One(0))
        );
        assert_eq!(
            classify_for(VehicleKind::LamborghiniRevuelto, "Object_2374.Light"),
            Some((Lamp::Cluster(0), LENS_LUMINANCE))
        );
        assert_eq!(
            headlight_sides(VehicleKind::LamborghiniRevuelto, "Object_2359.Light"),
            Some(HeadlightSides::Both)
        );
    }

    #[test]
    fn the_pedal_lights_the_lamps_long_before_it_is_floored() {
        let barely = ControlInput {
            brake: 0.05,
            ..default()
        };
        assert!(switch_closed(&barely));
        assert!(!switch_closed(&ControlInput::default()));
    }

    #[test]
    fn the_handbrake_lights_the_lamps_on_its_own() {
        let lever = ControlInput {
            handbrake: 0.4,
            ..default()
        };
        assert!(switch_closed(&lever));
    }

    #[test]
    fn the_lamp_is_lit_within_a_few_frames_and_dark_again_just_as_fast() {
        let dt = 1.0 / 60.0;
        let mut glow = 0.0;
        let mut frames = 0;
        while glow < 1.0 && frames < 60 {
            glow = step_glow(glow, true, dt);
            frames += 1;
        }
        assert!(frames <= 4, "took {frames} frames to light up");

        frames = 0;
        while glow > 0.0 && frames < 60 {
            glow = step_glow(glow, false, dt);
            frames += 1;
        }
        assert!(frames <= 6, "took {frames} frames to go dark");
    }

    #[test]
    fn brightness_does_not_depend_on_how_hard_the_car_is_stopped() {
        // A brake lamp is a switch, not a dimmer: a brush of the pedal and a
        // panic stop must end up at the same output.
        let dt = 1.0 / 60.0;
        let mut gentle = 0.0;
        let mut panic = 0.0;
        for _ in 0..10 {
            gentle = step_glow(
                gentle,
                switch_closed(&ControlInput {
                    brake: 0.06,
                    ..default()
                }),
                dt,
            );
            panic = step_glow(
                panic,
                switch_closed(&ControlInput {
                    brake: 1.0,
                    ..default()
                }),
                dt,
            );
        }
        assert_eq!(gentle, panic);
        assert_eq!(gentle, 1.0);
    }

    #[test]
    fn a_dark_lamp_emits_nothing() {
        assert_eq!(lamp_emissive(0.0), LinearRgba::BLACK);
        assert!(lamp_emissive(LENS_LUMINANCE).red > lamp_emissive(LENS_LUMINANCE).green);
    }

    #[test]
    fn the_first_frame_always_writes() {
        assert!(!settled(f32::NAN, 0.0));
        assert!(settled(0.5, 0.5));
        assert!(!settled(0.5, 0.9));
    }
}
