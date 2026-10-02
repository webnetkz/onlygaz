//! Scripted screenshots of the phone interface, for checking the landscape
//! touch layout and the touch controls without a phone.
//!
//! ```sh
//! cargo run --release --features mobile --example mobile_screenshots -- out_dir
//! ```
//!
//! Opens the phone-shaped 975×450 preview window and drives it with synthetic
//! input only: taps menu buttons found by their labels (they activate when
//! the finger lifts), drags the graphics list, starts a race, holds GAS and
//! the right arrow, switches to tilt steering (simulated 12° roll), taps the
//! pause button, leaves the race and uses the Back button, saving a PNG at
//! every step. Works with a software Vulkan driver (Mesa lavapipe under Xvfb),
//! just slowly.

use std::path::PathBuf;

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput, NativeKeyCode};
use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::PrimaryWindow;

use veyron_sim::mobile::{SteeringMode, TouchControlKind, TouchControlSettings};

/// Simulated phone roll for the tilt-steering part of the script [degrees].
const FAKE_ROLL_DEGREES: &str = "12";

#[derive(Debug, Clone, Copy)]
enum Action {
    Wait(f64),
    Shot(&'static str),
    Steering(SteeringMode),
    /// Finger `id` lands on the visible text label (e.g. a button title).
    TouchText(u64, &'static str),
    /// Finger `id` lands at the right end of the settings row with this
    /// title: its "›" selector or its ON/OFF switch.
    TouchRowEnd(u64, &'static str),
    /// Finger `id` slides vertically by this many logical pixels.
    MoveBy(u64, f32),
    /// Finger `id` lands on the centre of an on-screen driving control.
    TouchControl(u64, TouchControlKind),
    Lift(u64),
    /// The Android Back button (as winit reports it on a phone).
    Back,
}

use Action as A;

const SCRIPT: &[Action] = &[
    A::Wait(10.0),
    A::Shot("01_main_menu"),
    A::TouchText(10, "CONTROLS"),
    A::Wait(0.2),
    A::Lift(10),
    A::Wait(1.5),
    A::Shot("02_controls"),
    // Steering: BUTTONS -> TILT.
    A::TouchRowEnd(11, "Steering"),
    A::Wait(0.2),
    A::Lift(11),
    A::Wait(1.5),
    A::Shot("02b_controls_tilt_selected"),
    A::TouchText(12, "BACK"),
    A::Wait(0.2),
    A::Lift(12),
    A::Wait(1.0),
    A::TouchText(13, "GRAPHICS"),
    A::Wait(0.2),
    A::Lift(13),
    A::Wait(1.5),
    // Contact shadows: OFF -> ON.
    A::TouchRowEnd(14, "Contact shadows"),
    A::Wait(0.2),
    A::Lift(14),
    A::Wait(1.0),
    A::Shot("03_graphics_tapped_toggle"),
    // Drag the list upwards starting on a selector: it must scroll, not
    // change the setting.
    A::TouchRowEnd(15, "Traffic"),
    A::Wait(0.1),
    A::MoveBy(15, -60.0),
    A::Wait(0.1),
    A::MoveBy(15, -80.0),
    A::Wait(0.1),
    A::MoveBy(15, -80.0),
    A::Wait(0.2),
    A::Lift(15),
    A::Wait(1.0),
    A::Shot("03b_graphics_dragged"),
    A::TouchText(16, "BACK"),
    A::Wait(0.2),
    A::Lift(16),
    A::Wait(1.0),
    A::TouchText(17, "AUDIO"),
    A::Wait(0.2),
    A::Lift(17),
    A::Wait(1.5),
    A::Shot("04_audio"),
    A::TouchText(18, "BACK"),
    A::Wait(0.2),
    A::Lift(18),
    A::Wait(1.0),
    A::TouchText(19, "PLAY"),
    A::Wait(0.2),
    A::Lift(19),
    A::Wait(1.5),
    A::Shot("05_mode_selection"),
    A::TouchText(20, "DRIVE WITHOUT DAMAGE"),
    A::Wait(0.2),
    A::Lift(20),
    A::Wait(6.0),
    A::Steering(SteeringMode::Buttons),
    A::Wait(2.5),
    A::TouchControl(1, TouchControlKind::Gas),
    A::TouchControl(2, TouchControlKind::SteerRight),
    A::Wait(4.0),
    A::Shot("06_driving_buttons"),
    A::Lift(1),
    A::Lift(2),
    A::Steering(SteeringMode::Tilt),
    A::Wait(2.0),
    A::TouchControl(3, TouchControlKind::Gas),
    A::TouchControl(4, TouchControlKind::Nitro),
    A::Wait(3.0),
    A::Shot("07_driving_tilt_mode"),
    A::Lift(3),
    A::Lift(4),
    A::TouchControl(5, TouchControlKind::Menu),
    A::Wait(0.3),
    A::Lift(5),
    A::Wait(2.5),
    A::Shot("08_pause_menu"),
    A::TouchText(21, "EXIT TO MAIN MENU"),
    A::Wait(0.2),
    A::Lift(21),
    A::Wait(2.5),
    A::Shot("09_exit_to_main_menu"),
    // CONTROLS, then the Back button returns to the main page...
    A::TouchText(22, "CONTROLS"),
    A::Wait(0.2),
    A::Lift(22),
    A::Wait(1.5),
    A::Back,
    A::Wait(2.0),
    A::Shot("10_back_closed_page"),
    A::Wait(3.0),
    // ...and Back on the main page closes the game.
    A::Back,
    A::Wait(5.0),
];

#[derive(Resource)]
struct Script {
    out: PathBuf,
    step: usize,
    resume_at: f64,
    held: Vec<(u64, Vec2)>,
    /// When the current step first failed to find its target (UI updates lag
    /// a frame or two behind, and software rendering is slow).
    retrying_since: Option<f64>,
}

/// How long a step waits for its button or control to appear.
const RETRY_FOR_SECONDS: f64 = 15.0;

fn main() {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "mobile_screenshots".to_string()),
    );
    std::fs::create_dir_all(&out).expect("output directory");
    let _ = std::fs::remove_file(out.join("settings.ron"));
    // SAFETY: single-threaded at this point, before Bevy starts any thread.
    unsafe {
        // Do not touch the player's real settings file.
        std::env::set_var("VEYRON_SETTINGS_PATH", out.join("settings.ron"));
        std::env::set_var("ONLYGAZ_FAKE_TILT_DEGREES", FAKE_ROLL_DEGREES);
    }
    let mut app = veyron_sim::mobile::build_mobile_app();
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

fn control_centre(
    kind: TouchControlKind,
    controls: &Query<(&TouchControlKind, &ComputedNode, &UiGlobalTransform)>,
    scale_factor: f32,
) -> Option<Vec2> {
    controls
        .iter()
        .find(|(candidate, ..)| **candidate == kind)
        .map(|(_, _, transform)| transform.translation / scale_factor)
}

#[allow(clippy::too_many_arguments)]
fn run_script(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut script: ResMut<Script>,
    mut touch_settings: ResMut<TouchControlSettings>,
    mut touches: MessageWriter<TouchInput>,
    mut keys: MessageWriter<KeyboardInput>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    controls: Query<(&TouchControlKind, &ComputedNode, &UiGlobalTransform)>,
    texts: Query<(
        &Text,
        &UiGlobalTransform,
        &InheritedVisibility,
        Option<&ChildOf>,
    )>,
    nodes: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut exit: MessageWriter<AppExit>,
) {
    let now = time.elapsed_secs_f64();
    let Ok((window, window_info)) = windows.single() else {
        return;
    };
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
            info!("screenshot script finished");
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
                script.resume_at = now + 0.6;
            }
            A::Steering(mode) => touch_settings.steering = mode,
            A::TouchText(id, label) | A::TouchRowEnd(id, label) => {
                let scale = window_info.scale_factor();
                let found = texts
                    .iter()
                    .find(|(text, _, visibility, _)| visibility.get() && text.as_str() == label);
                let position = found.and_then(|(_, transform, _, parent)| {
                    if matches!(action, A::TouchText(..)) {
                        return Some(transform.translation / scale);
                    }
                    // The row is the label's parent: aim 22 px inside its right edge.
                    let (row, row_transform) = nodes.get(parent?.parent()).ok()?;
                    let right = row_transform.translation.x + row.size().x * 0.5;
                    Some(Vec2::new(right - 22.0 * scale, row_transform.translation.y) / scale)
                });
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
            A::MoveBy(id, dy) => {
                if let Some(held) = script.held.iter_mut().find(|(held, _)| *held == id) {
                    held.1.y += dy;
                    send(TouchPhase::Moved, id, held.1);
                }
            }
            A::TouchControl(id, kind) => {
                let position = control_centre(kind, &controls, window_info.scale_factor())
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
            A::Back => {
                info!("Back button");
                for state in [ButtonState::Pressed, ButtonState::Released] {
                    keys.write(KeyboardInput {
                        key_code: KeyCode::Unidentified(NativeKeyCode::Android(4)),
                        logical_key: Key::BrowserBack,
                        state,
                        text: None,
                        repeat: false,
                        window,
                    });
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
