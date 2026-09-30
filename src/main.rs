#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

//! Entry point. Everything of substance lives in the library crate; this file
//! only builds the Bevy application.

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::window::{MonitorSelection, PresentMode, WindowMode, WindowResolution};

use veyron_sim::asset_paths::asset_directory;
use veyron_sim::game::GamePlugin;
use veyron_sim::settings::SettingsPersistencePlugin;

/// Visible marker for the locally assembled release. It makes it immediately
/// clear that Finder or Cargo did not open an older copy of the simulator.
const BUILD_LABEL: &str = "STORE RELEASE 2026-09-30";

fn main() -> AppExit {
    println!("OnlyGAZ build: {BUILD_LABEL}");
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: asset_directory().to_string_lossy().into_owned(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: format!("OnlyGAZ - Main Menu [{BUILD_LABEL}]"),
                        resolution: WindowResolution::new(1920, 1080),
                        mode: WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
                        present_mode: PresentMode::AutoVsync,
                        ..default()
                    }),
                    ..default()
                })
                .set(ImagePlugin::default_linear()),
        )
        .add_plugins(SettingsPersistencePlugin)
        .add_plugins(GamePlugin)
        .run()
}
