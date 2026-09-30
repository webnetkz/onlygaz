//! Render a single frame of the simulation to a PNG, without opening a window.
//!
//! Useful for eyeballing the scene on a machine with no display, and as a visual
//! regression check. It builds the real game, lets the world settle (optionally
//! with the throttle held), points the game camera at an off-screen image and
//! writes the result to disk.
//!
//! ```bash
//! cargo run --release --example render_still -- shot.png 3.0 0 0.0 0.0
//! #                                             file    settle cam throttle nitro
//! ```
//!
//! On a machine without a GPU a software Vulkan driver (`mesa-vulkan-drivers`
//! on Debian and Ubuntu) is enough, though it is slow. The camera only starts
//! rendering for the last few frames, so the settling phase stays fast.

use std::time::Duration;

use avian3d::prelude::Position;
use bevy::camera::{RenderTarget, ShadowLodOrigin};
use bevy::light::DirectionalLightShadowMap;
use bevy::prelude::*;
use bevy::render::render_resource::{TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;

use veyron_sim::camera::controller::GameCamera;
use veyron_sim::game::{GamePlugin, GameState};

/// Output resolution, overridable with `VEYRON_SHOT_WIDTH` and
/// `VEYRON_SHOT_HEIGHT` - software rendering is slow enough that being able to
/// preview at a quarter of the size is worth the two lines.
fn resolution() -> (u32, u32) {
    let read = |name: &str, default: u32| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(default)
    };
    (
        read("VEYRON_SHOT_WIDTH", 1280),
        read("VEYRON_SHOT_HEIGHT", 720),
    )
}
/// Wall-clock step used to drive the app by hand.
const STEP: Duration = Duration::from_millis(8);

#[derive(Resource)]
struct TargetSize {
    width: u32,
    height: u32,
}

#[derive(Resource)]
struct Target {
    /// Tiny target used while the world settles.
    warmup: Handle<Image>,
    /// Full size target used for the frame that is actually saved.
    full: Handle<Image>,
}

struct Options {
    path: String,
    settle_seconds: f32,
    camera_index: usize,
    throttle_seconds: f32,
    nitro_seconds: f32,
    brake_seconds: f32,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let options = Options {
        path: args.next().unwrap_or_else(|| "shot.png".to_string()),
        settle_seconds: args.next().and_then(|v| v.parse().ok()).unwrap_or(3.0),
        camera_index: args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
        throttle_seconds: args.next().and_then(|v| v.parse().ok()).unwrap_or(0.0),
        nitro_seconds: args.next().and_then(|v| v.parse().ok()).unwrap_or(0.0),
        brake_seconds: std::env::var("VEYRON_SHOT_BRAKE_SECONDS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0.0),
    };
    let (width, height) = resolution();
    println!(
        "rendering {} ({width}x{height}), settle {:.1} s, camera {}, throttle {:.1} s, nitro {:.1} s, brake {:.1} s",
        options.path,
        options.settle_seconds,
        options.camera_index,
        options.throttle_seconds,
        options.nitro_seconds,
        options.brake_seconds
    );

    let shot_state = GameState::Driving;

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                close_when_requested: false,
                ..default()
            })
            .build()
            .disable::<WinitPlugin>(),
    )
    .add_plugins(GamePlugin)
    .insert_state(shot_state)
    // Software rasterisers hate big shadow maps.
    .insert_resource(DirectionalLightShadowMap { size: 1024 })
    // A software-rendered frame can take seconds. Without this the virtual
    // clock would jump a quarter of a second every frame and the car would
    // shoot away from the camera, which is an artefact of the harness rather
    // than of the game.
    .insert_resource({
        let mut virtual_time = Time::<Virtual>::default();
        virtual_time.set_max_delta(Duration::from_millis(16));
        virtual_time
    })
    .insert_resource(TargetSize { width, height })
    .add_systems(Startup, create_target)
    .add_systems(Update, attach_warmup_target);

    // `App::update` does not run the plugins' `finish`/`cleanup` hooks.
    app.finish();
    app.cleanup();

    // Night visual checks use the same setting as the in-game menu.
    // Example: VEYRON_SHOT_TIME=night cargo run --example render_still -- ...
    if let Ok(value) = std::env::var("VEYRON_SHOT_TIME") {
        use veyron_sim::world::lighting::{LightingSettings, TimeOfDay};
        let time = match value.trim().to_ascii_lowercase().as_str() {
            "night" => TimeOfDay::Night,
            _ => TimeOfDay::Day,
        };
        app.world_mut()
            .resource_mut::<LightingSettings>()
            .time_of_day = time;
    }

    let steps = |seconds: f32| (seconds / STEP.as_secs_f32()).ceil() as u32;

    // Let assets load and the suspension settle. The camera renders into a
    // 64x64 image while it does: a camera with no valid target produces no
    // view at all, and the atmosphere's environment map cannot cope with that,
    // while rendering the full frame every step would take minutes on a
    // software rasteriser.
    for _ in 0..steps(options.settle_seconds) {
        app.update();
        std::thread::sleep(Duration::from_millis(1));
    }

    // Repeatable crash renders exercise the real contact solver and visual
    // damage path. This exists only in the off-screen validation harness.
    if let Ok(speed) = std::env::var("VEYRON_SHOT_CRASH_KMH") {
        use avian3d::prelude::{Collider, LinearVelocity, RigidBody};
        use veyron_sim::vehicle::physics::Vehicle;
        let speed = speed.parse::<f32>().unwrap_or(520.0).clamp(0.0, 800.0);
        let world = app.world_mut();
        world.spawn((
            Name::new("Render validation impact wall"),
            RigidBody::Static,
            Collider::cuboid(20.0, 3.0, 1.0),
            Transform::from_xyz(-5.4, 1.5, 254.0),
        ));
        let mut cars = world
            .query_filtered::<(&mut Transform, &mut Position, &mut LinearVelocity), With<Vehicle>>(
            );
        for (mut pose, mut position, mut velocity) in cars.iter_mut(world) {
            pose.translation = Vec3::new(-5.4, 0.05, 250.0);
            position.0 = pose.translation;
            velocity.0 = Vec3::new(0.0, 0.0, speed / 3.6);
        }
        for _ in 0..24 {
            app.update();
            std::thread::sleep(Duration::from_millis(8));
        }
    }

    // Place one selected NPC at a repeatable distance for visual model checks
    // without changing normal traffic placement in the actual game.
    if let Ok(value) = std::env::var("VEYRON_SHOT_TRAFFIC_NEAR") {
        use veyron_sim::vehicle::physics::Vehicle;
        use veyron_sim::world::traffic::TrafficVehicle;

        let ahead = value.parse::<f32>().unwrap_or(42.0).clamp(10.0, 1500.0);
        let selected_slot = std::env::var("VEYRON_SHOT_TRAFFIC_SLOT")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);

        let player_position = {
            let world = app.world_mut();
            let mut player = world.query_filtered::<&Transform, With<Vehicle>>();
            player
                .iter(world)
                .next()
                .map(|transform| transform.translation)
        };
        if let Some(player_position) = player_position {
            let world = app.world_mut();
            let mut traffic = world.query::<(&TrafficVehicle, &mut Position, &mut Transform)>();
            for (vehicle, mut position, mut transform) in traffic.iter_mut(world) {
                if vehicle.slot == selected_slot {
                    // Centre the selected NPC in the validation camera even
                    // when its normal traffic slot belongs to another lane.
                    position.x = player_position.x;
                    position.z = player_position.z + ahead;
                    transform.translation = position.0;
                    println!(
                        "traffic slot {selected_slot} moved to ({:.2}, {:.2}, {:.2})",
                        position.x, position.y, position.z
                    );
                    break;
                }
            }
        }
        for _ in 0..4 {
            app.update();
        }
    }

    // Optionally drive, so the picture is not of a parked car.
    for _ in 0..steps(options.throttle_seconds) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        app.update();
        std::thread::sleep(Duration::from_millis(1));
    }

    // Optional held nitro verifies the complete physics, fullscreen shader and
    // exhaust-flame path in one rendered frame.
    for _ in 0..steps(options.nitro_seconds) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::KeyW);
        keys.press(KeyCode::ShiftLeft);
        app.update();
        std::thread::sleep(Duration::from_millis(1));
    }

    // Optional emergency stop for visual tyre-mark regression shots. Disable
    // ABS for one frame so every contact patch locks and leaves an unambiguous
    // full-width strip, then release the throttle and hold the real brake key.
    if options.brake_seconds > 0.0 {
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::KeyW);
            keys.release(KeyCode::ShiftLeft);
            keys.press(KeyCode::F8);
        }
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::F8);
        for _ in 0..steps(options.brake_seconds) {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyS);
            app.update();
            std::thread::sleep(Duration::from_millis(1));
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyS);
    }

    // Select the requested camera mode and let the smoothing catch up.
    {
        let world = app.world_mut();
        let mut cameras = world.query::<&mut GameCamera>();
        for mut camera in cameras.iter_mut(world) {
            for _ in 0..options.camera_index {
                camera.cycle();
            }
            if std::env::var_os("VEYRON_SHOT_REAR").is_some() {
                camera.rear_view = true;
            }
        }
    }
    if std::env::var_os("VEYRON_SHOT_REAR").is_some() {
        // The game's held-view input system owns this flag each frame, so
        // setting GameCamera.rear_view alone is immediately overwritten.
        let key = app
            .world()
            .resource::<veyron_sim::vehicle::controller::KeyBindings>()
            .toggle_rear_view;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
    }
    for _ in 0..14 {
        app.update();
    }

    // Optional live preset regression check, including returning to SDR-era
    // Performance settings after the atmosphere pipelines have been cached.
    if std::env::var_os("VEYRON_CHECK_PRESETS").is_some() {
        use veyron_sim::world::lighting::{LightingQuality, LightingSettings};
        for quality in [
            LightingQuality::Balanced,
            LightingQuality::Cinematic,
            LightingQuality::Performance,
        ] {
            app.world_mut()
                .resource_mut::<LightingSettings>()
                .apply_preset(quality);
            for _ in 0..40 {
                app.update();
            }
        }
    }
    if let Ok(page) = std::env::var("VEYRON_SHOT_MENU") {
        use veyron_sim::ui::menu::PauseMenuPage;
        *app.world_mut().resource_mut::<PauseMenuPage>() = match page.as_str() {
            "graphics" => PauseMenuPage::Graphics,
            "controls" => PauseMenuPage::Controls,
            "audio" => PauseMenuPage::Audio,
            _ => PauseMenuPage::Main,
        };
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Paused);
    }

    // Only now switch to the full resolution target, so exactly one large frame
    // is drawn.
    let handle = app.world().resource::<Target>().full.clone();
    {
        let target = RenderTarget::Image(handle.clone().into());
        let world = app.world_mut();
        let mut cameras = world.query_filtered::<Entity, With<GameCamera>>();
        let entities: Vec<Entity> = cameras.iter(world).collect();
        for entity in entities {
            world.entity_mut(entity).insert(target.clone());
        }
    }
    // Enough frames for the interface to be laid out for the new target size,
    // and - much more importantly - for the temporal passes to converge.
    // Temporal anti-aliasing, the temporal shadow filter and the atmosphere's
    // environment map all accumulate over several frames, and changing the
    // render target throws that history away, so a screenshot taken two frames
    // later is dark and noisy through no fault of the scene.
    for _ in 0..28 {
        app.update();
    }

    if std::env::var_os("VEYRON_SHOT_SCROLL_BOTTOM").is_some() {
        let world = app.world_mut();
        let mut panels = world.query::<(&mut ScrollPosition, &ComputedNode)>();
        for (mut scroll, node) in panels.iter_mut(world) {
            scroll.y =
                ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
        }
        for _ in 0..4 {
            app.update();
        }
    }

    {
        let world = app.world_mut();
        let mut cars =
            world.query_filtered::<&Transform, With<veyron_sim::vehicle::physics::Vehicle>>();
        let car = cars.iter(world).next().copied();
        let mut cams = world.query_filtered::<&Transform, With<GameCamera>>();
        let cam = cams.iter(world).next().copied();
        if let (Some(car), Some(cam)) = (car, cam) {
            let local = car.rotation.inverse() * (cam.translation - car.translation);
            println!(
                "car at ({:.2}, {:.2}, {:.2}), camera at ({:.2}, {:.2}, {:.2}), \
                 camera in car frame ({:+.2}, {:+.2}, {:+.2})",
                car.translation.x,
                car.translation.y,
                car.translation.z,
                cam.translation.x,
                cam.translation.y,
                cam.translation.z,
                local.x,
                local.y,
                local.z
            );
        }
    }

    if options.brake_seconds > 0.0 {
        use veyron_sim::vehicle::skid_marks::EmergencySkidMark;
        let world = app.world_mut();
        let mut marks = world.query_filtered::<(&Name, &Transform), With<EmergencySkidMark>>();
        let mut count = 0usize;
        let mut widths = std::collections::BTreeMap::new();
        for (name, transform) in marks.iter(world) {
            count += 1;
            widths.insert(name.as_str().to_owned(), transform.scale.x);
        }
        println!("skid segments {count}, quad widths {widths:?}");
    }

    if std::env::var_os("VEYRON_SHOT_FOREST_DIAGNOSTICS").is_some() {
        use veyron_sim::world::forest::{
            CinematicForestChunk, CinematicForestMesh, CinematicForestTree,
        };
        use veyron_sim::world::lighting::LightingSettings;

        let world = app.world_mut();
        let quality = world.resource::<LightingSettings>().quality;
        let mut chunks = world.query_filtered::<Entity, With<CinematicForestChunk>>();
        let chunk_count = chunks.iter(world).count();
        let mut trees = world.query_filtered::<Entity, With<CinematicForestTree>>();
        let tree_count = trees.iter(world).count();
        let mut meshes = world.query_filtered::<Entity, With<CinematicForestMesh>>();
        let mesh_count = meshes.iter(world).count();
        println!(
            "forest diagnostics: quality={quality:?}, chunks={chunk_count}, trees={tree_count}, meshes={mesh_count}"
        );
    }

    if std::env::var_os("VEYRON_SHOT_LIGHT_DIAGNOSTICS").is_some() {
        let world = app.world_mut();
        let mut lights = world.query::<(&Name, &SpotLight, &GlobalTransform)>();
        for (name, light, transform) in lights.iter(world) {
            let position = transform.translation();
            let direction = transform.forward();
            println!(
                "{}: {:.0} lm at ({:.2}, {:.2}, {:.2}), direction ({:.3}, {:.3}, {:.3})",
                name,
                light.intensity,
                position.x,
                position.y,
                position.z,
                direction.x,
                direction.y,
                direction.z
            );
        }
        let mut exposure = world.query_filtered::<&bevy::camera::Exposure, With<GameCamera>>();
        for exposure in exposure.iter(world) {
            println!("camera exposure: EV100 {:.2}", exposure.ev100);
        }
    }

    let path = options.path.clone();
    app.world_mut()
        .spawn(Screenshot::image(handle))
        .observe(save_to_disk(path.clone()));

    // The capture takes a few frames to make it back from the GPU.
    for _ in 0..500 {
        app.update();
        if std::path::Path::new(&path).exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        std::path::Path::new(&path).exists(),
        "Screenshot was not saved: {path}"
    );
    println!("wrote {path}");
}

fn create_target(mut commands: Commands, size: Res<TargetSize>, mut images: ResMut<Assets<Image>>) {
    let mut make = |width: u32, height: u32| {
        let mut image = Image::new_target_texture(
            width,
            height,
            TextureFormat::Rgba8Unorm,
            Some(TextureFormat::Rgba8UnormSrgb),
        );
        // The screenshot reads the texture back, which needs an explicit flag.
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        images.add(image)
    };
    commands.insert_resource(Target {
        warmup: make(64, 64),
        full: make(size.width, size.height),
    });
}

/// Point the camera at the small warm-up target as soon as it exists.
///
/// `Camera3d` already carries a `RenderTarget` for the primary window, which
/// does not exist here, so this replaces it rather than adding a missing one.
fn attach_warmup_target(
    mut commands: Commands,
    target: Res<Target>,
    cameras: Query<Entity, With<GameCamera>>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    for entity in &cameras {
        commands.entity(entity).insert((
            RenderTarget::Image(target.warmup.clone().into()),
            ShadowLodOrigin,
        ));
        *done = true;
    }
}
