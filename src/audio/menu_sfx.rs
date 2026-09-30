//! Lightweight, throttled menu feedback from the W16 pack.

use bevy::prelude::*;
use w16_audio::W16Sfx;

use crate::game::GameState;

use super::AudioSettings;

#[derive(Resource, Default)]
pub(crate) struct MenuSfxState {
    hover_cooldown: f32,
}

pub(crate) fn play_menu_sfx(
    time: Res<Time>,
    game: Res<State<GameState>>,
    audio: Res<AudioSettings>,
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<MenuSfxState>,
    interactions: Query<&Interaction, (Changed<Interaction>, With<Button>)>,
    mut sounds: MessageWriter<W16Sfx>,
) {
    state.hover_cooldown = (state.hover_cooldown - time.delta_secs()).max(0.0);
    if *game.get() == GameState::Driving {
        return;
    }
    let pressed = interactions
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed);
    let hovered = interactions
        .iter()
        .any(|interaction| *interaction == Interaction::Hovered);
    let id = if pressed {
        Some("ui_select")
    } else if keys.just_pressed(KeyCode::Escape) {
        Some("ui_back")
    } else if hovered && state.hover_cooldown <= 0.0 {
        state.hover_cooldown = 0.08;
        Some("ui_hover")
    } else {
        None
    };
    if let Some(id) = id {
        let mut sound = W16Sfx::new(id);
        sound.volume = audio.effects_gain() * 0.38;
        sounds.write(sound);
    }
}
