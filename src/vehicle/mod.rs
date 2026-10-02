//! Vehicle simulation.
//!
//! The modules split into two halves:
//!
//! * **Pure model** - [`config`], [`presets`], [`engine`], [`transmission`],
//!   [`drivetrain`], [`brakes`], [`tire`], [`aero`], [`wheels`], [`solver`] and
//!   [`testbed`] contain no Bevy systems and no physics-engine types. They can be
//!   reasoned about, unit tested and tuned on their own, which is what
//!   `tests/physics_validation.rs` does.
//! * **Integration** - [`physics`], [`controller`], [`model_rig`], [`lights`]
//!   and [`telemetry`] wire that model into Bevy and Avian.

pub mod aero;
pub mod brakes;
pub mod config;
pub mod controller;
pub mod damage;
pub mod drivetrain;
pub mod engine;
pub mod instruments;
pub mod lights;
pub mod model_rig;
pub mod nitro;
pub mod physics;
pub mod presets;
pub mod skid_marks;
pub mod solver;
pub mod spoiler;
pub mod telemetry;
pub mod testbed;
pub mod tire;
pub mod transmission;
pub mod wheels;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;

use crate::game::GameState;
use crate::world::road::RoadSpec;
use config::{VehicleConfig, WheelId};
use physics::{VehicleModelRoot, WheelHubPivot, WheelSpinPivot};

/// Vehicle variants retained for asset and configuration compatibility.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Reflect)]
pub enum VehicleKind {
    #[default]
    Veyron,
    MustangGt3,
    MercedesSlsAmg,
    PaganiZondaCinque,
    MercedesAmgOne,
    LamborghiniAventador,
    KoenigseggJesko,
    FerrariEnzo,
    LamborghiniCountach,
    LamborghiniRevuelto,
    FordGt2006,
    FordMustang1965,
}

impl VehicleKind {
    pub const ALL: [Self; 1] = [Self::Veyron];

    pub const fn next(self) -> Self {
        let _ = self;
        Self::Veyron
    }

    pub const fn previous(self) -> Self {
        let _ = self;
        Self::Veyron
    }

    pub const fn model_path(self) -> &'static str {
        match self {
            Self::Veyron => "models/bugatti/scene.gltf",
            Self::MustangGt3 => "models/mustang_gt3/scene.gltf",
            Self::MercedesSlsAmg => "models/mercedes_sls_amg/scene.gltf",
            Self::PaganiZondaCinque => "models/pagani_zonda_cinque/scene.gltf",
            Self::MercedesAmgOne => "models/mercedes_amg_one/scene.gltf",
            Self::LamborghiniAventador => "models/lamborghini_aventador/scene.gltf",
            Self::KoenigseggJesko => "models/koenigsegg_jesko/scene.gltf",
            Self::FerrariEnzo => "models/ferrari_enzo/scene.gltf",
            Self::LamborghiniCountach => "models/lamborghini_countach/scene.gltf",
            Self::LamborghiniRevuelto => "models/lamborghini_revuelto/scene.gltf",
            Self::FordGt2006 => "models/ford_gt_2006/scene.gltf",
            Self::FordMustang1965 => "models/ford_mustang_1965/scene.gltf",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Veyron => "BUGATTI VEYRON 16.4 SUPER SPORT",
            Self::MustangGt3 => "FORD MUSTANG GT3",
            Self::MercedesSlsAmg => "2010 MERCEDES-BENZ SLS AMG",
            Self::PaganiZondaCinque => "2009 PAGANI ZONDA CINQUE",
            Self::MercedesAmgOne => "MERCEDES-AMG ONE",
            Self::LamborghiniAventador => "LAMBORGHINI AVENTADOR",
            Self::KoenigseggJesko => "2020 KOENIGSEGG JESKO",
            Self::FerrariEnzo => "2002 FERRARI ENZO",
            Self::LamborghiniCountach => "LAMBORGHINI COUNTACH 5000 QV",
            Self::LamborghiniRevuelto => "LAMBORGHINI REVUELTO",
            Self::FordGt2006 => "2006 FORD GT",
            Self::FordMustang1965 => "1965 FORD MUSTANG",
        }
    }

    /// Coordinate of the simulation origin inside the unmodified source glTF.
    /// The Mustang export is intentionally kept intact (including its driver /
    /// pylon and every aero part), so it is aligned by this non-destructive root
    /// offset rather than by rewriting or deleting nodes from the asset.
    pub const fn model_origin(self) -> Vec3 {
        match self {
            Self::Veyron => Vec3::ZERO,
            // Measured midway between the four wheel centres. Y is the tyre
            // contact plane in the supplied Sketchfab scene.
            Self::MustangGt3 => Vec3::new(-0.150, 0.085, -1.880),
            Self::MercedesSlsAmg => Vec3::new(0.0, 0.0, 0.085),
            Self::PaganiZondaCinque => Vec3::new(0.0, -0.103, 0.005),
            Self::MercedesAmgOne => Vec3::new(0.0, -0.042, 0.005),
            Self::LamborghiniAventador => Vec3::new(0.0, 0.005, -0.034),
            Self::KoenigseggJesko => Vec3::new(0.0, -0.006, 0.086),
            Self::FerrariEnzo => Vec3::new(0.0, 0.0, 0.0024),
            Self::LamborghiniCountach => Vec3::new(-25.728, 0.075, -9.259),
            Self::LamborghiniRevuelto => Vec3::new(1.286, -0.111, 0.003),
            Self::FordGt2006 => Vec3::new(0.0, -0.014, 0.063),
            Self::FordMustang1965 => Vec3::new(0.0, 0.0095, -0.0048),
        }
    }

    pub fn config(self) -> VehicleConfig {
        match self {
            Self::Veyron => presets::bugatti_veyron_super_sport(),
            Self::MustangGt3 => presets::ford_mustang_gt3(),
            Self::MercedesSlsAmg => presets::mercedes_benz_sls_amg(),
            Self::PaganiZondaCinque => presets::pagani_zonda_cinque(),
            Self::MercedesAmgOne => presets::mercedes_amg_one(),
            Self::LamborghiniAventador => presets::lamborghini_aventador(),
            Self::KoenigseggJesko => presets::koenigsegg_jesko(),
            Self::FerrariEnzo => presets::ferrari_enzo(),
            Self::LamborghiniCountach => presets::lamborghini_countach(),
            Self::LamborghiniRevuelto => presets::lamborghini_revuelto(),
            Self::FordGt2006 => presets::ford_gt_2006(),
            Self::FordMustang1965 => presets::ford_mustang_1965(),
        }
    }

    pub const fn model_scale(self) -> f32 {
        match self {
            Self::FerrariEnzo => 100.0,
            Self::LamborghiniCountach => 0.25,
            Self::FordMustang1965 => 2.5,
            Self::KoenigseggJesko => 0.79,
            _ => 1.0,
        }
    }

    pub const fn model_yaw(self) -> f32 {
        match self {
            // This archive is authored lengthwise on X: the nose is -X and
            // the driver's left is +Z. A -90° turn maps those axes to the
            // simulator's -Z front / -X left convention.
            Self::LamborghiniRevuelto => -core::f32::consts::FRAC_PI_2,
            _ => core::f32::consts::PI,
        }
    }
}

/// Vertical clearance used when the car is placed on the road [m].
const SPAWN_LIFT: f32 = 0.05;

/// The set the vehicle force system runs in, between Avian's `First` and its
/// broad phase.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VehicleForceSystems;

pub struct VehiclePlugin;

impl Plugin for VehiclePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(damage::DamagePlugin)
            .init_resource::<telemetry::PerformanceLog>()
            .init_resource::<skid_marks::SkidTrailState>()
            .init_resource::<skid_marks::TireSmokeState>()
            .register_type::<controller::DriverInput>()
            .register_type::<controller::KeyBindings>()
            .register_type::<lights::BrakeLamps>()
            .add_systems(
                Startup,
                (
                    nitro::setup_nitro_assets,
                    skid_marks::setup_skid_mark_assets,
                    spawn_player_vehicle,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    controller::collect_keyboard_input,
                    controller::smooth_driver_input,
                )
                    .chain()
                    .run_if(in_state(GameState::Driving)),
            )
            // The lamps deliberately keep running while the game is paused, so a
            // frozen frame shows the brake lights the driver had on.
            .add_systems(
                Update,
                (lights::drive_brake_lamps, lights::apply_brake_lamps)
                    .chain()
                    .after(controller::smooth_driver_input),
            )
            .add_systems(Update, lights::apply_headlights)
            .add_systems(
                Update,
                nitro::animate_nitro_vfx.after(controller::smooth_driver_input),
            )
            .add_systems(
                Update,
                spoiler::update_active_spoiler_state.after(controller::smooth_driver_input),
            )
            .add_systems(
                Update,
                (
                    instruments::apply_instrument_lighting,
                    instruments::apply_cockpit_fill,
                ),
            )
            .add_systems(
                Update,
                (release_controls_when_paused,).run_if(in_state(GameState::Paused)),
            )
            .add_systems(Update, telemetry::record_performance)
            .add_systems(
                Update,
                (
                    skid_marks::clear_disabled_tire_fx,
                    skid_marks::fade_emergency_skid_marks,
                    skid_marks::animate_tire_smoke,
                ),
            )
            // The visual rig must be updated before Bevy propagates transforms,
            // otherwise the wheels lag one frame behind the physics.
            .add_systems(
                PostUpdate,
                (
                    physics::update_visual_rig,
                    spoiler::animate_active_spoiler,
                    instruments::animate_instruments,
                    skid_marks::spawn_emergency_skid_marks.run_if(in_state(GameState::Driving)),
                    skid_marks::spawn_tire_smoke.run_if(in_state(GameState::Driving)),
                )
                    .before(TransformSystems::Propagate),
            )
            // Forces have to be applied inside Avian's own step, so that the
            // very next solver iteration consumes them. The set is ordered
            // explicitly between two of Avian's own phases rather than dropped
            // into one of them, so the schedule has no ambiguity to complain
            // about.
            .configure_sets(
                PhysicsSchedule,
                VehicleForceSystems
                    .after(PhysicsStepSystems::First)
                    .before(PhysicsStepSystems::BroadPhase),
            )
            .add_systems(
                PhysicsSchedule,
                (physics::vehicle_physics, spoiler::apply_active_spoiler_aero)
                    .chain()
                    .in_set(VehicleForceSystems),
            )
            .add_systems(
                PhysicsSchedule,
                // Run after every Avian write-back and collider-tree update so
                // the emergency correction is the final authority on the
                // player's vertical position for this physics tick.
                (
                    physics::deflect_from_road_rail,
                    physics::prevent_road_tunneling,
                )
                    .chain()
                    .after(PhysicsStepSystems::Last),
            );
    }
}

/// A glTF scene is instantiated over several ECS commands. Keeping its root
/// hidden until Bevy reports the complete world instance prevents large models
/// (notably the Mustang) from visibly appearing panel by panel in the garage.
fn reveal_complete_model(
    ready: On<WorldInstanceReady>,
    mut roots: Query<&mut Visibility, With<VehicleModelRoot>>,
) {
    if let Ok(mut visibility) = roots.get_mut(ready.entity) {
        *visibility = Visibility::Inherited;
    }
}

/// Let the car coast to a stop rather than holding the last input while paused.
fn release_controls_when_paused(mut query: Query<&mut controller::DriverInput>) {
    for mut input in &mut query {
        *input = controller::DriverInput::default();
    }
}

fn spawn_player_vehicle(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    spec: Res<RoadSpec>,
    nitro_assets: Res<nitro::NitroVfxAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut pose = spec.spawn_transform();
    pose.translation.y += SPAWN_LIFT;

    let vehicle = spawn_vehicle_kind(
        &mut commands,
        &asset_server,
        VehicleKind::Veyron,
        pose,
        &nitro_assets,
        &mut meshes,
        &mut materials,
    );
    commands
        .entity(vehicle)
        .insert(w16_audio::CarAudio::player());
}

/// Reset the single installed Bugatti before the player chooses Offline or
/// This replaces the reset that used to happen on the removed garage
/// screen, so returning to the main menu still starts a genuinely fresh run.
pub(crate) fn prepare_new_race(
    mut commands: Commands,
    road: Res<RoadSpec>,
    mut performance: ResMut<telemetry::PerformanceLog>,
    mut vehicles: Query<(Entity, &mut physics::Vehicle), With<physics::VehicleTag>>,
) {
    let mut pose = road.spawn_transform();
    pose.translation.y += SPAWN_LIFT;
    for (entity, mut vehicle) in &mut vehicles {
        physics::respawn(&mut commands, entity, &mut vehicle, pose);
    }
    performance.restart_run();
}

/// Spawn one complete car, including physics, visual model and all runtime
/// observers.
pub(crate) fn spawn_vehicle_kind(
    commands: &mut Commands,
    asset_server: &AssetServer,
    kind: VehicleKind,
    pose: Transform,
    nitro_assets: &nitro::NitroVfxAssets,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Entity {
    let config = kind.config();

    info!(
        "spawning {} - {:.0} kg, {:.0} PS, {} speed {:?}",
        config.name,
        config.mass,
        config.engine.torque_curve.peak_power().0 / 735.5,
        config.transmission.gear_ratios.len(),
        config.drivetrain
    );

    let vehicle = physics::spawn_vehicle(commands, config.clone(), pose);
    commands
        .entity(vehicle)
        .insert((kind, lights::BrakeLamps::default()));
    if kind == VehicleKind::Veyron {
        commands
            .entity(vehicle)
            .insert(spoiler::ActiveSpoiler::default());
    }
    nitro::spawn_nitro_vfx(commands, vehicle, kind, nitro_assets);
    instruments::spawn_cockpit_fill(commands, vehicle, kind);

    // The glTF is authored with +Z forward; the simulation uses Bevy's -Z, so
    // the whole visual model hangs off a half turn about the vertical axis.
    let rotation = Quat::from_rotation_y(kind.model_yaw());
    let scale = kind.model_scale();
    let model_origin = kind.model_origin();
    let model_transform = Transform::from_translation(-(rotation * (model_origin * scale)))
        .with_rotation(rotation)
        .with_scale(Vec3::splat(scale));

    let model_path = kind.model_path();
    // Use the same root as Bevy's AssetPlugin. A Store/MSIX launch can have an
    // unrelated working directory, so `Path::new("assets")` is not reliable.
    // On Android the model is looked up inside the APK instead.
    let model_file = crate::asset_paths::asset_directory().join(model_path);
    let model_available = crate::asset_paths::asset_exists(model_path);

    if model_available {
        commands
            .spawn((
                Name::new(format!("VehicleModel: {}", kind.display_name())),
                VehicleModelRoot {
                    vehicle,
                    kind,
                    model_origin,
                    model_scale: scale,
                    model_yaw: kind.model_yaw(),
                },
                model_transform,
                // `WorldInstanceReady` reveals the entire hierarchy in one
                // frame after every child mesh and runtime rig is present.
                Visibility::Hidden,
                WorldAssetRoot(asset_server.load(GltfAssetLabel::Scene(0).from_asset(model_path))),
                ChildOf(vehicle),
            ))
            .observe(model_rig::rig_model_on_ready)
            .observe(lights::wire_brake_lamps_on_ready)
            .observe(lights::wire_headlights_on_ready)
            .observe(instruments::wire_instruments_on_ready)
            .observe(reveal_complete_model);
    } else {
        warn!(
            "car model not found at {}; using the primitive placeholder. \
             Reinstall the complete game package to use the real car.",
            model_file.display()
        );
        let model_root = commands
            .spawn((
                Name::new("VehicleModelPlaceholder"),
                VehicleModelRoot {
                    vehicle,
                    kind,
                    model_origin: Vec3::ZERO,
                    model_scale: 1.0,
                    model_yaw: core::f32::consts::PI,
                },
                Transform::from_rotation(rotation),
                Visibility::default(),
                ChildOf(vehicle),
            ))
            .id();
        spawn_placeholder_visuals(commands, model_root, vehicle, &config, meshes, materials);
    }

    vehicle
}

/// A blocky stand-in used only when the real model is missing, so that the
/// physics can still be developed and tested.
fn spawn_placeholder_visuals(
    commands: &mut Commands,
    model_root: Entity,
    vehicle: Entity,
    config: &VehicleConfig,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let body_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.10, 0.16, 0.34),
        perceptual_roughness: 0.35,
        metallic: 0.5,
        ..default()
    });
    let tire_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.05, 0.05, 0.06),
        perceptual_roughness: 0.95,
        ..default()
    });

    for (center, half_extents) in &config.collider_boxes {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(
                half_extents.x * 2.0,
                half_extents.y * 2.0,
                half_extents.z * 2.0,
            ))),
            MeshMaterial3d(body_material.clone()),
            // Placeholder geometry lives in model space like everything else.
            Transform::from_translation(physics::model_space(*center)),
            ChildOf(model_root),
        ));
    }

    for id in WheelId::ALL {
        let wheel_cfg = config.wheel(id);
        let hub =
            physics::model_space(wheel_cfg.anchor - Vec3::Y * wheel_cfg.suspension.rest_length);
        let hub_pivot = commands
            .spawn((
                Name::new(format!("WheelHub{}", id.short_name())),
                WheelHubPivot {
                    vehicle,
                    id,
                    model_origin: Vec3::ZERO,
                    model_scale: 1.0,
                    model_yaw: core::f32::consts::PI,
                },
                Transform::from_translation(hub),
                Visibility::default(),
                ChildOf(model_root),
            ))
            .id();
        // The spin pivot's own rotation is overwritten every frame, so the
        // cylinder - which Bevy builds along +Y - is turned onto the axle by a
        // child transform instead.
        let spin_pivot = commands
            .spawn((
                Name::new(format!("WheelSpin{}", id.short_name())),
                WheelSpinPivot { vehicle, id },
                Transform::IDENTITY,
                Visibility::default(),
                ChildOf(hub_pivot),
            ))
            .id();
        commands.spawn((
            Mesh3d(meshes.add(Cylinder::new(wheel_cfg.tire.radius, wheel_cfg.tire.width))),
            MeshMaterial3d(tire_material.clone()),
            Transform::from_rotation(Quat::from_rotation_z(core::f32::consts::FRAC_PI_2)),
            ChildOf(spin_pivot),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bugatti_is_the_only_available_vehicle() {
        assert_eq!(VehicleKind::ALL, [VehicleKind::Veyron]);
        assert_eq!(VehicleKind::Veyron.next(), VehicleKind::Veyron);
        assert_eq!(VehicleKind::Veyron.previous(), VehicleKind::Veyron);
    }
}
