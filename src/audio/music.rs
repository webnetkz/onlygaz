//! Looped soundtrack used by the main, mode-selection and pause menus.

use bevy::audio::{AudioSink, AudioSinkPlayback, Volume};
use bevy::prelude::*;

use crate::game::GameState;

use super::AudioSettings;

pub const MENU_MUSIC_PATH: &str = "music/raim_where_are_you.mp3";

/// Marks the single menu soundtrack entity.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuMusicTrack;

#[derive(Resource)]
pub(super) struct MenuMusicSource(Handle<AudioSource>);

pub(super) fn load_menu_music(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.insert_resource(MenuMusicSource(asset_server.load(MENU_MUSIC_PATH)));
}

fn is_menu_state(state: GameState) -> bool {
    matches!(
        state,
        GameState::MainMenu | GameState::ModeSelection | GameState::Paused
    )
}

/// Keeps one looping player alive, pausing it during a race and resuming in menus.
pub(super) fn update_menu_music(
    mut commands: Commands,
    state: Res<State<GameState>>,
    audio: Res<AudioSettings>,
    source: Res<MenuMusicSource>,
    mut current: Query<(&mut PlaybackSettings, Option<&mut AudioSink>), With<MenuMusicTrack>>,
) {
    let in_menu = is_menu_state(*state.get());
    let volume = Volume::Linear(audio.music_volume.clamp(0.0, 1.0));

    if let Ok((mut playback, sink)) = current.single_mut() {
        // Keep the startup settings correct while the MP3 is still decoding.
        playback.volume = volume;
        playback.paused = !in_menu;

        if let Some(mut sink) = sink {
            sink.set_volume(volume);
            if in_menu {
                sink.play();
            } else {
                sink.pause();
            }
        }
        return;
    }

    if !in_menu {
        return;
    }

    commands.spawn((
        Name::new("Menu music"),
        MenuMusicTrack,
        AudioPlayer::new(source.0.clone()),
        PlaybackSettings {
            volume,
            ..PlaybackSettings::LOOP
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::audio::PlaybackMode;

    #[test]
    fn soundtrack_contains_the_supplied_mp3_file() {
        assert!(MENU_MUSIC_PATH.ends_with(".mp3"));
        assert!(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets")
                .join(MENU_MUSIC_PATH)
                .is_file(),
            "missing menu music asset: {MENU_MUSIC_PATH}"
        );
    }

    #[test]
    fn music_is_active_in_every_menu_including_the_launch_menu() {
        assert!(is_menu_state(GameState::MainMenu));
        assert!(is_menu_state(GameState::ModeSelection));
        assert!(is_menu_state(GameState::Paused));
        assert!(!is_menu_state(GameState::Driving));
    }

    #[test]
    fn menu_system_keeps_one_looping_player_across_state_changes() {
        let mut app = App::new();
        app.insert_resource(State::new(GameState::MainMenu))
            .insert_resource(AudioSettings {
                music_volume: 0.4,
                ..default()
            })
            .insert_resource(MenuMusicSource(Handle::default()))
            .add_systems(Update, update_menu_music);

        app.update();
        let first_entity = {
            let world = app.world_mut();
            let mut music =
                world.query_filtered::<(Entity, &PlaybackSettings), With<MenuMusicTrack>>();
            let (entity, playback) = music.single(world).unwrap();
            assert!(matches!(playback.mode, PlaybackMode::Loop));
            assert!(!playback.paused);
            assert!((playback.volume.to_linear() - 0.4).abs() < f32::EPSILON);
            entity
        };

        app.insert_resource(State::new(GameState::Driving));
        app.update();
        assert!(
            app.world()
                .get::<PlaybackSettings>(first_entity)
                .unwrap()
                .paused
        );

        app.insert_resource(State::new(GameState::Paused));
        app.update();
        let world = app.world_mut();
        let mut music = world.query_filtered::<Entity, With<MenuMusicTrack>>();
        assert_eq!(music.iter(world).collect::<Vec<_>>(), vec![first_entity]);
        assert!(!world.get::<PlaybackSettings>(first_entity).unwrap().paused);
    }
}
