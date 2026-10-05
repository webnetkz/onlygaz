//! Google Play screenshots of the phone build: 1920×1080 (16:9 landscape, the
//! shape Play asks for) with the touch controls on screen.
//!
//! ```sh
//! cargo run --features mobile --example store_screenshots -- out_dir
//! ```
//!
//! Drives the phone interface with synthetic touches like
//! `mobile_screenshots`: starts a race without damage, holds GAS, then NITRO,
//! switches cameras, turns night on and opens the CONTROLS page, saving a PNG
//! at every step. Uses the Balanced graphics preset, one of the presets the
//! phone build offers in GRAPHICS. Works with a software Vulkan driver (Mesa
//! lavapipe under Xvfb, with the X pointer outside the window), just slowly.

use std::path::PathBuf;
use std::time::Duration;

use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::{PrimaryWindow, WindowResolution};

use veyron_sim::camera::controller::GameCamera;
use veyron_sim::camera::modes::CameraMode;
use veyron_sim::mobile::{SteeringMode, TouchControlKind, TouchControlSettings};
use veyron_sim::world::lighting::{LightingQuality, LightingSettings, TimeOfDay};
use veyron_sim::world::traffic::{TrafficDensity, TrafficSettings};

/// Size of the saved screenshots [physical pixels].
const SIZE: (u32, u32) = (1920, 1080);

#[derive(Debug, Clone, Copy)]
enum Action {
    Wait(f64),
    Shot(&'static str),
    Preset(LightingQuality),
    Traffic(TrafficDensity),
    Night(bool),
    Camera(CameraMode),
    Steering(SteeringMode),
    /// Finger `id` lands on the visible text label (e.g. a button title).
    TouchText(u64, &'static str),
    /// Finger `id` lands on the centre of an on-screen driving control.
    TouchControl(u64, TouchControlKind),
    Lift(u64),
}

use Action as A;

const SCRIPT: &[Action] = &[
    A::Wait(12.0),
    A::Preset(LightingQuality::Balanced),
    A::Traffic(TrafficDensity::Low),
    A::Wait(8.0),
    A::Shot("01_main_menu"),
    A::TouchText(10, "PLAY"),
    A::Wait(0.3),
    A::Lift(10),
    A::Wait(2.0),
    A::TouchText(11, "DRIVE WITHOUT DAMAGE"),
    A::Wait(0.3),
    A::Lift(11),
    A::Wait(10.0),
    A::Steering(SteeringMode::Buttons),
    A::Wait(2.0),
    A::TouchControl(1, TouchControlKind::Gas),
    A::Wait(12.0),
    A::Shot("02_full_throttle"),
    A::TouchControl(2, TouchControlKind::Nitro),
    A::Wait(5.0),
    A::Shot("03_nitro"),
    A::Lift(2),
    A::Wait(3.0),
    A::Camera(CameraMode::Cockpit),
    A::Wait(4.0),
    A::Shot("04_cockpit"),
    A::Camera(CameraMode::Hood),
    A::Wait(3.0),
    A::Shot("05_hood"),
    A::Camera(CameraMode::ChaseFar),
    A::Wait(3.0),
    A::Shot("06_chase_far"),
    A::Camera(CameraMode::ChaseClose),
    A::Night(true),
    A::Wait(6.0),
    A::Shot("07_night"),
    A::Camera(CameraMode::Hood),
    A::Wait(3.0),
    A::Shot("08_night_hood"),
    A::Camera(CameraMode::Cockpit),
    A::Wait(3.0),
    A::Shot("09_night_cockpit"),
    A::Lift(1),
    A::Camera(CameraMode::ChaseClose),
    A::Wait(2.0),
    A::TouchControl(5, TouchControlKind::Menu),
    A::Wait(0.3),
    A::Lift(5),
    A::Wait(3.0),
    A::TouchText(12, "CONTROLS"),
    A::Wait(0.3),
    A::Lift(12),
    A::Wait(4.0),
    A::Shot("10_controls"),
    A::Wait(15.0),
];

#[derive(Resource)]
struct Script {
    out: PathBuf,
    step: usize,
    resume_at: f64,
    held: Vec<(u64, Vec2)>,
    retrying_since: Option<f64>,
}

/// How long a step waits for its button or control to appear.
const RETRY_FOR_SECONDS: f64 = 60.0;

fn main() {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "store_screenshots".to_string()),
    );
    std::fs::create_dir_all(&out).expect("output directory");
    let _ = std::fs::remove_file(out.join("settings.ron"));
    // SAFETY: single-threaded at this point, before Bevy starts any thread.
    unsafe {
        // Do not touch the player's real settings file.
        std::env::set_var("VEYRON_SETTINGS_PATH", out.join("settings.ron"));
    }
    let mut app = veyron_sim::mobile::build_mobile_app();
    {
        let world = app.world_mut();
        let mut windows = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
        for mut window in windows.iter_mut(world) {
            window.resolution = WindowResolution::new(SIZE.0, SIZE.1).with_scale_factor_override(1.0);
        }
    }
    // Software rendering manages well under one frame per second at this
    // size: let game time keep pace with real time instead of slowing down.
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .set_max_delta(Duration::from_secs(4));
    app.insert_resource(Script {
        out,
        step: 0,
        resume_at: 0.0,
        held: Vec::new(),
        retrying_since: None,
    })
    .add_systems(Update, run_script);
    app.run();
}

#[allow(clippy::too_many_arguments)]
fn run_script(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut script: ResMut<Script>,
    mut touch_settings: ResMut<TouchControlSettings>,
    mut lighting: ResMut<LightingSettings>,
    mut traffic: ResMut<TrafficSettings>,
    mut cameras: Query<&mut GameCamera>,
    mut touches: MessageWriter<TouchInput>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    controls: Query<(&TouchControlKind, &UiGlobalTransform)>,
    texts: Query<(&Text, &UiGlobalTransform, &InheritedVisibility)>,
    mut exit: MessageWriter<AppExit>,
) {
    let now = time.elapsed_secs_f64();
    let Ok((window, window_info)) = windows.single() else {
        return;
    };
    let scale = window_info.scale_factor();
    let mut send = |phase, id, position| {
        touches.write(TouchInput {
            phase,
            position,
            window,
            force: None,
            id,
        });
    };
    // Keep held fingers alive so Bevy keeps them in `Touches`.
    for (id, position) in script.held.clone() {
        send(TouchPhase::Moved, id, position);
    }
    while now >= script.resume_at {
        let Some(action) = SCRIPT.get(script.step).copied() else {
            info!("store screenshot script finished");
            exit.write(AppExit::Success);
            return;
        };
        script.step += 1;
        match action {
            A::Wait(seconds) => script.resume_at = now + seconds,
            A::Shot(name) => {
                let path = script.out.join(format!("{name}.png"));
                info!("screenshot {}", path.display());
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(path));
                script.resume_at = now + 1.0;
            }
            A::Preset(quality) => {
                lighting.apply_preset(quality);
                info!("graphics preset {quality:?}");
            }
            A::Traffic(density) => traffic.density = density,
            A::Night(night) => {
                lighting.time_of_day = if night { TimeOfDay::Night } else { TimeOfDay::Day };
            }
            A::Camera(mode) => {
                for mut camera in &mut cameras {
                    // Cycling (the camera button) snaps to the new view
                    // instead of sweeping through the bodywork.
                    while camera.mode != mode {
                        camera.cycle();
                    }
                }
                info!("camera {mode:?}");
            }
            A::Steering(mode) => touch_settings.steering = mode,
            A::TouchText(id, label) => {
                let position = texts
                    .iter()
                    .find(|(text, _, visibility)| visibility.get() && text.as_str() == label)
                    .map(|(_, transform, _)| transform.translation / scale);
                if let Some(position) = position {
                    info!("finger {id} on '{label}' at {position}");
                    send(TouchPhase::Started, id, position);
                    script.held.push((id, position));
                    script.resume_at = now + 0.05;
                    script.retrying_since = None;
                } else if now - *script.retrying_since.get_or_insert(now) < RETRY_FOR_SECONDS {
                    // Not on screen yet: try this step again shortly.
                    script.step -= 1;
                    script.resume_at = now + 0.25;
                    return;
                } else {
                    warn!("no visible text '{label}' to touch");
                    script.retrying_since = None;
                }
            }
            A::TouchControl(id, kind) => {
                let position = controls
                    .iter()
                    .find(|(candidate, _)| **candidate == kind)
                    .map(|(_, transform)| transform.translation / scale)
                    // A control that was just shown has no layout yet.
                    .filter(|position| *position != Vec2::ZERO);
                if let Some(position) = position {
                    info!("finger {id} on {kind:?} at {position}");
                    send(TouchPhase::Started, id, position);
                    script.held.push((id, position));
                    script.retrying_since = None;
                } else if now - *script.retrying_since.get_or_insert(now) < RETRY_FOR_SECONDS {
                    script.step -= 1;
                    script.resume_at = now + 0.25;
                    return;
                } else {
                    warn!("no visible {kind:?} control to touch");
                    script.retrying_since = None;
                }
            }
            A::Lift(id) => {
                if let Some(index) = script.held.iter().position(|(held, _)| *held == id) {
                    let (_, position) = script.held.remove(index);
                    send(TouchPhase::Ended, id, position);
                }
            }
        }
    }
}
