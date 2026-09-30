//! Head-less smoke test for the simulation.
//!
//! Building the real `SimulationPlugin` and stepping it catches everything the
//! compiler cannot: conflicting queries, missing resources, systems ordered
//! against sets that do not exist, and panics during world generation or vehicle
//! spawning. It runs without a window and without a GPU, which is what makes it
//! usable in CI. The user interface is deliberately left out, which is why
//! `GamePlugin` is split into [`SimulationPlugin`] plus the UI.
//!
//! There is neither a window nor a GPU: the renderer is created with no
//! backends and winit is disabled, so the test runs anywhere.

use std::time::Duration;

use avian3d::prelude::{
    Collider, CollisionEventsEnabled, LinearVelocity, PhysicsTime, Position, RigidBody, Rotation,
};
use bevy::light::Skybox;
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::settings::WgpuSettings;
use bevy::time::TimeUpdateStrategy;
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;

use veyron_sim::audio::music::MenuMusicTrack;
use veyron_sim::camera::controller::GameCamera;
use veyron_sim::game::{GamePlugin, GameState, SimulationPlugin};
use veyron_sim::vehicle::VehicleKind;
use veyron_sim::vehicle::damage::{BreakablePart, DamagePartKind};
use veyron_sim::vehicle::instruments::{
    InstrumentGlowSurface, InstrumentNeedle, InstrumentPanelLight,
};
use veyron_sim::vehicle::lights::HeadlightSpill;
use veyron_sim::vehicle::physics::Vehicle;
use veyron_sim::vehicle::spoiler::{ActiveSpoiler, SpoilerVisualPivot};
use veyron_sim::world::lighting::{LightingSettings, TimeOfDay};
use veyron_sim::world::road::{RoadMaterials, RoadSpec};
use veyron_sim::world::street_lights::{RoadLamp, StreetLightModelRoot};
use veyron_sim::world::traffic::{TrafficSettings, TrafficVehicle};
use w16_audio::{CarAudio, W16Library};

fn headless_app() -> App {
    headless_app_with(SimulationPlugin, VehicleKind::Veyron)
}

fn headless_app_with<P: Plugin>(plugin: P, kind: VehicleKind) -> App {
    let mut app = App::new();
    let _ = kind;
    app.add_plugins(
        DefaultPlugins
            .set(RenderPlugin {
                render_creation: WgpuSettings {
                    backends: None,
                    ..default()
                }
                .into(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                close_when_requested: false,
                ..default()
            })
            .build()
            .disable::<WinitPlugin>(),
    )
    .add_plugins(plugin);
    // `App::update` does not run the plugins' `finish`/`cleanup` hooks - only
    // `App::run` does - and several plugins initialise resources there. Driving
    // the app by hand therefore means doing it explicitly.
    app.finish();
    app.cleanup();
    app
}

#[test]
fn road_lamps_switch_on_at_night() {
    let mut app = headless_app();
    app.update();

    {
        let world = app.world_mut();
        let mut lamps = world.query_filtered::<&SpotLight, With<RoadLamp>>();
        let mut lamp_models = world.query_filtered::<Entity, With<StreetLightModelRoot>>();
        assert_eq!(lamps.iter(world).count(), 66);
        assert_eq!(lamp_models.iter(world).count(), 66);
        assert!(lamps.iter(world).all(|light| light.intensity == 0.0));
    }

    app.world_mut()
        .resource_mut::<LightingSettings>()
        .time_of_day = TimeOfDay::Night;
    app.update();

    {
        let world = app.world_mut();
        let mut lamps = world.query_filtered::<&SpotLight, With<RoadLamp>>();
        assert!(lamps.iter(world).all(|light| light.intensity > 0.0));
    }
}

#[test]
fn full_game_opens_the_main_menu_before_mode_selection() {
    let mut app = headless_app_with(GamePlugin, VehicleKind::Veyron);
    let mut soundtrack_ready = false;
    for _ in 0..500 {
        app.update();
        let world = app.world_mut();
        soundtrack_ready = world.resource::<W16Library>().len() == w16_audio::SOUNDS.len()
            && world.resource::<Assets<AudioSource>>().len() >= 3;
        if soundtrack_ready {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        *app.world().resource::<State<GameState>>().get(),
        GameState::MainMenu
    );
    assert!(
        app.world()
            .resource::<Time<avian3d::prelude::Physics>>()
            .is_paused()
    );
    let world = app.world_mut();
    let mut labels = world.query::<&Text>();
    assert!(labels.iter(world).any(|text| text.as_str() == "PLAY"));
    assert!(
        labels
            .iter(world)
            .all(|text| text.as_str() != "SELECT VEHICLE"),
        "the removed vehicle-selection screen must not be present"
    );
    assert!(
        soundtrack_ready,
        "the menu track must decode alongside the engine banks"
    );
    let mut menu_music = app
        .world_mut()
        .query_filtered::<Entity, With<MenuMusicTrack>>();
    assert_eq!(
        menu_music.iter(app.world()).count(),
        1,
        "exactly one menu soundtrack track must be active"
    );
}

/// Asset completion is asynchronous; iteration counts become flaky when the
/// whole test suite loads several cars and 2K skies at once.
fn wait_for_damage_rig(app: &mut App) {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        app.update();
        let world = app.world_mut();
        let mut parts = world.query::<&BreakablePart>();
        if parts.iter(world).count() == 21 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Bugatti damage rig did not finish loading"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn bugatti_scene_rigs_twenty_breakable_assemblies_and_the_main_shell() {
    let mut app = headless_app();
    wait_for_damage_rig(&mut app);
    let world = app.world_mut();
    let mut parts = world.query::<&BreakablePart>();
    let kinds: Vec<_> = parts.iter(world).map(|part| part.kind).collect();
    assert_eq!(kinds.len(), 21, "rigged damage assemblies: {kinds:?}");
    assert!(kinds.contains(&DamagePartKind::BodyShell));
    assert!(kinds.contains(&DamagePartKind::FrontBumper));
    assert!(kinds.contains(&DamagePartKind::DoorLeft));
    assert!(kinds.contains(&DamagePartKind::DoorRight));
    assert!(kinds.contains(&DamagePartKind::MirrorLeft));
    assert!(kinds.contains(&DamagePartKind::MirrorRight));
    assert!(kinds.contains(&DamagePartKind::Wing));
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| matches!(kind, DamagePartKind::Wheel(_)))
            .count(),
        4
    );
}

#[test]
fn hard_side_impact_breaks_the_struck_door_mirror_and_glass() {
    let mut app = headless_app();
    wait_for_damage_rig(&mut app);
    for _ in 0..5 {
        app.update();
    }
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 30.0,
    )));
    // Four fixed physics steps per rendered frame must not lose the impact.
    let original_meshes: Vec<_> = {
        let world = app.world_mut();
        let mut meshes = world.query::<(Entity, &Mesh3d)>();
        meshes
            .iter(world)
            .map(|(e, mesh)| (e, mesh.0.clone()))
            .collect()
    };
    let pose = Vec3::new(-9.8, 0.05, 250.0);
    {
        let world = app.world_mut();
        let mut cars = world.query_filtered::<(
            &mut Transform,
            &mut Position,
            &mut LinearVelocity,
            &mut Vehicle,
        ), With<Vehicle>>();
        let (mut transform, mut position, mut velocity, mut car) =
            cars.single_mut(world).expect("player car");
        transform.translation = pose;
        position.0 = pose;
        velocity.0 = Vec3::new(-38.0, 0.0, 0.0);
        car.input = Default::default();
    }
    for _ in 0..90 {
        app.update();
    }
    let world = app.world_mut();
    let mut parts = world.query::<&BreakablePart>();
    let detached: Vec<_> = parts
        .iter(world)
        .filter(|part| part.detached)
        .map(|part| part.kind)
        .collect();
    let mut cars = world.query::<&Vehicle>();
    let car = cars.single(world).expect("player car");
    assert!(
        detached.contains(&DamagePartKind::DoorRight),
        "side impact did not tear away the door: {detached:?}"
    );
    assert!(detached.contains(&DamagePartKind::MirrorRight));
    assert!(detached.contains(&DamagePartKind::SideGlassRight));
    assert!(car.dynamics.detached_wheels.iter().all(|missing| !missing));
    let assets = world.resource::<Assets<Mesh>>();
    let dented = original_meshes.iter().any(|(entity, original)| {
        let Some(current) = world.get::<Mesh3d>(*entity) else {
            return false;
        };
        if current.0 == *original {
            return false;
        }
        use bevy::mesh::VertexAttributeValues;
        let Some(VertexAttributeValues::Float32x3(before)) = assets
            .get(original)
            .and_then(|mesh| mesh.attribute(Mesh::ATTRIBUTE_POSITION))
        else {
            return false;
        };
        let Some(VertexAttributeValues::Float32x3(after)) = assets
            .get(&current.0)
            .and_then(|mesh| mesh.attribute(Mesh::ATTRIBUTE_POSITION))
        else {
            return false;
        };
        before != after
    });
    assert!(
        dented,
        "a crash must deform vertices, not only move whole panels"
    );
}

#[test]
fn five_hundred_kmh_crashes_crumple_the_main_shell() {
    {
        let mut app = headless_app();
        wait_for_damage_rig(&mut app);
        for _ in 0..5 {
            app.update();
        }
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 60.0,
        )));
        let wall = app
            .world_mut()
            .spawn((
                Name::new("Frontal crash test wall"),
                RigidBody::Static,
                Collider::cuboid(20.0, 3.0, 1.0),
                Transform::from_xyz(-5.4, 1.5, 254.0),
            ))
            .id();
        let originals: Vec<_> = {
            let world = app.world_mut();
            let mut parts = world.query::<(Entity, &BreakablePart)>();
            let shell = parts
                .iter(world)
                .find(|(_, part)| part.kind == DamagePartKind::BodyShell)
                .expect("main shell")
                .0;
            let mut pending = vec![shell];
            let mut originals = Vec::new();
            while let Some(entity) = pending.pop() {
                if let Some(children) = world.get::<Children>(entity) {
                    pending.extend(children.iter());
                }
                if let Some(mesh) = world.get::<Mesh3d>(entity) {
                    originals.push((entity, mesh.0.clone()));
                }
            }
            originals
        };
        assert!(!originals.is_empty());
        {
            let world = app.world_mut();
            let mut cars = world.query::<(&mut Transform, &mut Position, &mut LinearVelocity)>();
            let mut player = world.query_filtered::<Entity, With<Vehicle>>();
            let player = player.single(world).unwrap();
            let (mut pose, mut position, mut velocity) = cars.get_mut(world, player).unwrap();
            pose.translation = Vec3::new(-5.4, 0.05, 250.0);
            position.0 = pose.translation;
            velocity.0 = Vec3::new(0.0, 0.0, 520.0 / 3.6);
        }
        let mut cursor =
            bevy::ecs::message::MessageCursor::<avian3d::prelude::CollisionStart>::default();
        let mut hit_wall = false;
        for _ in 0..20 {
            app.update();
            for collision in cursor.read(
                app.world()
                    .resource::<Messages<avian3d::prelude::CollisionStart>>(),
            ) {
                hit_wall |= collision.collider1 == wall || collision.collider2 == wall;
            }
        }
        assert!(hit_wall, "the test car must actually collide with the wall");
        let world = app.world_mut();
        let assets = world.resource::<Assets<Mesh>>();
        let mut maximum_displacement = 0.0_f32;
        for (entity, original) in &originals {
            let current = world.get::<Mesh3d>(*entity).unwrap();
            let pose = world.get::<GlobalTransform>(*entity).unwrap();
            use bevy::mesh::VertexAttributeValues;
            let Some(VertexAttributeValues::Float32x3(before)) = assets
                .get(original)
                .unwrap()
                .attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                panic!("positions");
            };
            let Some(VertexAttributeValues::Float32x3(after)) = assets
                .get(&current.0)
                .unwrap()
                .attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                panic!("positions");
            };
            for (before, after) in before.iter().zip(after) {
                maximum_displacement = maximum_displacement.max(
                    pose.affine()
                        .transform_vector3(Vec3::from_array(*after) - Vec3::from_array(*before))
                        .length(),
                );
            }
        }
        let mut parts = world.query::<&BreakablePart>();
        {
            assert!(
                maximum_displacement > 1.0,
                "520 km/h must deeply crumple the main shell: {maximum_displacement}"
            );
            assert!(
                parts
                    .iter(world)
                    .any(|part| part.kind == DamagePartKind::BodyShell
                        && !part.detached
                        && part.damage > 0.0)
            );
        }
    }
}

#[test]
fn direct_wheel_impact_removes_that_wheels_suspension_force() {
    let mut app = headless_app();
    wait_for_damage_rig(&mut app);
    for _ in 0..5 {
        app.update();
    }
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 60.0,
    )));
    app.world_mut().spawn((
        Name::new("Wheel impact test post"),
        RigidBody::Static,
        Collider::cuboid(0.25, 0.9, 0.5),
        Transform::from_xyz(-10.5, 0.45, 251.3),
    ));
    let pose = Vec3::new(-8.5, 0.05, 250.0);
    {
        let world = app.world_mut();
        let mut cars = world.query_filtered::<(
            &mut Transform,
            &mut Position,
            &mut LinearVelocity,
            &mut Vehicle,
        ), With<Vehicle>>();
        let (mut transform, mut position, mut velocity, mut car) =
            cars.single_mut(world).expect("player car");
        transform.translation = pose;
        position.0 = pose;
        velocity.0 = Vec3::new(-36.0, 0.0, 0.0);
        car.input = Default::default();
    }
    for _ in 0..90 {
        app.update();
    }
    let world = app.world_mut();
    let mut parts = world.query::<&BreakablePart>();
    let detached: Vec<_> = parts
        .iter(world)
        .filter(|part| part.detached)
        .map(|part| part.kind)
        .collect();
    let mut cars = world.query::<&Vehicle>();
    let car = cars.single(world).expect("player car");
    assert!(
        detached.contains(&DamagePartKind::Wheel(
            veyron_sim::vehicle::config::WheelId::FrontRight
        )),
        "wheel stayed attached after a direct impact: {detached:?}"
    );
    assert!(car.dynamics.detached_wheels[1]);
    assert_eq!(car.dynamics.wheels[1].normal_force, 0.0);

    // Starting a new race repairs the physical and visual car completely.
    drop(cars);
    {
        let mut cars = world.query::<&mut Vehicle>();
        let mut car = cars.single_mut(world).expect("player car");
        let config = car.config.clone();
        car.dynamics.reset(&config);
        car.reset_generation = car.reset_generation.wrapping_add(1);
    }
    app.update();
    app.update();
    let world = app.world_mut();
    let mut parts = world.query::<&BreakablePart>();
    assert!(
        parts
            .iter(world)
            .all(|part| !part.detached && part.damage == 0.0)
    );
    let mut repaired = world.query::<(
        &BreakablePart,
        &ChildOf,
        Option<&RigidBody>,
        Option<&Position>,
    )>();
    for (part, parent, body, physics_position) in repaired.iter(world) {
        assert_eq!(parent.parent(), part.model_root);
        assert!(body.is_none());
        assert!(
            physics_position.is_none(),
            "debris physics pose survived repair"
        );
    }
    let mut cars = world.query::<&Vehicle>();
    let car = cars.single(world).expect("player car");
    assert!(
        car.dynamics
            .detached_wheels
            .iter()
            .all(|detached| !detached)
    );
}

#[test]
#[cfg(any())]
fn mustang_gt3_model_and_configuration_load() {
    let mut app = headless_app_with(SimulationPlugin, VehicleKind::MustangGt3);
    let mut model_ready = false;
    for _ in 0..1_000 {
        app.update();
        let world = app.world_mut();
        let mut headlights = world.query_filtered::<Entity, With<HeadlightSpill>>();
        let beams_ready = headlights.iter(world).count() == 2;
        let mut wheel_pivots =
            world.query_filtered::<Entity, With<veyron_sim::vehicle::physics::WheelSpinPivot>>();
        let wheels_ready = wheel_pivots.iter(world).count() == 4;
        let mut model_roots = world.query_filtered::<&Visibility, With<VehicleModelRoot>>();
        let revealed = model_roots
            .iter(world)
            .next()
            .is_some_and(|visibility| *visibility == Visibility::Inherited);
        model_ready = beams_ready && wheels_ready && revealed;
        if model_ready {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        model_ready,
        "Mustang scene did not finish loading, rigging and atomic reveal"
    );

    let world = app.world_mut();
    let mut vehicles = world.query::<&Vehicle>();
    let vehicle = vehicles.single(world).expect("one Mustang vehicle");
    assert_eq!(vehicle.config.name, "Ford Mustang GT3");
    assert_eq!(vehicle.config.transmission.gear_ratios.len(), 6);
    assert_eq!(
        vehicle.config.drivetrain,
        veyron_sim::vehicle::config::Drivetrain::Rwd
    );

    let mut headlights = world.query_filtered::<Entity, With<HeadlightSpill>>();
    assert_eq!(headlights.iter(world).count(), 2, "Mustang headlight beams");
    let mut wheel_pivots =
        world.query_filtered::<Entity, With<veyron_sim::vehicle::physics::WheelSpinPivot>>();
    assert_eq!(wheel_pivots.iter(world).count(), 4, "Mustang wheel pivots");
    let mut model_roots = world.query_filtered::<&Visibility, With<VehicleModelRoot>>();
    assert_eq!(
        *model_roots.single(world).expect("Mustang model root"),
        Visibility::Inherited,
        "the Mustang must be revealed only after its complete scene is ready"
    );
}

#[test]
#[cfg(any())]
fn remaining_added_models_are_fully_rigged() {
    for (kind, name, gears, headlights, instrument_surfaces) in [
        (
            VehicleKind::MercedesSlsAmg,
            "Mercedes-Benz SLS AMG",
            7,
            2,
            5,
        ),
        (
            VehicleKind::PaganiZondaCinque,
            "Pagani Zonda Cinque",
            6,
            2,
            8,
        ),
    ] {
        let mut app = headless_app_with(SimulationPlugin, kind);
        let mut ready = false;
        // The complete suite loads several multi-gigabyte vehicle scenes in
        // parallel. Give the asynchronous asset workers enough time on slower
        // disks without weakening any of the rig assertions below.
        for _ in 0..1800 {
            app.update();
            let world = app.world_mut();
            let mut pivots = world
                .query_filtered::<Entity, With<veyron_sim::vehicle::physics::WheelSpinPivot>>();
            ready = pivots.iter(world).count() == 4;
            if ready {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(ready, "{name} glTF never completed its wheel rig");

        let initial_steering_rotation = {
            let world = app.world_mut();
            let mut vehicles = world.query::<&Vehicle>();
            let vehicle = vehicles.single(world).expect("one selected vehicle");
            assert_eq!(vehicle.config.name, name);
            assert_eq!(vehicle.config.transmission.gear_ratios.len(), gears);

            let mut beams = world.query_filtered::<Entity, With<HeadlightSpill>>();
            assert_eq!(beams.iter(world).count(), headlights, "{name} beams");
            let mut glow = world.query_filtered::<Entity, With<InstrumentGlowSurface>>();
            assert_eq!(
                glow.iter(world).count(),
                instrument_surfaces,
                "{name} instrument lights"
            );
            let mut needles = world.query_filtered::<Entity, With<InstrumentNeedle>>();
            assert_eq!(
                needles.iter(world).count(),
                2,
                "{name} live speedometer/tachometer pointers"
            );
            let mut steering =
                world.query_filtered::<(&Children, &Transform), With<SteeringWheelPivot>>();
            let (steering_children, steering_transform) =
                steering.single(world).expect("one steering-wheel pivot");
            assert!(
                !steering_children.is_empty(),
                "{name} steering pivot has no real model geometry"
            );
            steering_transform.rotation
        };

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyA);
        for _ in 0..8 {
            std::thread::sleep(Duration::from_millis(2));
            app.update();
        }
        let world = app.world_mut();
        let mut steering =
            world.query_filtered::<(&SteeringWheelPivot, &Transform), With<Children>>();
        let (steering_pivot, steering_transform) = steering
            .single(world)
            .expect("animated steering-wheel pivot");
        assert!(
            steering_pivot.visual_steering < 0.0,
            "{name} steering value did not follow the A key"
        );
        assert_ne!(
            steering_transform.rotation, initial_steering_rotation,
            "{name} steering-wheel geometry did not rotate"
        );

        app.world_mut()
            .resource_mut::<LightingSettings>()
            .time_of_day = TimeOfDay::Night;
        app.update();
        let world = app.world_mut();
        let mut panel_lights = world.query_filtered::<&PointLight, With<InstrumentPanelLight>>();
        assert!(
            panel_lights.iter(world).all(|light| light.intensity > 0.0),
            "{name} instrument and button lighting did not switch on at night"
        );
        let glow_materials: Vec<Handle<StandardMaterial>> = {
            let mut glow = world
                .query_filtered::<&MeshMaterial3d<StandardMaterial>, With<InstrumentGlowSurface>>();
            glow.iter(world)
                .map(|material| material.0.clone())
                .collect()
        };
        let materials = world.resource::<Assets<StandardMaterial>>();
        assert!(
            glow_materials.iter().all(|handle| materials
                .get(handle)
                .is_some_and(|material| material.emissive != LinearRgba::BLACK)),
            "{name} gauges or button legends remained dark at night"
        );
    }
}

#[test]
#[cfg(any())]
fn eight_car_garage_expansion_loads_and_finishes_each_runtime_rig() {
    for (kind, expected_name, expected_gears) in [
        (VehicleKind::MercedesAmgOne, "Mercedes-AMG ONE", 7),
        (
            VehicleKind::LamborghiniAventador,
            "Lamborghini Aventador LP700-4",
            7,
        ),
        (VehicleKind::KoenigseggJesko, "Koenigsegg Jesko", 9),
        (VehicleKind::FerrariEnzo, "Ferrari Enzo", 6),
        (
            VehicleKind::LamborghiniCountach,
            "Lamborghini Countach 5000 QV",
            5,
        ),
        (VehicleKind::LamborghiniRevuelto, "Lamborghini Revuelto", 8),
        (VehicleKind::FordGt2006, "2006 Ford GT", 6),
        (VehicleKind::FordMustang1965, "1965 Ford Mustang GT", 4),
    ] {
        let mut app = headless_app_with(SimulationPlugin, kind);
        let mut ready = false;
        for _ in 0..1_000 {
            app.update();
            let world = app.world_mut();
            let wheels = world
                .query_filtered::<Entity, With<veyron_sim::vehicle::physics::WheelSpinPivot>>()
                .iter(world)
                .count();
            let beams = world
                .query_filtered::<Entity, With<HeadlightSpill>>()
                .iter(world)
                .count();
            let panel_lights = world
                .query_filtered::<Entity, With<InstrumentPanelLight>>()
                .iter(world)
                .count();
            let revealed = world
                .query_filtered::<&Visibility, With<VehicleModelRoot>>()
                .iter(world)
                .next()
                .is_some_and(|visibility| *visibility == Visibility::Inherited);
            ready = wheels == 4 && beams == 2 && panel_lights == 1 && revealed;
            if ready {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            ready,
            "{} did not finish its complete runtime rig",
            kind.display_name()
        );

        let world = app.world_mut();
        let vehicle = world
            .query::<&Vehicle>()
            .single(world)
            .expect("one selected vehicle");
        assert_eq!(vehicle.config.name, expected_name);
        assert_eq!(
            vehicle.config.transmission.gear_ratios.len(),
            expected_gears
        );
        assert!(vehicle.config.engine.torque_curve.peak_power().0 > 100_000.0);
        assert!(vehicle.config.mass > 1_000.0);
    }
}

#[test]
fn the_app_starts_and_runs() {
    let mut app = headless_app();

    // First update runs Startup; the rest exercise Update, FixedUpdate and the
    // physics schedule. Asset decoding is asynchronous, so wait for the
    // player sound bank and the car observers rather than
    // relying on a machine-dependent frame count.
    let mut assets_ready = false;
    let mut headlight_count = 0;
    let mut decoded_audio_count = 0;
    for step in 0..1200 {
        app.update();
        let world = app.world_mut();
        let mut headlights = world.query_filtered::<Entity, With<HeadlightSpill>>();
        headlight_count = headlights.iter(world).count();
        decoded_audio_count = world.resource::<Assets<AudioSource>>().len();
        let car_ready = headlight_count == 2;
        let audio_ready = decoded_audio_count >= 3
            && world.resource::<W16Library>().len() == w16_audio::SOUNDS.len();
        assets_ready = car_ready && audio_ready;
        // The suspension also needs its original 90-frame settling window;
        // asset readiness alone can happen on the first few frames.
        if step >= 89 && assets_ready {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        assets_ready,
        "car model or W16 audio pack did not load: {headlight_count} headlights, \
         {decoded_audio_count} decoded audio sources"
    );

    let world = app.world_mut();

    let mut impact_listeners =
        world.query_filtered::<Entity, (With<Vehicle>, With<CollisionEventsEnabled>)>();
    assert_eq!(
        impact_listeners.iter(world).count(),
        1,
        "player vehicle must publish collision-start messages"
    );

    // The track exists.
    let spec = world.resource::<RoadSpec>().clone();
    assert!((spec.length - 33_000.0).abs() < 1.0);
    let asphalt_handle = world.resource::<RoadMaterials>().asphalt.clone();
    let asphalt = world
        .resource::<Assets<StandardMaterial>>()
        .get(&asphalt_handle)
        .expect("asphalt material");
    assert!(asphalt.base_color_texture.is_some(), "asphalt colour map");
    assert!(asphalt.normal_map_texture.is_some(), "asphalt normal map");
    assert!(
        asphalt.metallic_roughness_texture.is_some() && asphalt.occlusion_texture.is_some(),
        "asphalt packed AO/roughness/metal map"
    );

    // Exactly one car, sitting on the road in its own middle lane.
    let mut vehicles = world.query::<(&Transform, &Vehicle)>();
    let found: Vec<_> = vehicles.iter(world).collect();
    assert_eq!(found.len(), 1, "expected exactly one vehicle");
    let (transform, vehicle) = found[0];

    assert!(
        transform.translation.is_finite(),
        "vehicle position is not finite: {:?}",
        transform.translation
    );
    assert!(
        transform.translation.y > -1.0 && transform.translation.y < 2.0,
        "vehicle fell through the road or launched into orbit: y = {}",
        transform.translation.y
    );
    let (_, lateral) = spec.local_coordinates(transform.translation);
    assert!(
        lateral < 0.0 && lateral.abs() < spec.paved_half_width(),
        "vehicle is not on our right-hand carriageway: lateral = {lateral}"
    );
    assert!(
        vehicle.dynamics.rpm() > 300.0,
        "engine is not running: {} rpm",
        vehicle.dynamics.rpm()
    );

    assert!(
        vehicle.speed_kmh().abs() < 5.0,
        "vehicle moved without input: {} km/h",
        vehicle.speed_kmh()
    );

    // Prove the physics actually ran: all four wheels found the road and the
    // suspension is carrying the car's weight.
    let weight = vehicle.config.mass * 9.81;
    let total_load: f32 = vehicle.dynamics.wheels.iter().map(|w| w.normal_force).sum();
    for wheel in &vehicle.dynamics.wheels {
        assert!(
            wheel.grounded,
            "{:?} never found the road surface",
            wheel.id
        );
    }
    assert!(
        (total_load - weight).abs() < weight * 0.25,
        "wheel loads sum to {total_load:.0} N but the car weighs {weight:.0} N"
    );

    // Only the player carries the dynamic, multi-RPM W16 mixer. NPC traffic
    // must not allocate a full collection of audio decoders per vehicle.
    let mut sounding_cars = world.query_filtered::<Entity, With<CarAudio>>();
    assert_eq!(sounding_cars.iter(world).count(), 1);
    assert_eq!(
        world.resource::<W16Library>().len(),
        w16_audio::SOUNDS.len()
    );

    // The asynchronous glTF observers have wired the actual car meshes into
    // functional lights and instruments rather than silently leaving static
    // decoration behind.
    let mut headlights = world.query_filtered::<Entity, With<HeadlightSpill>>();
    assert_eq!(headlights.iter(world).count(), 2, "headlight beams");
    let mut needles = world.query_filtered::<Entity, With<InstrumentNeedle>>();
    assert_eq!(needles.iter(world).count(), 5, "analogue gauge needles");
    let mut instrument_glow = world.query_filtered::<Entity, With<InstrumentGlowSurface>>();
    assert_eq!(
        instrument_glow.iter(world).count(),
        7,
        "illuminated instrument meshes"
    );
    let mut active_spoilers = world.query_filtered::<Entity, With<ActiveSpoiler>>();
    assert_eq!(
        active_spoilers.iter(world).count(),
        1,
        "Bugatti active-aero controller"
    );
    let mut spoiler_pivots = world.query_filtered::<Entity, With<SpoilerVisualPivot>>();
    assert_eq!(
        spoiler_pivots.iter(world).count(),
        2,
        "original wing and hydraulic-linkage pivots"
    );

    let mut daylight_beams = world.query_filtered::<&SpotLight, With<HeadlightSpill>>();
    assert!(
        daylight_beams
            .iter(world)
            .all(|light| light.intensity == 0.0),
        "automatic headlights must remain off in daylight"
    );
    let daylight_sky = {
        let mut skyboxes = world.query_filtered::<&Skybox, With<GameCamera>>();
        let skybox = skyboxes.single(world).expect("game camera skybox");
        assert_eq!(skybox.brightness, TimeOfDay::Day.skybox_brightness());
        skybox.image.clone().expect("loaded daylight cubemap")
    };

    app.world_mut()
        .resource_mut::<LightingSettings>()
        .time_of_day = TimeOfDay::Night;
    app.update();
    let world = app.world_mut();
    let mut night_beams = world.query_filtered::<&SpotLight, With<HeadlightSpill>>();
    assert!(
        night_beams
            .iter(world)
            .all(|light| light.intensity >= 450_000.0 && light.range >= 140.0),
        "night headlight beams must illuminate the road brightly at long range"
    );
    let mut skyboxes = world.query_filtered::<&Skybox, With<GameCamera>>();
    let night_skybox = skyboxes.single(world).expect("game camera skybox");
    assert_eq!(
        night_skybox.brightness,
        TimeOfDay::Night.skybox_brightness()
    );
    assert_ne!(
        night_skybox.image.as_ref(),
        Some(&daylight_sky),
        "night must use a separate skybox"
    );
    let mut panel_lights = world.query_filtered::<&PointLight, With<InstrumentPanelLight>>();
    assert!(
        panel_lights.iter(world).all(|light| light.intensity > 0.0),
        "instrument panel illumination must switch on with the headlights"
    );
}

#[test]
fn full_throttle_actually_accelerates_the_car() {
    let mut app = headless_app();

    // Let the suspension settle first.
    for _ in 0..40 {
        app.update();
        std::thread::sleep(Duration::from_millis(4));
    }

    let road = app.world().resource::<RoadSpec>().clone();
    let start_distance = road.distance_along(vehicle_state(&mut app).0.translation);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 60.0,
    )));

    // Hold W. Pressing every frame keeps the button state alive regardless of
    // how Bevy's input plumbing clears it.
    let mut max_height = f32::NEG_INFINITY;
    let mut max_vertical_speed = 0.0_f32;
    let mut minimum_upright = 1.0_f32;
    // Continue far beyond the old 98 m coverage so the car crosses several
    // 100 m road-chunk boundaries at high speed.
    for _ in 0..900 {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        app.update();
        {
            let world = app.world_mut();
            let mut motion = world.query_filtered::<(&Transform, &LinearVelocity), With<Vehicle>>();
            let (transform, velocity) = motion.single(world).expect("vehicle motion");
            max_height = max_height.max(transform.translation.y);
            max_vertical_speed = max_vertical_speed.max(velocity.y.abs());
            minimum_upright = minimum_upright.min((transform.rotation * Vec3::Y).dot(Vec3::Y));
        }
    }

    let (transform, speed_kmh, rpm, gear) = {
        let (transform, vehicle) = vehicle_state(&mut app);
        (
            transform,
            vehicle.speed_kmh(),
            vehicle.dynamics.rpm(),
            vehicle.dynamics.transmission.gear,
        )
    };
    let travelled = road.distance_along(transform.translation) - start_distance;
    let (_, lateral) = road.local_coordinates(transform.translation);

    println!(
        "after full throttle: {speed_kmh:.1} km/h, {rpm:.0} rpm, gear {gear},          travelled {travelled:.1} m, max y {max_height:.3} m, max |vy| {max_vertical_speed:.3} m/s"
    );

    assert!(
        speed_kmh > 30.0,
        "the car barely moved under full throttle: {speed_kmh:.1} km/h"
    );
    assert!(
        travelled > 10.0,
        "the car only travelled {travelled:.1} m along the road"
    );
    assert!(travelled > 300.0, "the seam test crossed too little road");
    assert!(
        max_height < 0.5,
        "the car was launched to y={max_height:.2} m"
    );
    assert!(
        max_vertical_speed < 3.0,
        "road seam produced {max_vertical_speed:.2} m/s vertical speed"
    );
    assert!(
        minimum_upright > 0.97,
        "the car tipped while crossing a road seam: up dot {minimum_upright:.3}"
    );
    assert!(rpm > 1200.0, "the engine never picked up: {rpm:.0} rpm");
    assert!(
        lateral.abs() < road.paved_half_width(),
        "the car left the road: lateral = {lateral}"
    );
}

#[test]
fn the_chase_camera_keeps_the_car_in_frame_at_speed() {
    let mut app = headless_app();
    for _ in 0..40 {
        app.update();
        std::thread::sleep(Duration::from_millis(4));
    }

    // Accelerate hard, sampling how far the camera trails the car once the
    // speed is up. Its local offset is smoothed, but rendered translation must
    // follow the car directly and remain independent of road speed.
    let mut worst_excess: f32 = 0.0;
    let mut top_speed: f32 = 0.0;
    for _ in 0..500 {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        app.update();
        std::thread::sleep(Duration::from_millis(6));

        let world = app.world_mut();
        let mut cars = world.query::<(&Transform, &Vehicle)>();
        let Some((car, vehicle)) = cars.iter(world).next() else {
            continue;
        };
        let (car_position, speed) = (car.translation, vehicle.speed_kmh());
        if speed < 80.0 {
            continue;
        }
        top_speed = top_speed.max(speed);

        let mut cameras = world.query::<(&Transform, &GameCamera)>();
        let Some((camera_transform, camera)) = cameras.iter(world).next() else {
            continue;
        };
        let nominal = camera.mode_offset_length();
        let actual = camera_transform.translation.distance(car_position);
        worst_excess = worst_excess.max(actual - nominal);
    }

    println!(
        "chase camera: reached {top_speed:.0} km/h, worst excess distance {worst_excess:.2} m"
    );
    assert!(top_speed > 100.0, "the car never got up to speed");
    assert!(
        worst_excess < 6.0,
        "the camera trailed {worst_excess:.1} m behind its nominal offset - \
         the local-offset camera follow is not working"
    );
}

/// Fetch the single vehicle's transform and a copy of the fields worth checking.
fn vehicle_state(app: &mut App) -> (Transform, &Vehicle) {
    let world = app.world_mut();
    let mut query = world.query::<(&Transform, &Vehicle)>();
    let (transform, vehicle) = query
        .iter(world)
        .next()
        .expect("exactly one vehicle should exist");
    (*transform, vehicle)
}

#[test]
fn the_world_contains_the_expected_static_geometry() {
    let mut app = headless_app();
    app.update();

    let world = app.world_mut();
    let spec = world.resource::<RoadSpec>().clone();

    let mut named = world.query::<&Name>();
    let names: Vec<String> = named.iter(world).map(|n| n.as_str().to_string()).collect();

    let count = |prefix: &str| names.iter().filter(|n| n.starts_with(prefix)).count();
    let exact = |name: &str| names.iter().filter(|n| n.as_str() == name).count();

    assert_eq!(count("RoadSurface"), spec.chunk_count(), "road chunks");
    assert!(
        count("LoopExtensionSurfaceBefore") > 0 && count("LoopExtensionSurfaceAfter") > 0,
        "the infinite road needs visual asphalt on both sides of its wrap"
    );
    assert!(
        count("LoopExtensionRailBefore") > 0 && count("LoopExtensionRailAfter") > 0,
        "the guard rails must continue across both sides of the wrap"
    );
    assert!(
        count("LoopExtensionRailColliderBefore") > 0 && count("LoopExtensionRailColliderAfter") > 0,
        "the visual continuation must also retain guard-rail collisions"
    );
    assert!(count("LoopExtensionHill") > 0);
    assert_eq!(count("RoadMarkings"), spec.chunk_count(), "marking chunks");
    assert_eq!(
        count("Barrier") - count("BarrierCollider"),
        spec.chunk_count() * 2,
        "barrier meshes"
    );
    assert_eq!(
        count("BarrierCollider"),
        spec.chunk_count() * 2,
        "barrier colliders"
    );
    assert_eq!(count("DistanceMarker"), 33, "kilometre boards");
    assert_eq!(
        exact("GuardrailModelModule"),
        0,
        "the imported guardrail model must not be spawned"
    );
    assert_eq!(exact("Sun"), 1);
    assert_eq!(exact("RoadCollider"), 1, "one seamless road collider");
    assert_eq!(exact("TerrainCollider"), 1);
    assert_eq!(count("ContainmentCollider"), 2, "seamless safety walls");
    assert_eq!(
        count("Terrain") - exact("TerrainCollider"),
        spec.chunk_count(),
        "terrain chunks"
    );
    assert_eq!(exact("Vehicle"), 1);
    assert_eq!(count("VehicleModel"), 1, "the car model root");
}

#[test]
fn player_crosses_the_finish_without_losing_speed() {
    let mut app = headless_app();
    for _ in 0..4 {
        app.update();
    }
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 120.0,
    )));
    let road = app.world().resource::<RoadSpec>().clone();
    let initial_velocity = Vec3::new(1.5, 0.0, 80.0);
    {
        let world = app.world_mut();
        let mut player = world
            .query_filtered::<(&mut Transform, &mut Position, &mut LinearVelocity), With<Vehicle>>(
            );
        let (mut transform, mut position, mut velocity) =
            player.single_mut(world).expect("one player vehicle");
        let beyond_finish = Vec3::new(-5.4, 0.8, road.length + 12.0);
        transform.translation = beyond_finish;
        position.0 = beyond_finish;
        velocity.0 = initial_velocity;
    }

    app.update();
    let world = app.world_mut();
    let mut player =
        world.query_filtered::<(&Transform, &Position, &LinearVelocity), With<Vehicle>>();
    let (transform, position, velocity) = player.single(world).expect("one player vehicle");
    assert!(
        (10.0..15.0).contains(&position.z),
        "finish did not wrap to the start: z={}",
        position.z
    );
    assert!((transform.translation.z - position.z).abs() < 0.01);
    assert!((velocity.x - initial_velocity.x).abs() < 0.1);
    assert!(
        velocity.z > initial_velocity.z - 0.5,
        "forward speed was lost"
    );
    let car_translation = transform.translation;
    let mut cameras = world.query::<(&Transform, &GameCamera)>();
    let (camera_pose, camera) = cameras.single(world).expect("one player camera");
    assert!(
        camera_pose.translation.distance(car_translation) < camera.mode_offset_length() + 5.0,
        "camera did not follow the teleport across the road seam"
    );

    for _ in 0..60 {
        app.update();
    }
    let world = app.world_mut();
    let mut player = world.query_filtered::<(&Transform, &LinearVelocity), With<Vehicle>>();
    let (pose, velocity) = player.single(world).expect("one player vehicle");
    assert!((35.0..100.0).contains(&pose.translation.z));
    assert!(
        (-1.0..3.0).contains(&pose.translation.y),
        "car launched after the seam"
    );
    assert!(
        velocity.z > 60.0,
        "physics lost forward motion after the seam"
    );
}

#[test]
fn reversing_past_the_start_rejoins_the_end_without_changing_direction() {
    let mut app = headless_app();
    for _ in 0..4 {
        app.update();
    }
    let road = app.world().resource::<RoadSpec>().clone();
    {
        let world = app.world_mut();
        let mut player = world
            .query_filtered::<(&mut Transform, &mut Position, &mut LinearVelocity), With<Vehicle>>(
            );
        let (mut transform, mut position, mut velocity) =
            player.single_mut(world).expect("one player vehicle");
        transform.translation.z = -4.0;
        position.z = -4.0;
        velocity.z = -18.0;
    }
    app.update();
    let world = app.world_mut();
    let mut player = world.query_filtered::<(&Position, &LinearVelocity), With<Vehicle>>();
    let (position, velocity) = player.single(world).expect("one player vehicle");
    assert!((position.z - (road.length - 4.0)).abs() < 1.0);
    assert!(velocity.z < -17.0, "reverse velocity was lost at the seam");
}

#[test]
fn traffic_uses_the_imported_cars_without_synthetic_position_lamps() {
    let mut app = headless_app();
    for _ in 0..3 {
        app.update();
    }
    let expected_vehicles = app
        .world()
        .resource::<TrafficSettings>()
        .density
        .vehicle_count();
    let world = app.world_mut();
    let mut traffic = world.query_filtered::<Entity, With<TrafficVehicle>>();
    assert_eq!(traffic.iter(world).count(), expected_vehicles);

    let mut names = world.query::<&Name>();
    assert!(
        names
            .iter(world)
            .all(|name| !name.as_str().contains("Position Lamp")),
        "traffic must use only the supplied vehicle models without synthetic lamps"
    );
    for imported in ["2011 Mercedes M-Class", "2017 Kia Picanto GT-Line"] {
        assert!(
            names
                .iter(world)
                .any(|name| name.as_str().contains(imported)),
            "offline traffic did not spawn {imported}"
        );
    }
    assert!(
        names
            .iter(world)
            .filter(|name| name.as_str().starts_with("TrafficVisual:"))
            .all(|name| !name.as_str().contains("BUGATTI")),
        "the player's Bugatti must never be reused as an NPC traffic car"
    );
}

#[test]
fn traffic_stops_before_a_static_obstacle_in_its_lane() {
    let mut app = headless_app();
    for _ in 0..4 {
        app.update();
    }
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 60.0,
    )));

    let road = app.world().resource::<RoadSpec>().clone();
    let traffic_start_z = 200.0;
    let obstacle_z = 280.0;
    let lane_x = {
        let world = app.world_mut();
        let mut traffic = world.query::<(
            &TrafficVehicle,
            &mut Transform,
            &mut Position,
            &mut LinearVelocity,
        )>();
        let (vehicle, mut transform, mut position, mut velocity) = traffic
            .iter_mut(world)
            .find(|(vehicle, ..)| vehicle.slot == 3)
            .expect("same-direction traffic vehicle");
        let lane_x = road.lane_center_x(vehicle.lane, vehicle.oncoming);
        transform.translation.x = lane_x;
        transform.translation.z = traffic_start_z;
        position.x = lane_x;
        position.z = traffic_start_z;
        velocity.0 = Vec3::Z * vehicle.cruise_speed_mps;
        lane_x
    };

    let obstacle = app
        .world_mut()
        .spawn((
            Name::new("Traffic braking test obstacle"),
            RigidBody::Static,
            Collider::cuboid(2.8, 2.0, 4.0),
            Transform::from_xyz(lane_x, 1.0, obstacle_z),
        ))
        .id();

    for _ in 0..600 {
        app.update();
    }

    let stopped_z = {
        let world = app.world_mut();
        let mut traffic = world.query::<(&TrafficVehicle, &Position, &LinearVelocity)>();
        let (_, position, velocity) = traffic
            .iter(world)
            .find(|(vehicle, ..)| vehicle.slot == 3)
            .expect("same-direction traffic vehicle remained active");
        assert!(
            velocity.length() < 0.5,
            "traffic did not stop: velocity={:?}",
            velocity.0
        );
        assert!(
            position.z > traffic_start_z + 50.0,
            "sensor mistook the road surface for an obstacle: z={}",
            position.z
        );
        assert!(
            position.z < obstacle_z - 14.0,
            "traffic stopped too close to or passed the obstacle: z={}",
            position.z
        );
        position.z
    };

    app.world_mut().entity_mut(obstacle).despawn();
    for _ in 0..240 {
        app.update();
    }

    let world = app.world_mut();
    let mut traffic = world.query::<(&TrafficVehicle, &Position, &LinearVelocity)>();
    let (_, position, velocity) = traffic
        .iter(world)
        .find(|(vehicle, ..)| vehicle.slot == 3)
        .expect("same-direction traffic vehicle remained active");
    assert!(velocity.z > 9.0, "traffic did not resume: {:?}", velocity.0);
    assert!(position.z > stopped_z + 10.0);
}

#[test]
fn airborne_high_speed_impact_cannot_cross_the_guard_rail_backstop() {
    let mut app = headless_app();
    for _ in 0..20 {
        app.update();
    }
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 60.0,
    )));

    let road = app.world().resource::<RoadSpec>().clone();
    let launch = Vec3::new(road.barrier_offset - 2.0, 3.0, 120.0);
    {
        let world = app.world_mut();
        let mut vehicles = world
            .query_filtered::<(&mut Transform, &mut Position, &mut LinearVelocity), With<Vehicle>>(
            );
        let (mut transform, mut position, mut velocity) =
            vehicles.single_mut(world).expect("one player vehicle");
        transform.translation = launch;
        position.0 = launch;
        velocity.0 = Vec3::new(100.0, 0.0, 0.0);
    }

    let mut furthest_lateral = launch.x;
    for _ in 0..90 {
        app.update();
        let world = app.world_mut();
        let mut vehicles = world.query_filtered::<&Transform, With<Vehicle>>();
        furthest_lateral = furthest_lateral.max(
            vehicles
                .single(world)
                .expect("one player vehicle")
                .translation
                .x,
        );
    }

    assert!(
        furthest_lateral < road.protected_surface_half_width(),
        "airborne 360 km/h side impact escaped to x={furthest_lateral:.2}"
    );
}

#[test]
fn coasting_angled_rail_impact_rebounds_before_the_car_stops() {
    for side in [-1.0_f32, 1.0] {
        let mut app = headless_app();
        for _ in 0..20 {
            app.update();
        }
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 60.0,
        )));

        let road = app.world().resource::<RoadSpec>().clone();
        let launch = Vec3::new(side * (road.barrier_offset - 1.5), 0.05, 250.0);
        let heading = Quat::from_rotation_y(core::f32::consts::PI + side * 0.12);
        {
            let world = app.world_mut();
            let mut vehicles = world.query_filtered::<(
                &mut Transform,
                &mut Position,
                &mut Rotation,
                &mut LinearVelocity,
                &mut Vehicle,
            ), With<Vehicle>>();
            let (mut transform, mut position, mut rotation, mut velocity, mut vehicle) =
                vehicles.single_mut(world).expect("one player vehicle");
            transform.translation = launch;
            transform.rotation = heading;
            position.0 = launch;
            rotation.0 = heading;
            velocity.0 = Vec3::new(side * 3.6, 0.0, 30.0);
            vehicle.input = Default::default();
        }

        for _ in 0..60 {
            app.update();
        }
        let world = app.world_mut();
        let mut vehicles = world.query_filtered::<(&Position, &LinearVelocity), With<Vehicle>>();
        let (position, velocity) = vehicles.single(world).expect("one player vehicle");
        let distance_from_rail = road.barrier_offset - side * position.0.x;
        assert!(
            distance_from_rail > 2.1,
            "car stayed stuck on side {side}: x={:.2}, vx={:.2}",
            position.0.x,
            velocity.0.x
        );
        assert!(
            velocity.0.z > 12.0,
            "rail stopped the coasting car instead of deflecting it"
        );
    }
}
