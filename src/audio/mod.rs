//! Sound: W16 production pack, effects, and the existing music channels.

pub mod engine;
mod menu_sfx;
pub mod music;
pub mod radio;
pub mod vehicle_sfx;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::game::GameState;

/// User-facing volume mix. Values are normalized to `0..=1` and can be
/// changed from the pause menu while the game is running.
#[derive(Resource, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Resource)]
#[serde(default)]
pub struct AudioSettings {
    /// Master level for effects, including braking and collisions.
    pub effects_volume: f32,
    /// Additional level applied to the continuous engine channel.
    pub engine_volume: f32,
    /// Independent level for the menu soundtrack.
    pub music_volume: f32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            effects_volume: 1.0,
            engine_volume: 1.0,
            music_volume: 0.7,
        }
    }
}

impl AudioSettings {
    /// Move a volume control to the next 10% step, wrapping after 100%.
    pub fn next_volume(value: f32) -> f32 {
        let step = (value.clamp(0.0, 1.0) * 10.0).round() as u8;
        f32::from((step + 1) % 11) / 10.0
    }

    pub fn effects_gain(&self) -> f32 {
        self.effects_volume.clamp(0.0, 1.0)
    }

    pub fn engine_gain(&self) -> f32 {
        self.engine_volume.clamp(0.0, 1.0)
    }
}

pub struct EngineAudioPlugin;

impl Plugin for EngineAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(w16_audio::W16AudioPlugin {
            config: w16_audio::W16AudioConfig {
                // Leave headroom for impacts and music even at 100% settings.
                master_volume: 0.55,
                ..default()
            },
        })
        .init_resource::<AudioSettings>()
        .init_resource::<engine::EngineAudioSettings>()
        .init_resource::<engine::PreviousGear>()
        .init_resource::<menu_sfx::MenuSfxState>()
        .init_resource::<vehicle_sfx::VehicleSfxState>()
        .init_resource::<radio::RadioPlaylist>()
        .register_type::<AudioSettings>()
        .register_type::<engine::EngineAudioSettings>()
        .add_systems(
            Startup,
            (
                music::load_menu_music,
                vehicle_sfx::setup_brake_skid_sfx,
                vehicle_sfx::setup_wheelspin_sfx,
                vehicle_sfx::load_collision_sounds,
            ),
        )
        .add_systems(PreUpdate, engine::feed_w16_audio)
        .add_systems(Update, menu_sfx::play_menu_sfx)
        .add_systems(
            Update,
            (
                engine::toggle_engine_audio,
                vehicle_sfx::update_brake_skid_sfx,
                vehicle_sfx::update_wheelspin_sfx,
                vehicle_sfx::play_collision_sfx,
                vehicle_sfx::play_breakage_sfx,
            )
                .chain()
                .run_if(in_state(GameState::Driving)),
        )
        .add_systems(Update, music::update_menu_music)
        .add_systems(Update, radio::update_radio)
        .add_systems(
            OnEnter(GameState::Paused),
            (
                vehicle_sfx::silence_brake_skid_sfx,
                vehicle_sfx::silence_wheelspin_sfx,
            ),
        )
        .add_systems(
            OnEnter(GameState::MainMenu),
            (
                vehicle_sfx::silence_brake_skid_sfx,
                vehicle_sfx::silence_wheelspin_sfx,
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mix_has_independent_channels() {
        let settings = AudioSettings::default();
        assert_eq!(settings.effects_volume, 1.0);
        assert_eq!(settings.engine_volume, 1.0);
        assert!((settings.music_volume - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn volume_steps_wrap_from_zero_to_full() {
        assert!((AudioSettings::next_volume(0.0) - 0.1).abs() < f32::EPSILON);
        assert!((AudioSettings::next_volume(0.9) - 1.0).abs() < f32::EPSILON);
        assert!(AudioSettings::next_volume(1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn every_pack_sound_is_a_bundled_vorbis_asset() {
        assert_eq!(w16_audio::SOUNDS.len(), 260);
        for sound in w16_audio::SOUNDS {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets/audio/w16")
                .join(format!("{}.ogg", sound.path));
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|error| panic!("missing {}: {error}", path.display()));
            assert!(
                bytes.starts_with(b"OggS"),
                "invalid OGG: {}",
                path.display()
            );
            assert!(
                bytes.windows(6).any(|window| window == b"vorbis"),
                "not Vorbis: {}",
                path.display()
            );
        }
    }
}
