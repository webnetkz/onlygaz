//! Choose whether the offline car can be damaged before entering the track.

use crate::game::GameState;
use crate::vehicle::damage::DamageSettings;
use bevy::prelude::*;
use bevy::text::FontSize;

#[derive(Component)]
pub(crate) struct StartScreenRoot;

#[derive(Component, Clone, Copy)]
pub(crate) enum StartAction {
    DamageOn,
    DamageOff,
    Back,
}

fn button(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    label: &str,
    action: StartAction,
    font: &Handle<Font>,
) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width: px(370),
                height: px(58),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.05, 0.11, 0.17)),
            BorderColor::all(Color::srgb(0.28, 0.72, 1.0)),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font: font.clone().into(),
                font_size: FontSize::Px(19.0),
                ..default()
            },
            TextColor(Color::WHITE),
        ));
}

pub(crate) fn spawn_start_screen(mut commands: Commands, assets: Res<AssetServer>) {
    let font = assets.load("fonts/ubuntu/Ubuntu-Regular.ttf");
    commands
        .spawn((
            Name::new("Offline race setup"),
            StartScreenRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.0),
                height: percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(15),
                ..default()
            },
            BackgroundColor(Color::srgba(0.006, 0.010, 0.020, 0.92)),
            Visibility::Hidden,
            GlobalZIndex(100),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("OFFLINE RACE"),
                TextFont {
                    font: font.clone().into(),
                    font_size: FontSize::Px(36.0),
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
            panel.spawn((
                Text::new("Choose whether collisions can damage your car"),
                TextFont {
                    font: font.clone().into(),
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                TextColor(Color::srgb(0.65, 0.75, 0.85)),
            ));
            button(panel, "DRIVE WITH DAMAGE", StartAction::DamageOn, &font);
            button(panel, "DRIVE WITHOUT DAMAGE", StartAction::DamageOff, &font);
            button(panel, "BACK TO MAIN MENU", StartAction::Back, &font);
        });
}

pub(crate) fn handle_start_screen(
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
    mut damage: ResMut<DamageSettings>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Query<(&Interaction, &StartAction), Changed<Interaction>>,
) {
    if *state.get() != GameState::ModeSelection {
        return;
    }
    let action = buttons.iter().find_map(|(interaction, action)| {
        (*interaction == Interaction::Pressed).then_some(*action)
    });
    match action {
        Some(StartAction::DamageOn) => {
            damage.enabled = true;
            next.set(GameState::Driving);
        }
        Some(StartAction::DamageOff) => {
            damage.enabled = false;
            next.set(GameState::Driving);
        }
        Some(StartAction::Back) => next.set(GameState::MainMenu),
        None if keys.just_pressed(KeyCode::Escape) => next.set(GameState::MainMenu),
        None => {}
    }
}

pub(crate) fn sync_start_screen(
    state: Res<State<GameState>>,
    mut roots: Query<&mut Visibility, With<StartScreenRoot>>,
) {
    for mut visibility in &mut roots {
        *visibility = if *state.get() == GameState::ModeSelection {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}
