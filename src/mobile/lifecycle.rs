//! Phone-specific application behaviour: the system Back button, going to the
//! background, and keeping the interface clear of camera cut-outs.

use bevy::audio::{AudioSink, AudioSinkPlayback, SpatialAudioSink};
use bevy::input::InputSystems;
use bevy::input::keyboard::{Key, NativeKeyCode};
use bevy::prelude::*;
use bevy::window::AppLifecycle;

use crate::game::GameState;
use crate::ui::menu::{PauseMenuPage, PauseOverlay, SettingSlider, sync_pause_only_actions};
use crate::ui::speedometer::HudRoot;

use super::layout::SafeArea;

/// Android's `KEYCODE_BACK`, delivered by winit as an unidentified key.
const ANDROID_KEYCODE_BACK: u32 = 4;

pub struct LifecyclePlugin;

impl Plugin for LifecyclePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SuspendedAudio>()
            .add_systems(PreUpdate, map_back_button.after(InputSystems))
            .add_systems(PostStartup, enlarge_sliders_for_fingers)
            .add_systems(
                Update,
                (
                    sync_pause_only_actions,
                    keep_ui_inside_safe_area,
                    show_hud_only_while_driving,
                ),
            )
            // After every audio system has decided what should be playing.
            .add_systems(PostUpdate, pause_audio_in_background);
    }
}

/// The Back button / gesture behaves like `Esc` on a keyboard: it pauses a
/// race, closes a settings page and leaves the mode selection. On the main
/// menu itself it closes the game, as Android users expect.
fn map_back_button(
    logical: Res<ButtonInput<Key>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    state: Res<State<GameState>>,
    mut page: ResMut<PauseMenuPage>,
    mut exit: MessageWriter<AppExit>,
    mut escape_held: Local<bool>,
) {
    if *escape_held {
        keys.release(KeyCode::Escape);
        *escape_held = false;
    }
    let back = logical.just_pressed(Key::BrowserBack)
        || keys.just_pressed(KeyCode::Unidentified(NativeKeyCode::Android(
            ANDROID_KEYCODE_BACK,
        )));
    if !back {
        return;
    }
    if *state.get() == GameState::MainMenu {
        // The desktop main menu has no keyboard "back" for its sub-pages.
        if *page != PauseMenuPage::Main {
            *page = PauseMenuPage::Main;
        } else {
            exit.write(AppExit::Success);
        }
        return;
    }
    keys.press(KeyCode::Escape);
    *escape_held = true;
}

/// Sinks paused because the app went to the background.
#[derive(Resource, Default)]
struct SuspendedAudio {
    suspended: bool,
    paused: Vec<Entity>,
}

/// Android keeps the audio device running while the activity is in the
/// background, but stops updating the game. Pause every playing sound (and the
/// race) before that happens, and resume exactly those sounds afterwards.
fn pause_audio_in_background(
    mut lifecycle: MessageReader<AppLifecycle>,
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
    mut audio: ResMut<SuspendedAudio>,
    sinks: Query<(Entity, &AudioSink)>,
    spatial: Query<(Entity, &SpatialAudioSink)>,
) {
    for event in lifecycle.read() {
        match event {
            AppLifecycle::WillSuspend | AppLifecycle::Suspended => {
                if audio.suspended {
                    continue;
                }
                audio.suspended = true;
                if *state.get() == GameState::Driving {
                    next.set(GameState::Paused);
                }
                for (entity, sink) in &sinks {
                    if !sink.is_paused() {
                        sink.pause();
                        audio.paused.push(entity);
                    }
                }
                for (entity, sink) in &spatial {
                    if !sink.is_paused() {
                        sink.pause();
                        audio.paused.push(entity);
                    }
                }
                info!(
                    "app in background: race paused, {} sounds paused",
                    audio.paused.len()
                );
            }
            AppLifecycle::WillResume | AppLifecycle::Running => {
                if !audio.suspended {
                    continue;
                }
                audio.suspended = false;
                for entity in audio.paused.drain(..) {
                    if let Ok((_, sink)) = sinks.get(entity) {
                        sink.play();
                    } else if let Ok((_, sink)) = spatial.get(entity) {
                        sink.play();
                    }
                }
                info!("app resumed");
            }
            AppLifecycle::Idle => {}
        }
    }
}

/// Keep the HUD and menus clear of display cut-outs reported by Android.
fn keep_ui_inside_safe_area(
    safe_area: Res<SafeArea>,
    ui_scale: Res<UiScale>,
    mut hud: Query<&mut Node, (With<HudRoot>, Without<PauseOverlay>)>,
    mut overlays: Query<&mut Node, (With<PauseOverlay>, Without<HudRoot>)>,
) {
    if !safe_area.is_changed() && !ui_scale.is_changed() {
        return;
    }
    let safe = safe_area.in_ui_pixels(ui_scale.0);
    for mut node in &mut hud {
        node.top = px(12.0 + safe.top);
        node.left = px(12.0 + safe.left);
    }
    for mut node in &mut overlays {
        node.padding = UiRect {
            left: px(10.0 + safe.left),
            right: px(10.0 + safe.right),
            top: px(10.0 + safe.top),
            bottom: px(10.0 + safe.bottom),
        };
    }
}

/// On a phone the speed panel sits at the top of the screen, exactly where the
/// menus start; it is only needed during a race.
fn show_hud_only_while_driving(
    state: Res<State<GameState>>,
    mut hud: Query<&mut Visibility, With<HudRoot>>,
) {
    let wanted = if *state.get() == GameState::Driving {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut hud {
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}

/// The desktop volume sliders are 14 px tall bars; make them finger-sized.
fn enlarge_sliders_for_fingers(mut sliders: Query<&mut Node, With<SettingSlider>>) {
    for mut node in &mut sliders {
        node.height = px(30);
    }
}
