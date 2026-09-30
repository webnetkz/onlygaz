//! Shuffled in-car radio. Tracks play once in a shuffled order while driving.

use bevy::audio::{AudioSink, AudioSinkPlayback, Volume};
use bevy::prelude::*;

use crate::game::GameState;
use crate::vehicle::controller::{ControlAction, KeyBindings};

use super::AudioSettings;

#[derive(Clone, Copy)]
struct RadioSong {
    title: &'static str,
    path: &'static str,
}

const SONGS: [RadioSong; 13] = [
    RadioSong {
        title: "Pink Reprise",
        path: "radio/1-13 Pink Reprise.mp3",
    },
    RadioSong {
        title: "Echoes of Mystery",
        path: "radio/1-02 Echoes of Mystery.mp3",
    },
    RadioSong {
        title: "Memory Reboot",
        path: "radio/1-01 Memory Reboot.mp3",
    },
    RadioSong {
        title: "Hoopin Hard",
        path: "radio/1-01 Hoopin Hard.mp3",
    },
    RadioSong {
        title: "Five Days Stress",
        path: "radio/1-01 Five Days Stress.mp3",
    },
    RadioSong {
        title: "Intro",
        path: "radio/1-01 Intro.mp3",
    },
    RadioSong {
        title: "nothing",
        path: "radio/1-01 nothing.mp3",
    },
    RadioSong {
        title: "Carry U",
        path: "radio/1-01 Carry U.mp3",
    },
    RadioSong {
        title: "Stranger Things",
        path: "radio/1-05 Stranger Things.mp3",
    },
    RadioSong {
        title: "Treachery",
        path: "radio/1-01 Treachery.mp3",
    },
    RadioSong {
        title: "I Got 5 On It",
        path: "radio/1-181 I Got 5 On It.mp3",
    },
    RadioSong {
        title: "LUNAR",
        path: "radio/1-01 LUNAR.mp3",
    },
    RadioSong {
        title: "Gone",
        path: "radio/1-01 Gone.mp3",
    },
];

#[derive(Component)]
pub(crate) struct RadioTrack;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RadioSkip {
    Previous,
    Next,
}

#[derive(Resource)]
pub struct RadioPlaylist {
    order: [usize; SONGS.len()],
    cursor: Option<usize>,
    player: Option<Entity>,
    shuffled: bool,
    pending_skip: Option<RadioSkip>,
}

impl Default for RadioPlaylist {
    fn default() -> Self {
        Self {
            order: std::array::from_fn(|index| index),
            cursor: None,
            player: None,
            shuffled: false,
            pending_skip: None,
        }
    }
}

impl RadioPlaylist {
    fn shuffle(&mut self, avoid_first: Option<usize>) {
        fastrand::shuffle(&mut self.order);
        if let Some(previous) = avoid_first
            && self.order[0] == previous
        {
            self.order.swap(0, 1);
        }
        self.shuffled = true;
    }

    fn next(&mut self) -> usize {
        if !self.shuffled {
            self.shuffle(None);
            self.cursor = Some(0);
        } else if let Some(cursor) = self.cursor {
            if cursor + 1 == SONGS.len() {
                let last = self.order[cursor];
                self.shuffle(Some(last));
                self.cursor = Some(0);
            } else {
                self.cursor = Some(cursor + 1);
            }
        } else {
            self.cursor = Some(0);
        }
        self.order[self.cursor.expect("radio cursor selected")]
    }

    fn previous(&mut self) -> usize {
        if !self.shuffled {
            self.shuffle(None);
        }
        self.cursor = Some(match self.cursor {
            Some(0) | None => SONGS.len() - 1,
            Some(cursor) => cursor - 1,
        });
        self.order[self.cursor.expect("radio cursor selected")]
    }

    pub(crate) fn current_title(&self) -> Option<&'static str> {
        self.cursor.map(|cursor| SONGS[self.order[cursor]].title)
    }

    pub(crate) fn request_skip(&mut self, direction: RadioSkip) {
        self.pending_skip = Some(direction);
    }

    fn reset(&mut self) {
        self.cursor = None;
        self.player = None;
        self.shuffled = false;
        self.pending_skip = None;
    }
}

fn spawn_radio_track(
    commands: &mut Commands,
    asset_server: &AssetServer,
    song_index: usize,
    volume: Volume,
    paused: bool,
) -> Entity {
    let song = SONGS[song_index];
    info!("radio: {}", song.title);
    commands
        .spawn((
            Name::new(format!("Radio: {}", song.title)),
            RadioTrack,
            AudioPlayer::new(asset_server.load(song.path)),
            PlaybackSettings {
                volume,
                paused,
                ..PlaybackSettings::ONCE
            },
        ))
        .id()
}

pub(crate) fn update_radio(
    mut commands: Commands,
    state: Res<State<GameState>>,
    audio: Res<AudioSettings>,
    asset_server: Res<AssetServer>,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<KeyBindings>,
    mut playlist: ResMut<RadioPlaylist>,
    mut players: Query<(&mut PlaybackSettings, Option<&mut AudioSink>), With<RadioTrack>>,
) {
    let volume = Volume::Linear(audio.music_volume.clamp(0.0, 1.0));
    if matches!(*state.get(), GameState::MainMenu | GameState::ModeSelection) {
        if let Some(entity) = playlist.player.take() {
            commands.entity(entity).despawn();
        }
        if playlist.shuffled || playlist.pending_skip.is_some() {
            playlist.reset();
        }
        return;
    }

    if *state.get() == GameState::Paused {
        if let Some(skip) = playlist.pending_skip.take() {
            if let Some(entity) = playlist.player.take() {
                commands.entity(entity).despawn();
            }
            let song_index = if skip == RadioSkip::Previous {
                playlist.previous()
            } else {
                playlist.next()
            };
            playlist.player = Some(spawn_radio_track(
                &mut commands,
                &asset_server,
                song_index,
                volume,
                true,
            ));
        }
        if let Some(entity) = playlist.player
            && let Ok((mut playback, sink)) = players.get_mut(entity)
        {
            playback.volume = volume;
            playback.paused = true;
            if let Some(mut sink) = sink {
                sink.set_volume(volume);
                sink.pause();
            }
        }
        return;
    }

    let skip = if bindings.just_pressed(&keys, ControlAction::NextRadioTrack) {
        playlist.pending_skip = None;
        Some(RadioSkip::Next)
    } else if bindings.just_pressed(&keys, ControlAction::PreviousRadioTrack) {
        playlist.pending_skip = None;
        Some(RadioSkip::Previous)
    } else {
        playlist.pending_skip.take()
    };

    let mut finished = false;
    if let Some(entity) = playlist.player {
        if let Ok((mut playback, sink)) = players.get_mut(entity) {
            playback.volume = volume;
            playback.paused = false;
            if let Some(mut sink) = sink {
                sink.set_volume(volume);
                sink.play();
                finished = sink.empty();
            }
        } else {
            finished = true;
        }
    }

    if skip.is_some() || finished {
        if let Some(entity) = playlist.player.take() {
            commands.entity(entity).despawn();
        }
    }
    if playlist.player.is_some() {
        return;
    }

    let song_index = if skip == Some(RadioSkip::Previous) {
        playlist.previous()
    } else {
        playlist.next()
    };
    playlist.player = Some(spawn_radio_track(
        &mut commands,
        &asset_server,
        song_index,
        volume,
        false,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_track_exists_in_the_game_assets() {
        assert_eq!(SONGS.len(), 13);
        for song in SONGS {
            assert!(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("assets")
                    .join(song.path)
                    .is_file(),
                "missing radio track: {}",
                song.path
            );
        }
    }

    #[test]
    fn shuffled_cycle_plays_each_track_once_and_avoids_boundary_repeat() {
        let mut radio = RadioPlaylist::default();
        let mut previous_last = None;
        for _ in 0..4 {
            let cycle: Vec<_> = (0..SONGS.len()).map(|_| radio.next()).collect();
            let mut sorted = cycle.clone();
            sorted.sort_unstable();
            assert_eq!(sorted, (0..SONGS.len()).collect::<Vec<_>>());
            if let Some(last) = previous_last {
                assert_ne!(cycle[0], last);
            }
            previous_last = cycle.last().copied();
        }
    }

    #[test]
    fn previous_and_next_follow_the_current_shuffled_order() {
        let mut radio = RadioPlaylist::default();
        let first = radio.next();
        let second = radio.next();
        assert_ne!(first, second);
        assert_eq!(radio.previous(), first);
        assert_eq!(radio.next(), second);
        radio.reset();
        assert!(radio.current_title().is_none());
    }
}
