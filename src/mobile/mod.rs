//! Phone build of the game: landscape touch layout, on-screen pedals,
//! steering buttons or tilt steering.
//!
//! Everything except [`settings`] is compiled only with the `mobile` cargo
//! feature, which the separate Android package in `android/` enables. The
//! Windows/macOS game never builds this code, so it behaves exactly as before.
//!
//! The mobile layer deliberately does not fork the simulation: touch input is
//! merged into the same [`DriverInput`](crate::vehicle::controller::DriverInput)
//! the keyboard writes, and the menus reuse every existing handler.

pub mod settings;

pub use settings::{SteeringMode, TiltSensitivity, TouchControlSettings};

#[cfg(feature = "mobile")]
mod gestures;
#[cfg(feature = "mobile")]
mod layout;
#[cfg(feature = "mobile")]
mod lifecycle;
#[cfg(feature = "mobile")]
mod tilt;
#[cfg(feature = "mobile")]
mod touch_controls;

#[cfg(all(feature = "mobile", target_os = "android"))]
mod android;

#[cfg(feature = "mobile")]
pub use self::app::{
    MobilePlugin, apply_mobile_graphics_defaults, build_mobile_app, run_mobile_app,
};
#[cfg(feature = "mobile")]
pub use layout::SafeArea;
#[cfg(feature = "mobile")]
pub use tilt::TiltInput;
#[cfg(feature = "mobile")]
pub use touch_controls::{TouchControlKind, TouchControlState};

#[cfg(feature = "mobile")]
mod app {
    use bevy::asset::AssetPlugin;
    use bevy::prelude::*;
    use bevy::window::{PresentMode, WindowResolution};

    use crate::game::GamePlugin;
    use crate::settings::SettingsPersistencePlugin;
    use crate::ui::UiLayout;
    use crate::world::lighting::{AntiAliasingQuality, LightingQuality, LightingSettings};
    use crate::world::traffic::{TrafficDensity, TrafficSettings};

    use super::{gestures, layout, lifecycle, tilt, touch_controls};

    /// Window used when the phone interface is previewed on a desktop
    /// (`cargo run --release --features mobile --example mobile_preview`): a
    /// 19.5:9 phone held sideways, at a comfortable size. Android ignores it
    /// and always uses the full screen.
    const PREVIEW_WINDOW: (u32, u32) = (975, 450);

    /// Build and run the complete game with the phone interface.
    ///
    /// This is what the Android entry point (`android/src/lib.rs`) calls.
    pub fn run_mobile_app() {
        let exit = build_mobile_app().run();
        info!("OnlyGAZ finished: {exit:?}");
        // Android keeps the process alive after the activity finishes and
        // would start `android_main` again inside it on the next launch,
        // which a Bevy app cannot survive. Settings are already saved.
        #[cfg(target_os = "android")]
        std::process::exit(0);
    }

    /// The complete game with the phone interface, ready to run.
    pub fn build_mobile_app() -> App {
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: crate::asset_paths::asset_directory()
                        .to_string_lossy()
                        .into_owned(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "OnlyGAZ".into(),
                        resolution: WindowResolution::new(PREVIEW_WINDOW.0, PREVIEW_WINDOW.1),
                        present_mode: PresentMode::AutoVsync,
                        ..default()
                    }),
                    ..default()
                })
                .set(ImagePlugin::default_linear()),
        )
        // Must precede the game plugins: it inserts `UiLayout::Phone` before
        // their start-up systems build the HUD and menus.
        .add_plugins(MobilePlugin)
        .add_plugins(SettingsPersistencePlugin)
        .add_plugins(GamePlugin);
        app
    }

    /// First-launch settings for phones: the Performance preset (without
    /// MSAA, see [`crate::world::lighting::MSAA_2X_UNAVAILABLE`]) and light
    /// traffic. Every value remains adjustable in the graphics menu.
    pub fn apply_mobile_graphics_defaults(
        graphics: &mut LightingSettings,
        traffic: &mut TrafficSettings,
    ) {
        graphics.apply_preset(LightingQuality::Performance);
        traffic.density = TrafficDensity::Low;
    }

    /// Touch controls, tilt steering, phone layout and Android lifecycle.
    pub struct MobilePlugin;

    impl Plugin for MobilePlugin {
        fn build(&self, app: &mut App) {
            crate::world::lighting::MSAA_2X_UNAVAILABLE
                .store(true, std::sync::atomic::Ordering::Relaxed);
            app.insert_resource(UiLayout::Phone)
                .init_resource::<crate::mobile::TouchControlSettings>()
                .add_plugins((
                    layout::LayoutPlugin,
                    gestures::MenuGesturePlugin,
                    touch_controls::TouchControlsPlugin,
                    tilt::TiltPlugin,
                    lifecycle::LifecyclePlugin,
                ))
                .add_systems(PreUpdate, avoid_2x_msaa);
        }
    }

    /// The Performance preset asks for 2x MSAA, which many phone GPUs do not
    /// support (see [`crate::world::lighting::MSAA_2X_UNAVAILABLE`]); show
    /// and use "off" instead.
    fn avoid_2x_msaa(mut graphics: ResMut<LightingSettings>) {
        if graphics.anti_aliasing == AntiAliasingQuality::Msaa2 {
            graphics.anti_aliasing = AntiAliasingQuality::Off;
        }
    }
}
