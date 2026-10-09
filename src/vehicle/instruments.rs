//! Animated, illuminated instruments in the supplied vehicle cockpits.
//!
//! The source model already contains separate meshes for every analogue needle
//! and a separate layer for the dial markings.  The needle meshes were exported
//! at their maximum readings with their pivots baked into the car mesh, so they
//! cannot simply be rotated in place.  Once the scene is ready this module
//! reparents them under measured, model-space pivots, then drives those pivots
//! from the real simulation state.

use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;

use super::VehicleKind;
use super::model_rig::SceneNodes;
use super::physics::{Vehicle, VehicleModelRoot};
use crate::camera::controller::GameCamera;
#[cfg(test)]
use crate::camera::modes::CameraMode;
use crate::world::lighting::{LightingQuality, LightingSettings, TimeOfDay};

/// Cool white/blue electroluminescent lighting used by the original cluster.
const BACKLIGHT_TINT: [f32; 3] = [0.48, 0.72, 1.0];
const SCALE_LUMINANCE: f32 = 70.0;
const DISPLAY_LUMINANCE: f32 = 25.0;
const NEEDLE_LUMINANCE: f32 = 55.0;
const BUTTON_LUMINANCE: f32 = 6.0;
const CABIN_GLOW_LUMENS: f32 = 30.0;
const CABIN_GLOW_RANGE: f32 = 0.85;
/// Upper graphics tiers correctly shadow the roof and pillars, but the current
/// renderer has no real-time bounced skylight to replace the light they block.
/// This local fill approximates that missing indirect light without changing
/// the exterior exposure or the deliberately inexpensive Performance look.
const COCKPIT_FILL_DAY_LUMENS: f32 = 18_000.0;
const COCKPIT_FILL_NIGHT_LUMENS: f32 = 18.0;
const COCKPIT_FILL_RANGE: f32 = 2.15;
const COCKPIT_FILL_RESPONSE: f32 = 10.0;

/// The dial plane is tilted very slightly in the model. This is its normal in
/// the `VehicleModel` frame, pointing from the glass towards the driver.
const DIAL_AXIS: Vec3 = Vec3::new(0.0, 0.012_711, -0.999_919);

/// The exported needles all point at their maximum values.  The real dials run
/// from roughly seven o'clock, over the top, to five o'clock: a 253 degree arc.
const MAX_TO_ZERO_RADIANS: f32 = 253.0_f32.to_radians();
const NEEDLE_RESPONSE: f32 = 14.0;
const WRITE_EPSILON: f32 = 1.0e-3;
const ZONDA_POINTER_LENGTH: f32 = 0.064;
const ZONDA_POINTER_WIDTH: f32 = 0.0045;
const ZONDA_POINTER_HUB_RADIUS: f32 = 0.0065;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstrumentKind {
    Tachometer,
    Speedometer,
    Power,
    OilTemperature,
    Fuel,
}

impl InstrumentKind {
    /// Centres measured from the circular hubs in the supplied 1:1 glTF.
    fn pivot(self) -> Vec3 {
        match self {
            Self::Tachometer => Vec3::new(0.386_986, 0.824_185, 0.780_624),
            Self::Speedometer => Vec3::new(0.292_086, 0.780_735, 0.758_370),
            Self::Power => Vec3::new(0.481_946, 0.780_168, 0.760_463),
            Self::OilTemperature => Vec3::new(0.472_913, 0.847_283, 0.780_918),
            Self::Fuel => Vec3::new(0.300_990, 0.847_283, 0.780_918),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Tachometer => "Tachometer",
            Self::Speedometer => "Speedometer",
            Self::Power => "PowerGauge",
            Self::OilTemperature => "OilTemperatureGauge",
            Self::Fuel => "FuelGauge",
        }
    }

    fn target(self, vehicle: &Vehicle, dial_maximum: f32) -> f32 {
        match self {
            Self::Tachometer => vehicle.dynamics.rpm() / dial_maximum,
            Self::Speedometer => vehicle.speed_kmh().abs() / dial_maximum,
            Self::Power => {
                let rpm = vehicle.dynamics.rpm();
                let torque = vehicle.config.engine.torque_curve.torque_at(rpm);
                let watts = torque * rpm * core::f32::consts::TAU / 60.0 * vehicle.input.throttle;
                let peak_watts = vehicle.config.engine.torque_curve.peak_power().0;
                watts / peak_watts.max(1.0)
            }
            // There is no thermal or fuel-consumption simulation yet. Keeping
            // these at believable running values is preferable to leaving the
            // source meshes pinned at their exported maximum positions.
            Self::OilTemperature => 0.55,
            Self::Fuel => 0.72,
        }
        .clamp(0.0, 1.0)
    }
}

/// Model-specific geometry and graduation for one authored analogue pointer.
#[derive(Debug, Clone, Copy)]
struct NeedleSpec {
    kind: InstrumentKind,
    pivot: Vec3,
    axis: Vec3,
    sweep: f32,
    dial_maximum: f32,
}

/// A pivot driving one of the five analogue needles.
#[derive(Component)]
pub struct InstrumentNeedle {
    vehicle: Entity,
    kind: InstrumentKind,
    displayed: f32,
    axis: Vec3,
    sweep: f32,
    dial_maximum: f32,
}

/// A private emissive material belonging to the instrument cluster.
#[derive(Component)]
pub struct InstrumentGlowSurface {
    peak: f32,
    written: f32,
}

/// Very small local light which lets the luminous dials tint the wheel rim and
/// nearby leather. Emissive materials alone do not illuminate other meshes.
#[derive(Component)]
pub struct InstrumentPanelLight {
    peak: f32,
    written: f32,
}

/// Short-range approximation of daylight bouncing through the windscreen.
/// It exists only on the Veyron and is enabled only for its forward cockpit
/// view on the two tiers whose accurate shadows otherwise black out the cabin.
#[derive(Component)]
pub struct CockpitFillLight {
    vehicle: Entity,
}

/// Add the Veyron's indirect cabin fill to the physical car so it follows the
/// chassis and is removed automatically when the player selects another car.
pub fn spawn_cockpit_fill(commands: &mut Commands, vehicle: Entity, kind: VehicleKind) {
    if kind != VehicleKind::Veyron {
        return;
    }

    commands.spawn((
        Name::new("BugattiCockpitIndirectFill"),
        CockpitFillLight { vehicle },
        PointLight {
            // A slightly cool neutral tint reads as sky entering through glass,
            // rather than as another dashboard lamp.
            color: Color::srgb(0.78, 0.86, 1.0),
            intensity: 0.0,
            range: COCKPIT_FILL_RANGE,
            radius: 0.42,
            shadow_maps_enabled: false,
            ..default()
        },
        // Inside the centre console, slightly behind the instrument faces. At
        // this height inverse-square falloff favours the wheel and dashboard
        // instead of creating a bright patch on the headlining.
        Transform::from_xyz(0.0, 0.68, -0.32),
        ChildOf(vehicle),
    ));
}

fn cockpit_fill_target(
    settings: &LightingSettings,
    kind: VehicleKind,
    camera: Option<&GameCamera>,
    vehicle: Entity,
) -> f32 {
    let visible = kind == VehicleKind::Veyron
        && matches!(
            settings.quality,
            LightingQuality::Balanced | LightingQuality::Cinematic
        )
        && camera.is_some_and(|camera| {
            camera.target == vehicle && camera.mode.is_interior() && !camera.rear_view
        });
    if !visible {
        return 0.0;
    }

    match settings.time_of_day {
        TimeOfDay::Day => COCKPIT_FILL_DAY_LUMENS,
        TimeOfDay::Night => COCKPIT_FILL_NIGHT_LUMENS,
    }
}

/// Fade the indirect fill with camera, quality and time-of-day changes so
/// cycling views never produces a visible lighting pop.
pub fn apply_cockpit_fill(
    time: Res<Time>,
    settings: Res<LightingSettings>,
    cameras: Query<&GameCamera>,
    vehicles: Query<&VehicleKind>,
    mut fills: Query<(&CockpitFillLight, &mut PointLight)>,
) {
    let camera = cameras.iter().next();
    let blend = 1.0 - (-COCKPIT_FILL_RESPONSE * time.delta_secs()).exp();
    for (fill, mut light) in &mut fills {
        let kind = vehicles
            .get(fill.vehicle)
            .copied()
            .unwrap_or(VehicleKind::MustangGt3);
        let target = cockpit_fill_target(&settings, kind, camera, fill.vehicle);
        light.intensity += (target - light.intensity) * blend;
        if light.intensity < 0.01 {
            light.intensity = 0.0;
        }
    }
}

fn classify_needle(name: &str) -> Option<InstrumentKind> {
    let lower = name.to_ascii_lowercase();
    if lower.contains("cockpit_s_tach_max_") {
        Some(InstrumentKind::Tachometer)
    } else if lower.contains("cockpit_s_speed_max_") {
        Some(InstrumentKind::Speedometer)
    } else if lower.contains("cockpit_s_power_max_") {
        Some(InstrumentKind::Power)
    } else if lower.contains("cockpit_s_oiltemp_max_") {
        Some(InstrumentKind::OilTemperature)
    } else if lower.contains("cockpit_s_fuel_max_") {
        Some(InstrumentKind::Fuel)
    } else {
        None
    }
}

fn needle_spec_for(vehicle: VehicleKind, name: &str) -> Option<NeedleSpec> {
    let lower = name.to_ascii_lowercase();
    let (kind, pivot, axis, sweep, dial_maximum) = match vehicle {
        VehicleKind::Veyron => {
            let kind = classify_needle(name)?;
            let maximum = match kind {
                InstrumentKind::Tachometer => 8_000.0,
                InstrumentKind::Speedometer => 420.0,
                InstrumentKind::Power | InstrumentKind::OilTemperature | InstrumentKind::Fuel => {
                    1.0
                }
            };
            (kind, kind.pivot(), DIAL_AXIS, MAX_TO_ZERO_RADIANS, maximum)
        }
        VehicleKind::MercedesSlsAmg if lower.contains("instrumentneedle_tachometer_sls") => (
            InstrumentKind::Tachometer,
            Vec3::new(0.328_85, 0.838_10, 0.188_40),
            Vec3::new(0.0, 0.32, -0.95),
            250.0_f32.to_radians(),
            8_000.0,
        ),
        VehicleKind::MercedesSlsAmg if lower.contains("instrumentneedle_speedometer_sls") => (
            InstrumentKind::Speedometer,
            Vec3::new(0.531_40, 0.837_50, 0.188_50),
            Vec3::new(0.0, 0.32, -0.95),
            250.0_f32.to_radians(),
            360.0,
        ),
        VehicleKind::PaganiZondaCinque if lower.contains("instrumentneedle_tachometer_zonda") => (
            InstrumentKind::Tachometer,
            Vec3::new(0.399_0, 0.645_1, 0.815_7),
            Vec3::new(0.0, 0.27, -0.96),
            180.0_f32.to_radians(),
            8_000.0,
        ),
        VehicleKind::PaganiZondaCinque if lower.contains("instrumentneedle_speedometer_zonda") => (
            InstrumentKind::Speedometer,
            // Centre measured from the supplied speedometer pointer and hub.
            // The former x=0.3135 value is the midpoint of the complete
            // cluster, which placed the live pointer between the two dials.
            Vec3::new(0.229_1, 0.645_1, 0.815_7),
            Vec3::new(0.0, 0.27, -0.96),
            270.0_f32.to_radians(),
            400.0,
        ),
        VehicleKind::MustangGt3 | VehicleKind::MercedesSlsAmg | VehicleKind::PaganiZondaCinque => {
            return None;
        }
        _ => return None,
    };
    Some(NeedleSpec {
        kind,
        pivot,
        axis,
        sweep,
        dial_maximum,
    })
}

fn classify_backlight(name: &str) -> Option<f32> {
    let lower = name.to_ascii_lowercase();
    if lower.contains("cockpit_s_interior_gauge_emissive_") {
        Some(SCALE_LUMINANCE)
    } else if lower.contains("cockpit_s_interior_digital_gauge_") {
        Some(DISPLAY_LUMINANCE)
    } else {
        None
    }
}

fn classify_backlight_for(kind: VehicleKind, name: &str) -> Option<f32> {
    match kind {
        VehicleKind::Veyron => classify_backlight(name),
        VehicleKind::MercedesSlsAmg
            if name.starts_with("Object_15.") || name.starts_with("Object_16.") =>
        {
            Some(SCALE_LUMINANCE)
        }
        VehicleKind::MercedesSlsAmg
            if name.starts_with("Object_11_body.") || name.starts_with("Object_12_body.") =>
        {
            Some(BUTTON_LUMINANCE)
        }
        VehicleKind::PaganiZondaCinque
            if [
                "Object_10.",
                "Object_18.",
                "Object_43.",
                "Object_43_body.",
                "Object_44.",
                "Object_58.",
            ]
            .iter()
            .any(|prefix| name.starts_with(prefix)) =>
        {
            Some(SCALE_LUMINANCE)
        }
        VehicleKind::PaganiZondaCinque if name.starts_with("Object_12_body.") => {
            Some(BUTTON_LUMINANCE)
        }
        VehicleKind::MustangGt3 | VehicleKind::MercedesSlsAmg | VehicleKind::PaganiZondaCinque => {
            None
        }
        _ if {
            let lower = name.to_ascii_lowercase();
            !lower.contains("headlight")
                && !lower.contains("taillight")
                && (lower.contains("display")
                    || lower.contains("speedmeter")
                    || lower.contains("speedometer")
                    || lower.contains("tachometer")
                    || lower.contains("dashboard")
                    || lower.contains("instrument")
                    || lower.contains("gauge")
                    || lower.contains("schermo")
                    || lower.contains("lightsnormal")
                    || lower.contains("decalamg_12interior"))
        } =>
        {
            Some(SCALE_LUMINANCE)
        }
        _ => None,
    }
}

fn panel_light_position(kind: VehicleKind) -> Option<Vec3> {
    match kind {
        VehicleKind::Veyron => Some(Vec3::new(0.387, 0.815, 0.68)),
        VehicleKind::MustangGt3 => None,
        VehicleKind::MercedesSlsAmg => Some(Vec3::new(0.414, 0.84, 0.20)),
        VehicleKind::PaganiZondaCinque => Some(Vec3::new(0.313, 0.65, 0.81)),
        _ => None,
    }
}

fn private_glowing_material(
    current: &MeshMaterial3d<StandardMaterial>,
    use_base_color_as_emissive: bool,
    materials: &mut Assets<StandardMaterial>,
) -> Option<Handle<StandardMaterial>> {
    let mut material = materials.get(&current.0)?.clone();
    material.emissive = LinearRgba::BLACK;
    material.emissive_exposure_weight = 1.0;
    if use_base_color_as_emissive && material.emissive_texture.is_none() {
        material.emissive_texture = material.base_color_texture.clone();
    }
    Some(materials.add(material))
}

/// The Zonda export bakes its dial artwork into one textured surface. Two
/// narrow disconnected pieces initially looked like pointers, but moving them
/// only displaced parts of the printed scale. Add real geometry immediately in
/// front of the two authored dials instead. The pivots still live in measured
/// model space and are driven by the same simulation-backed component as the
/// other cars.
fn spawn_zonda_runtime_needles(
    commands: &mut Commands,
    root: Entity,
    vehicle: Entity,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> usize {
    let pointer_mesh = meshes.add(Rectangle::new(ZONDA_POINTER_WIDTH, ZONDA_POINTER_LENGTH));
    let hub_mesh = meshes.add(Circle::new(ZONDA_POINTER_HUB_RADIUS));
    let pointer_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.96, 0.055, 0.018),
        emissive: LinearRgba::BLACK,
        emissive_exposure_weight: 1.0,
        perceptual_roughness: 0.32,
        metallic: 0.12,
        cull_mode: None,
        ..default()
    });

    for (name, spec, angle_at_max) in [
        (
            "ZondaLiveTachometerNeedle",
            needle_spec_for(
                VehicleKind::PaganiZondaCinque,
                "InstrumentNeedle_Tachometer_Zonda",
            )
            .expect("Zonda tachometer specification"),
            -45.0_f32.to_radians(),
        ),
        (
            "ZondaLiveSpeedometerNeedle",
            needle_spec_for(
                VehicleKind::PaganiZondaCinque,
                "InstrumentNeedle_Speedometer_Zonda",
            )
            .expect("Zonda speedometer specification"),
            -135.0_f32.to_radians(),
        ),
    ] {
        let normal = spec.axis.normalize();
        let up = (Vec3::Y - normal * normal.dot(Vec3::Y)).normalize();
        let right = up.cross(normal).normalize();
        let face = Quat::from_mat3(&Mat3::from_cols(right, up, normal));
        let baseline = Quat::from_axis_angle(normal, angle_at_max);
        let pointer_rotation = baseline * face;
        let pivot = commands
            .spawn((
                Name::new(name),
                InstrumentNeedle {
                    vehicle,
                    kind: spec.kind,
                    displayed: f32::NAN,
                    axis: normal,
                    sweep: spec.sweep,
                    dial_maximum: spec.dial_maximum,
                },
                Transform::from_translation(spec.pivot),
                Visibility::default(),
                ChildOf(root),
            ))
            .id();

        // A short counterweight behind the pivot and a slight offset towards
        // the driver keep the pointer readable without z-fighting the glass.
        let centre = baseline * (up * (ZONDA_POINTER_LENGTH * 0.34)) + normal * 0.0022;
        commands.spawn((
            Name::new(format!("{name}Blade")),
            Mesh3d(pointer_mesh.clone()),
            MeshMaterial3d(pointer_material.clone()),
            InstrumentGlowSurface {
                peak: NEEDLE_LUMINANCE,
                written: f32::NAN,
            },
            Transform {
                translation: centre,
                rotation: pointer_rotation,
                ..default()
            },
            ChildOf(pivot),
        ));
        commands.spawn((
            Name::new(format!("{name}Hub")),
            Mesh3d(hub_mesh.clone()),
            MeshMaterial3d(pointer_material.clone()),
            Transform {
                translation: normal * 0.0024,
                rotation: face,
                ..default()
            },
            ChildOf(pivot),
        ));
    }
    2
}

/// Discover and re-pivot the actual gauge meshes as soon as the glTF exists.
pub fn wire_instruments_on_ready(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    roots: Query<&VehicleModelRoot>,
    vehicles: Query<&Vehicle>,
    mut scene_and_meshes: ParamSet<(SceneNodes, ResMut<Assets<Mesh>>)>,
    painted: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let root = ready.entity;
    let Ok(model_root) = roots.get(root) else {
        return;
    };

    let mut needles = 0usize;
    let mut glowing_surfaces = 0usize;

    {
        let scene = scene_and_meshes.p0();
        for entity in scene.descendants(root) {
            let Some(name) = scene.name(entity) else {
                continue;
            };
            let Ok(current) = painted.get(entity) else {
                // Only the leaf node has a mesh and material. Matching that node
                // avoids trying to move its same-named parent as well.
                continue;
            };

            // Zonda's semantic-looking nodes are actually pieces of the printed
            // dial texture and must remain static. Its real runtime pointers are
            // spawned below after all authored backlights have been claimed.
            if model_root.kind != VehicleKind::PaganiZondaCinque
                && let Some(spec) = needle_spec_for(model_root.kind, name)
            {
                let Some(matrix) = scene.transform_relative_to(entity, root) else {
                    continue;
                };
                let pivot = spec.pivot;
                let Some(material) = private_glowing_material(current, false, &mut materials)
                else {
                    continue;
                };
                let pivot_entity = commands
                    .spawn((
                        Name::new(format!("{}NeedlePivot", spec.kind.label())),
                        InstrumentNeedle {
                            vehicle: model_root.vehicle,
                            kind: spec.kind,
                            // Forces the first animation frame to snap to the real
                            // reading rather than sweep down from maximum.
                            displayed: f32::NAN,
                            axis: spec.axis,
                            sweep: spec.sweep,
                            dial_maximum: spec.dial_maximum,
                        },
                        Transform::from_translation(pivot),
                        Visibility::default(),
                        ChildOf(root),
                    ))
                    .id();

                let rebased = Mat4::from_translation(-pivot) * matrix;
                let (scale, rotation, translation) = rebased.to_scale_rotation_translation();
                commands.entity(entity).insert((
                    MeshMaterial3d(material),
                    InstrumentGlowSurface {
                        peak: NEEDLE_LUMINANCE,
                        written: f32::NAN,
                    },
                    Transform {
                        translation,
                        rotation,
                        scale,
                    },
                    ChildOf(pivot_entity),
                ));
                needles += 1;
                glowing_surfaces += 1;
                continue;
            }

            let Some(peak) = scene.find_in_ancestry(entity, root, |candidate| {
                classify_backlight_for(model_root.kind, candidate)
            }) else {
                continue;
            };
            let Some(material) = private_glowing_material(
                current,
                model_root.kind != VehicleKind::Veyron,
                &mut materials,
            ) else {
                continue;
            };
            commands.entity(entity).insert((
                MeshMaterial3d(material),
                InstrumentGlowSurface {
                    peak,
                    written: f32::NAN,
                },
            ));
            glowing_surfaces += 1;
        }
    }

    if model_root.kind == VehicleKind::PaganiZondaCinque {
        let mut meshes = scene_and_meshes.p1();
        let runtime_needles = spawn_zonda_runtime_needles(
            &mut commands,
            root,
            model_root.vehicle,
            &mut meshes,
            &mut materials,
        );
        needles += runtime_needles;
        glowing_surfaces += runtime_needles;
    }

    if model_root.kind == VehicleKind::Veyron && needles == 0 {
        warn!("no analogue instrument needles found in the car model");
        return;
    }

    if glowing_surfaces > 0 || model_root.kind != VehicleKind::MustangGt3 {
        let position = panel_light_position(model_root.kind).or_else(|| {
            let vehicle = vehicles.get(model_root.vehicle).ok()?;
            let vehicle_point = vehicle.config.driver_eye + Vec3::new(0.0, -0.22, -0.34);
            Some(
                model_root.model_origin
                    + super::physics::model_space_for(
                        vehicle_point,
                        model_root.model_scale,
                        model_root.model_yaw,
                    ),
            )
        });
        if let Some(position) = position {
            commands.spawn((
                Name::new("InstrumentPanelLight"),
                InstrumentPanelLight {
                    peak: CABIN_GLOW_LUMENS,
                    written: f32::NAN,
                },
                PointLight {
                    color: Color::linear_rgb(
                        BACKLIGHT_TINT[0],
                        BACKLIGHT_TINT[1],
                        BACKLIGHT_TINT[2],
                    ),
                    intensity: 0.0,
                    range: CABIN_GLOW_RANGE,
                    radius: 0.12,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::from_translation(position),
                ChildOf(root),
            ));
        }
    }

    info!("cockpit instruments: {needles} animated needles, {glowing_surfaces} illuminated meshes");
}

/// Drive every analogue pointer from the simulation. The main dials use actual
/// engine rpm and signed road speed (shown as magnitude in reverse).
pub fn animate_instruments(
    time: Res<Time>,
    vehicles: Query<&Vehicle>,
    mut needles: Query<(&mut InstrumentNeedle, &mut Transform)>,
) {
    for (mut needle, mut transform) in &mut needles {
        let Ok(vehicle) = vehicles.get(needle.vehicle) else {
            continue;
        };
        let target = needle.kind.target(vehicle, needle.dial_maximum);
        needle.displayed = if needle.displayed.is_finite() {
            let blend = 1.0 - (-NEEDLE_RESPONSE * time.delta_secs()).exp();
            needle.displayed + (target - needle.displayed) * blend
        } else {
            target
        };
        transform.rotation = Quat::from_axis_angle(
            needle.axis.normalize(),
            needle.sweep * (1.0 - needle.displayed),
        );
    }
}

/// Switch the dial faces, needles, digital displays and their subtle cabin fill
/// together with the automatic headlights.
pub fn apply_instrument_lighting(
    settings: Res<LightingSettings>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut surfaces: Query<(
        &mut InstrumentGlowSurface,
        &MeshMaterial3d<StandardMaterial>,
    )>,
    mut panel_lights: Query<(&mut InstrumentPanelLight, &mut PointLight)>,
) {
    let glow = if settings.time_of_day.headlights_enabled() {
        1.0
    } else {
        0.0
    };

    for (mut surface, material) in &mut surfaces {
        if settled(surface.written, glow) {
            continue;
        }
        let Some(mut target) = materials.get_mut(&material.0) else {
            continue;
        };
        target.emissive = LinearRgba::rgb(
            BACKLIGHT_TINT[0] * surface.peak * glow,
            BACKLIGHT_TINT[1] * surface.peak * glow,
            BACKLIGHT_TINT[2] * surface.peak * glow,
        );
        surface.written = glow;
    }

    for (mut state, mut light) in &mut panel_lights {
        if settled(state.written, glow) {
            continue;
        }
        light.intensity = state.peak * glow;
        state.written = glow;
    }
}

#[inline]
fn settled(written: f32, glow: f32) -> bool {
    (glow - written).abs() < WRITE_EPSILON
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::engine::rpm_to_rad;
    use crate::vehicle::physics::Vehicle;
    use crate::vehicle::presets::{bugatti_veyron_super_sport, mercedes_benz_sls_amg};
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    fn parked_car() -> Vehicle {
        Vehicle::new(bugatti_veyron_super_sport(), Transform::IDENTITY)
    }

    #[test]
    fn only_real_instrument_meshes_are_claimed() {
        assert_eq!(
            classify_needle("bug_veyron_09_cockpit_s_tach_max_bump_plastic5_"),
            Some(InstrumentKind::Tachometer)
        );
        assert_eq!(
            classify_needle("bug_veyron_09_cockpit_s_speed_max_bump_plastic5_"),
            Some(InstrumentKind::Speedometer)
        );
        assert_eq!(
            classify_needle("bug_veyron_09_cockpit_s_interior_solid_"),
            None
        );
        assert_eq!(
            classify_backlight("bug_veyron_09_cockpit_s_interior_gauge_emissive_"),
            Some(SCALE_LUMINANCE)
        );

        assert_eq!(
            classify_backlight_for(VehicleKind::MercedesSlsAmg, "Object_15.int_display"),
            Some(SCALE_LUMINANCE)
        );
        assert_eq!(
            classify_backlight_for(VehicleKind::MercedesSlsAmg, "Object_11_body.material_11"),
            Some(BUTTON_LUMINANCE)
        );
        assert_eq!(
            classify_backlight_for(
                VehicleKind::PaganiZondaCinque,
                "Object_43.gauge_emissive.001"
            ),
            Some(SCALE_LUMINANCE)
        );
        assert_eq!(
            classify_backlight_for(VehicleKind::MercedesSlsAmg, "Object_12.BLACK.006"),
            None,
            "bodywork must never be turned into an instrument light"
        );
    }

    #[test]
    fn tachometer_and_speedometer_use_real_vehicle_values() {
        let mut vehicle = parked_car();
        assert!(InstrumentKind::Tachometer.target(&vehicle, 8_000.0) > 0.05);
        assert_eq!(InstrumentKind::Speedometer.target(&vehicle, 420.0), 0.0);

        vehicle.dynamics.forward_speed = 210.0 / 3.6;
        assert!((InstrumentKind::Speedometer.target(&vehicle, 420.0) - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn every_reading_is_clamped_to_the_physical_dial() {
        let mut vehicle = parked_car();
        vehicle.dynamics.forward_speed = -500.0 / 3.6;
        assert_eq!(InstrumentKind::Speedometer.target(&vehicle, 420.0), 1.0);
        assert!((0.0..=1.0).contains(&InstrumentKind::Tachometer.target(&vehicle, 8_000.0)));
    }

    #[test]
    fn sls_and_zonda_have_real_speed_and_rpm_pointer_specs() {
        for (vehicle, suffix, speed_maximum) in [
            (VehicleKind::MercedesSlsAmg, "SLS", 360.0),
            (VehicleKind::PaganiZondaCinque, "Zonda", 400.0),
        ] {
            let tach = needle_spec_for(
                vehicle,
                &format!("InstrumentNeedle_Tachometer_{suffix}.material"),
            )
            .expect("tachometer pointer");
            let speed = needle_spec_for(
                vehicle,
                &format!("InstrumentNeedle_Speedometer_{suffix}.material"),
            )
            .expect("speedometer pointer");
            assert_eq!(tach.kind, InstrumentKind::Tachometer);
            assert_eq!(tach.dial_maximum, 8_000.0);
            assert_eq!(speed.kind, InstrumentKind::Speedometer);
            assert_eq!(speed.dial_maximum, speed_maximum);
            assert_ne!(tach.pivot, speed.pivot);
            if vehicle == VehicleKind::PaganiZondaCinque {
                assert!(tach.sweep > 0.0, "Zonda tachometer sweep");
                assert!(speed.sweep > 0.0, "Zonda speedometer sweep");
                assert!((tach.pivot.x - 0.399_0).abs() < 1.0e-4);
                assert!((speed.pivot.x - 0.229_1).abs() < 1.0e-4);
                assert!(
                    (tach.pivot.x - speed.pivot.x).abs() > 0.16,
                    "Zonda needles must sit at the centres of separate dials"
                );
                assert!((tach.pivot.y - speed.pivot.y).abs() < 1.0e-4);
            }
        }
    }

    #[test]
    fn live_pointer_transforms_follow_speed_and_engine_rpm() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                100,
            )))
            .add_systems(Update, animate_instruments);

        let vehicle = app
            .world_mut()
            .spawn(Vehicle::new(mercedes_benz_sls_amg(), Transform::IDENTITY))
            .id();
        let tach_spec = needle_spec_for(
            VehicleKind::MercedesSlsAmg,
            "InstrumentNeedle_Tachometer_SLS",
        )
        .unwrap();
        let speed_spec = needle_spec_for(
            VehicleKind::MercedesSlsAmg,
            "InstrumentNeedle_Speedometer_SLS",
        )
        .unwrap();
        let spawn_needle = |world: &mut World, spec: NeedleSpec| {
            world
                .spawn((
                    InstrumentNeedle {
                        vehicle,
                        kind: spec.kind,
                        displayed: f32::NAN,
                        axis: spec.axis,
                        sweep: spec.sweep,
                        dial_maximum: spec.dial_maximum,
                    },
                    Transform::IDENTITY,
                ))
                .id()
        };
        let tach = spawn_needle(app.world_mut(), tach_spec);
        let speed = spawn_needle(app.world_mut(), speed_spec);
        app.update();
        let parked_tach = app.world().get::<Transform>(tach).unwrap().rotation;
        let parked_speed = app.world().get::<Transform>(speed).unwrap().rotation;

        {
            let mut car = app.world_mut().get_mut::<Vehicle>(vehicle).unwrap();
            car.dynamics.engine.angular_velocity = rpm_to_rad(6_000.0);
            car.dynamics.forward_speed = 270.0 / 3.6;
        }
        app.update();

        assert_ne!(
            app.world().get::<Transform>(tach).unwrap().rotation,
            parked_tach
        );
        assert_ne!(
            app.world().get::<Transform>(speed).unwrap().rotation,
            parked_speed
        );
    }

    #[test]
    fn first_lighting_frame_is_never_skipped() {
        assert!(!settled(f32::NAN, 0.0));
        assert!(settled(1.0, 1.0));
    }

    #[test]
    fn cockpit_fill_is_limited_to_bugatti_upper_tiers_and_forward_interior_views() {
        let vehicle = Entity::from_bits(17);
        let mut camera = GameCamera::new(vehicle);
        camera.mode = CameraMode::Cockpit;
        let mut settings = LightingSettings::default();

        settings.apply_preset(LightingQuality::Performance);
        assert_eq!(
            cockpit_fill_target(&settings, VehicleKind::Veyron, Some(&camera), vehicle),
            0.0
        );

        settings.apply_preset(LightingQuality::Balanced);
        assert_eq!(
            cockpit_fill_target(&settings, VehicleKind::Veyron, Some(&camera), vehicle),
            COCKPIT_FILL_DAY_LUMENS
        );
        assert_eq!(
            cockpit_fill_target(&settings, VehicleKind::MustangGt3, Some(&camera), vehicle),
            0.0
        );

        camera.mode = CameraMode::SteeringWheel;
        assert_eq!(
            cockpit_fill_target(&settings, VehicleKind::Veyron, Some(&camera), vehicle),
            COCKPIT_FILL_DAY_LUMENS
        );

        camera.rear_view = true;
        assert_eq!(
            cockpit_fill_target(&settings, VehicleKind::Veyron, Some(&camera), vehicle),
            0.0
        );
    }

    #[test]
    fn cockpit_fill_preserves_time_of_day_contrast() {
        let vehicle = Entity::from_bits(23);
        let mut camera = GameCamera::new(vehicle);
        camera.mode = CameraMode::Cockpit;
        let mut settings = LightingSettings::default();
        settings.apply_preset(LightingQuality::Cinematic);

        settings.time_of_day = TimeOfDay::Day;
        let day = cockpit_fill_target(&settings, VehicleKind::Veyron, Some(&camera), vehicle);
        settings.time_of_day = TimeOfDay::Night;
        let night = cockpit_fill_target(&settings, VehicleKind::Veyron, Some(&camera), vehicle);

        assert!(day > night && night > 0.0);
    }
}
