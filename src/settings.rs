//! Persistent user settings for the desktop game.

use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use serde::{Deserialize, Serialize};

use crate::audio::{AudioSettings, engine::EngineAudioSettings};
use crate::ui::menu::DisplaySettings;
use crate::vehicle::controller::KeyBindings;
#[cfg(not(feature = "mobile"))]
use crate::world::lighting::LightingQuality;
use crate::world::lighting::LightingSettings;
use crate::world::traffic::TrafficSettings;

mod auto_graphics;

/// 5: the phone build added the Lite preset and render resolution; phone
/// settings from older versions are configured again for the hardware.
const SETTINGS_VERSION: u32 = 5;
const SETTINGS_PATH_ENV: &str = "VEYRON_SETTINGS_PATH";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct SavedSettings {
    version: u32,
    display: DisplaySettings,
    graphics: LightingSettings,
    traffic: TrafficSettings,
    audio: AudioSettings,
    controls: KeyBindings,
    engine_audio_enabled: bool,
    auto_graphics_configured: bool,
    /// Phone build only: steering mode and tilt calibration.
    #[cfg(feature = "mobile")]
    touch: crate::mobile::TouchControlSettings,
}

impl Default for SavedSettings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            display: DisplaySettings::default(),
            graphics: LightingSettings::default(),
            traffic: TrafficSettings::default(),
            audio: AudioSettings::default(),
            controls: KeyBindings::default(),
            engine_audio_enabled: true,
            auto_graphics_configured: false,
            #[cfg(feature = "mobile")]
            touch: crate::mobile::TouchControlSettings::default(),
        }
    }
}

impl SavedSettings {
    #[cfg_attr(feature = "mobile", allow(clippy::too_many_arguments))]
    fn capture(
        display: &DisplaySettings,
        graphics: &LightingSettings,
        traffic: &TrafficSettings,
        audio: &AudioSettings,
        controls: &KeyBindings,
        engine_audio: &EngineAudioSettings,
        auto_graphics_configured: bool,
        #[cfg(feature = "mobile")] touch: &crate::mobile::TouchControlSettings,
    ) -> Self {
        Self {
            version: SETTINGS_VERSION,
            display: display.clone(),
            graphics: graphics.clone(),
            traffic: traffic.clone(),
            audio: audio.clone(),
            controls: controls.clone(),
            engine_audio_enabled: engine_audio.enabled,
            auto_graphics_configured,
            #[cfg(feature = "mobile")]
            touch: touch.clone(),
        }
    }
}

#[derive(Resource)]
struct SettingsFile {
    path: PathBuf,
    auto_graphics_configured: bool,
}

/// Loads settings before the simulation plugins read their initial resources,
/// then writes every in-game change back to the same file.
pub struct SettingsPersistencePlugin;

impl Plugin for SettingsPersistencePlugin {
    fn build(&self, app: &mut App) {
        let path = settings_file_path();
        let mut auto_graphics_configured = false;
        if let Some(mut saved) = read_settings(&path) {
            if saved.version < 4 {
                saved.controls.migrate_default_arrows();
            }
            let mut engine_audio = EngineAudioSettings::default();
            engine_audio.enabled = saved.engine_audio_enabled;
            // Any settings file from before automatic detection represents an
            // existing installation. Preserve its hand-picked graphics instead
            // of treating the next update as the first launch.
            auto_graphics_configured = saved.auto_graphics_configured || saved.version < 3;
            // Phones updated from 1.0.4 or older started from a preset that
            // was too heavy for budget hardware: choose again, once.
            #[cfg(feature = "mobile")]
            if saved.version < 5 {
                auto_graphics_configured = false;
            }
            #[cfg(feature = "mobile")]
            app.insert_resource(saved.touch);
            app.insert_resource(saved.display)
                .insert_resource(saved.graphics)
                .insert_resource(saved.traffic)
                .insert_resource(saved.audio)
                .insert_resource(saved.controls)
                .insert_resource(engine_audio);
            info!("settings loaded from {}", path.display());
        }
        #[cfg(feature = "mobile")]
        app.init_resource::<crate::mobile::TouchControlSettings>();
        app.insert_resource(SettingsFile {
            path,
            auto_graphics_configured,
        })
        .add_systems(PreStartup, auto_configure_graphics_on_first_launch)
        .add_systems(Last, save_changed_settings);
    }
}

fn auto_configure_graphics_on_first_launch(
    mut file: ResMut<SettingsFile>,
    adapter: Option<Res<RenderAdapterInfo>>,
    mut graphics: ResMut<LightingSettings>,
    #[cfg(feature = "mobile")] mut traffic: ResMut<TrafficSettings>,
) {
    if file.auto_graphics_configured {
        return;
    }

    // Phones share their power budget with the display and heat up quickly:
    // only the two lightest presets are ever chosen automatically.
    #[cfg(feature = "mobile")]
    {
        let profile = auto_graphics::HardwareProfile::detect(adapter.as_deref());
        let quality = if std::env::var_os(crate::world::lighting::QUALITY_ENV).is_some() {
            crate::world::lighting::LightingQuality::from_env()
        } else {
            auto_graphics::recommended_mobile_quality(&profile)
        };
        crate::mobile::apply_mobile_graphics_defaults(&mut graphics, &mut traffic, quality);
        file.auto_graphics_configured = true;
        info!(
            "first-launch graphics auto-configuration: phone {} ({})",
            quality.label(),
            profile.description(),
        );
    }

    #[cfg(not(feature = "mobile"))]
    {
        let profile = auto_graphics::HardwareProfile::detect(adapter.as_deref());
        let quality = if std::env::var_os(crate::world::lighting::QUALITY_ENV).is_some() {
            LightingQuality::from_env()
        } else {
            auto_graphics::recommended_quality(&profile)
        };
        graphics.apply_preset(quality);
        file.auto_graphics_configured = true;
        info!(
            "first-launch graphics auto-configuration: {} ({})",
            quality.label(),
            profile.description(),
        );
    }
}

fn settings_file_path() -> PathBuf {
    if let Some(path) = std::env::var_os(SETTINGS_PATH_ENV) {
        return PathBuf::from(path);
    }
    default_settings_file_path()
}

#[cfg(target_os = "macos")]
fn default_settings_file_path() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join("Library/Application Support/Veyron Sim/settings.ron");
    }
    PathBuf::from("veyron_settings.ron")
}

#[cfg(target_os = "windows")]
fn default_settings_file_path() -> PathBuf {
    windows_settings_file_path(std::env::var_os("APPDATA"))
}

/// Android keeps per-app files in the private internal storage directory,
/// which survives updates of the APK and is removed with the app.
#[cfg(target_os = "android")]
fn default_settings_file_path() -> PathBuf {
    bevy::android::ANDROID_APP
        .get()
        .and_then(|app| app.internal_data_path())
        .map(|directory| directory.join("settings.ron"))
        .unwrap_or_else(|| PathBuf::from("settings.ron"))
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "android")))]
fn default_settings_file_path() -> PathBuf {
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config).join("veyron_sim/settings.ron");
    }
    PathBuf::from("veyron_settings.ron")
}

/// Keep the Windows path construction independently testable on non-Windows
/// build hosts. An unpacked portable build may live in a protected directory,
/// while the roaming application-data directory is writable for the current
/// user and survives replacing the game folder with a newer test build.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn windows_settings_file_path(app_data: Option<std::ffi::OsString>) -> PathBuf {
    app_data
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Veyron Sim")
        .join("settings.ron")
}

fn read_settings(path: &Path) -> Option<SavedSettings> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            warn!("could not read settings from {}: {error}", path.display());
            return None;
        }
    };
    match ron::from_str(&text) {
        Ok(saved) => Some(saved),
        Err(error) => {
            warn!("could not parse settings from {}: {error}", path.display());
            None
        }
    }
}

#[cfg_attr(feature = "mobile", allow(clippy::too_many_arguments))]
fn save_changed_settings(
    file: Res<SettingsFile>,
    display: Res<DisplaySettings>,
    graphics: Res<LightingSettings>,
    traffic: Res<TrafficSettings>,
    audio: Res<AudioSettings>,
    controls: Res<KeyBindings>,
    engine_audio: Res<EngineAudioSettings>,
    #[cfg(feature = "mobile")] touch: Res<crate::mobile::TouchControlSettings>,
) {
    #[cfg(feature = "mobile")]
    let touch_changed = touch.is_changed();
    #[cfg(not(feature = "mobile"))]
    let touch_changed = false;
    if !display.is_changed()
        && !graphics.is_changed()
        && !traffic.is_changed()
        && !audio.is_changed()
        && !controls.is_changed()
        && !engine_audio.is_changed()
        && !touch_changed
    {
        return;
    }

    let saved = SavedSettings::capture(
        &display,
        &graphics,
        &traffic,
        &audio,
        &controls,
        &engine_audio,
        file.auto_graphics_configured,
        #[cfg(feature = "mobile")]
        &touch,
    );
    let pretty = ron::ser::PrettyConfig::new()
        .struct_names(true)
        .enumerate_arrays(true);
    let text = match ron::ser::to_string_pretty(&saved, pretty) {
        Ok(text) => text,
        Err(error) => {
            error!("could not serialize settings: {error}");
            return;
        }
    };
    if let Some(parent) = file.path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        error!(
            "could not create settings directory {}: {error}",
            parent.display()
        );
        return;
    }
    if let Err(error) = fs::write(&file.path, text) {
        error!(
            "could not save settings to {}: {error}",
            file.path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::menu::{DisplayModeSetting, ResolutionPreset};
    use crate::vehicle::controller::{ControlAction, KeySlot};
    use crate::world::lighting::{GraphicsLevel, LightingQuality};
    use crate::world::traffic::TrafficDensity;

    #[test]
    fn saved_settings_round_trip_every_user_category() {
        let mut saved = SavedSettings::default();
        saved.display.mode = DisplayModeSetting::Windowed;
        saved.display.resolution = ResolutionPreset::Qhd;
        saved.display.vsync = false;
        saved.graphics.apply_preset(LightingQuality::Cinematic);
        saved.graphics.speed_effect = GraphicsLevel::Low;
        saved.graphics.collision_effect = false;
        saved.graphics.rearview_mirrors = false;
        saved.graphics.tire_marks = false;
        saved.graphics.tire_smoke = false;
        saved.traffic.density = TrafficDensity::High;
        saved.audio.effects_volume = 0.4;
        saved.audio.engine_volume = 0.6;
        saved.audio.music_volume = 0.8;
        saved.controls.set(
            ControlAction::Nitro,
            KeySlot::Alternate,
            Some(KeyCode::KeyN),
        );
        saved.controls.set(
            ControlAction::NextRadioTrack,
            KeySlot::Primary,
            Some(KeyCode::KeyP),
        );
        saved.engine_audio_enabled = false;
        saved.auto_graphics_configured = true;

        let text = ron::to_string(&saved).unwrap();
        let loaded: SavedSettings = ron::from_str(&text).unwrap();

        assert_eq!(loaded.display.mode, DisplayModeSetting::Windowed);
        assert_eq!(loaded.display.resolution, ResolutionPreset::Qhd);
        assert!(!loaded.display.vsync);
        assert_eq!(loaded.graphics.quality, LightingQuality::Cinematic);
        assert_eq!(loaded.graphics.speed_effect, GraphicsLevel::Low);
        assert!(!loaded.graphics.collision_effect);
        assert!(!loaded.graphics.rearview_mirrors);
        assert!(!loaded.graphics.tire_marks);
        assert!(!loaded.graphics.tire_smoke);
        assert_eq!(loaded.traffic.density, TrafficDensity::High);
        assert!((loaded.audio.effects_volume - 0.4).abs() < f32::EPSILON);
        assert_eq!(
            loaded.controls.alternate(ControlAction::Nitro),
            Some(KeyCode::KeyN)
        );
        assert_eq!(
            loaded.controls.primary(ControlAction::NextRadioTrack),
            KeyCode::KeyP
        );
        assert!(!loaded.engine_audio_enabled);
        assert!(loaded.auto_graphics_configured);
    }

    #[test]
    fn older_settings_remain_compatible_and_are_recognized_as_an_existing_install() {
        let loaded: SavedSettings = ron::from_str("(version:2)").unwrap();
        assert!(!loaded.auto_graphics_configured);
        assert!(loaded.version < SETTINGS_VERSION);
    }

    #[test]
    fn customized_performance_settings_survive_a_round_trip() {
        let mut saved = SavedSettings::default();
        saved.graphics.apply_preset(LightingQuality::Performance);
        saved.graphics.speed_effect = GraphicsLevel::High;
        saved.graphics.reflection_quality = GraphicsLevel::Low;
        saved.graphics.rearview_mirrors = true;
        saved.graphics.tire_marks = true;
        saved.graphics.tire_smoke = true;

        let serialized = ron::to_string(&saved).unwrap();
        let loaded: SavedSettings = ron::from_str(&serialized).unwrap();
        assert_eq!(loaded.graphics.quality, LightingQuality::Performance);
        assert!(!loaded.graphics.matches_selected_preset());
        assert_eq!(loaded.graphics.speed_effect_strength(), 2.0);
        assert!(loaded.graphics.uses_reflections());
        assert!(loaded.graphics.mirrors_enabled());
        assert!(loaded.graphics.tire_marks_enabled());
        assert!(loaded.graphics.tire_smoke_enabled());
    }

    #[test]
    fn windows_portable_build_uses_the_writable_appdata_directory() {
        let path = windows_settings_file_path(Some("C:/Users/Test/AppData/Roaming".into()));
        assert_eq!(
            path,
            PathBuf::from("C:/Users/Test/AppData/Roaming")
                .join("Veyron Sim")
                .join("settings.ron")
        );
    }

    #[test]
    fn settings_file_is_really_written_and_loaded_again() {
        let unique = format!(
            "veyron-settings-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let directory = std::env::temp_dir().join(unique);
        let path = directory.join("Veyron Sim").join("settings.ron");
        let mut saved = SavedSettings::default();
        saved.display.mode = DisplayModeSetting::Windowed;
        saved.audio.engine_volume = 0.37;

        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text = ron::ser::to_string_pretty(&saved, ron::ser::PrettyConfig::new()).unwrap();
        fs::write(&path, text).unwrap();
        let loaded = read_settings(&path).expect("written settings reload");

        assert_eq!(loaded.display.mode, DisplayModeSetting::Windowed);
        assert!((loaded.audio.engine_volume - 0.37).abs() < f32::EPSILON);
        fs::remove_dir_all(directory).unwrap();
    }
}
