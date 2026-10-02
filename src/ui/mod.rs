//! User interface: the driving HUD, the telemetry overlay, the physics gizmos
//! and the pause menu.

pub mod benchmark;
pub mod debug;
pub mod gizmos;
pub mod menu;
mod navigation;
pub mod speedometer;
pub mod start_screen;

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::prelude::*;
use bevy_resvg::prelude::SvgPlugin;

use crate::game::GameState;
use crate::vehicle::controller::{ControlAction, KeyBindings};

/// Which interface layout the game builds.
///
/// The desktop game never inserts this resource and keeps its monitor-sized
/// menus. The phone build (`mobile` feature) inserts [`UiLayout::Phone`]
/// before start-up, which switches the HUD and menus to their compact,
/// touch-friendly variants for a landscape phone screen.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiLayout {
    #[default]
    Desktop,
    Phone,
}

/// Run condition: the landscape desktop interface is active.
pub fn desktop_layout(layout: Option<Res<UiLayout>>) -> bool {
    layout.is_none_or(|layout| *layout == UiLayout::Desktop)
}

/// Run condition: the phone interface is active.
pub fn phone_layout(layout: Option<Res<UiLayout>>) -> bool {
    !desktop_layout(layout)
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((FrameTimeDiagnosticsPlugin::default(), SvgPlugin))
            // The overlays and their keys belong to the interface, so the
            // simulation can be built without them.
            .init_resource::<KeyBindings>()
            .init_resource::<debug::DebugOverlay>()
            .init_resource::<gizmos::PhysicsGizmoOverlay>()
            .init_resource::<benchmark::FpsBenchmark>()
            .init_resource::<menu::PauseMenuPage>()
            .init_resource::<menu::DisplaySettings>()
            .init_resource::<crate::audio::AudioSettings>()
            .init_resource::<menu::RebindState>()
            .init_resource::<navigation::KeyboardMenuNavigation>()
            .add_systems(
                Startup,
                (
                    speedometer::spawn_hud,
                    debug::spawn_debug_overlay,
                    menu::spawn_menu,
                    start_screen::spawn_start_screen,
                ),
            )
            .add_systems(
                Update,
                (
                    navigation::keyboard_menu_navigation,
                    start_screen::handle_start_screen,
                    start_screen::sync_start_screen,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    toggle_overlays,
                    speedometer::update_hud,
                    debug::toggle_debug_overlay,
                    debug::update_debug_overlay,
                    benchmark::measure_fps,
                    menu::handle_menu_buttons,
                    menu::handle_graphics_quality_buttons,
                    menu::handle_setting_buttons,
                    menu::handle_setting_sliders,
                    menu::handle_rebind_buttons,
                    menu::handle_radio_buttons,
                    menu::apply_rebind_key,
                    // Window mode and resolution belong to desktop monitors;
                    // a phone always runs full screen at its native size.
                    menu::apply_display_settings.run_if(desktop_layout),
                    menu::sync_button_hover,
                    menu::sync_graphics_menu,
                    menu::sync_controls_menu,
                    menu::sync_radio_menu,
                    menu::sync_menu_visibility,
                    menu::scroll_graphics,
                )
                    .chain()
                    .after(navigation::keyboard_menu_navigation),
            )
            .add_systems(
                Update,
                menu::sync_main_menu_copy.after(menu::sync_menu_visibility),
            )
            .add_systems(
                Update,
                navigation::sync_menu_highlights
                    .after(menu::sync_button_hover)
                    .after(menu::sync_graphics_menu)
                    .after(menu::sync_controls_menu)
                    .after(start_screen::sync_start_screen),
            )
            .add_systems(
                PostUpdate,
                gizmos::draw_physics_gizmos.run_if(in_state(GameState::Driving)),
            );
    }
}

/// `F3` and `F4`.
fn toggle_overlays(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<KeyBindings>,
    mut debug_overlay: ResMut<debug::DebugOverlay>,
    mut gizmo_overlay: ResMut<gizmos::PhysicsGizmoOverlay>,
    rebind: Option<Res<menu::RebindState>>,
) {
    if rebind.as_ref().is_some_and(|state| state.pending.is_some()) {
        return;
    }
    if bindings.just_pressed(&keys, ControlAction::ToggleDebug) {
        debug_overlay.visible = !debug_overlay.visible;
    }
    if bindings.just_pressed(&keys, ControlAction::TogglePhysicsGizmos) {
        gizmo_overlay.visible = !gizmo_overlay.visible;
    }
}
