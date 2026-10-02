//! Interface scale and safe-area handling for phone screens.
//!
//! The phone menus and HUD are laid out on a virtual landscape canvas of at
//! least [`DESIGN_WIDTH`] × [`DESIGN_HEIGHT`] UI pixels. [`UiScale`] maps that
//! canvas onto the physical screen, so the same layout reads the same on a
//! 640×360-dp budget phone, a 20:9 flagship and a tablet.

use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowResized};

/// Minimum width of the virtual landscape canvas, in UI pixels.
pub const DESIGN_WIDTH: f32 = 860.0;
/// Minimum height of the virtual landscape canvas, in UI pixels.
pub const DESIGN_HEIGHT: f32 = 480.0;

/// Areas at the screen edges that must stay free of interactive UI: display
/// cut-outs (camera holes, notches) and, if they are shown, system bars.
/// Values are logical pixels of the window.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct SafeArea {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

impl SafeArea {
    /// The same insets expressed in UI pixels (before [`UiScale`]).
    pub fn in_ui_pixels(&self, ui_scale: f32) -> Self {
        let scale = ui_scale.max(0.01);
        Self {
            top: self.top / scale,
            bottom: self.bottom / scale,
            left: self.left / scale,
            right: self.right / scale,
        }
    }
}

pub struct LayoutPlugin;

impl Plugin for LayoutPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SafeArea>()
            .add_systems(Startup, fit_ui_scale_to_window)
            .add_systems(PreUpdate, fit_ui_scale_to_window);
        #[cfg(target_os = "android")]
        app.add_systems(PreUpdate, super::android::poll_display_info);
    }
}

/// UI scale for a window of `width` × `height` logical pixels.
pub fn ui_scale_for(width: f32, height: f32) -> f32 {
    if width <= 0.0 || height <= 0.0 {
        return 1.0;
    }
    (width / DESIGN_WIDTH)
        .min(height / DESIGN_HEIGHT)
        .clamp(0.5, 3.0)
}

fn fit_ui_scale_to_window(
    mut resized: MessageReader<WindowResized>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut ui_scale: ResMut<UiScale>,
    mut initialised: Local<bool>,
) {
    let resized = resized.read().count() > 0;
    if *initialised && !resized {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let scale = ui_scale_for(window.width(), window.height());
    if (ui_scale.0 - scale).abs() > 1.0e-3 {
        ui_scale.0 = scale;
        info!(
            "phone interface scale {scale:.3} for a {:.0}x{:.0} logical window",
            window.width(),
            window.height()
        );
    }
    *initialised = window.width() > 0.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_phones_get_a_readable_scale() {
        // 1080x2400 at 2.625 dpr, held sideways.
        let modern = ui_scale_for(914.0, 411.0);
        assert!((0.8..0.9).contains(&modern), "{modern}");
        // Old 16:9 budget phone.
        let budget = ui_scale_for(640.0, 360.0);
        assert!((0.7..0.8).contains(&budget), "{budget}");
        // A tablet grows the controls.
        assert!(ui_scale_for(1280.0, 800.0) > 1.4);
        // The canvas always keeps at least the design size.
        for (width, height) in [(914.0, 411.0), (640.0, 360.0), (1280.0, 800.0)] {
            let scale = ui_scale_for(width, height);
            assert!(width / scale >= DESIGN_WIDTH - 0.5);
            assert!(height / scale >= DESIGN_HEIGHT - 0.5);
        }
    }

    #[test]
    fn safe_area_converts_to_ui_pixels() {
        let area = SafeArea {
            top: 0.0,
            bottom: 0.0,
            left: 40.0,
            right: 20.0,
        };
        let ui = area.in_ui_pixels(0.8);
        assert!((ui.left - 50.0).abs() < 1.0e-4);
        assert!((ui.right - 25.0).abs() < 1.0e-4);
    }
}
