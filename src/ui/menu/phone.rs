//! Phone variant of the main / pause menu, for a landscape touch screen.
//!
//! It spawns the same marker components as the desktop layout, so every menu
//! handler and sync system in the parent module works unchanged. Only the
//! arrangement differs: panels sized for a phone held sideways (the virtual
//! canvas is at least 860 × 480 UI px, see `mobile::layout`), two columns,
//! taller buttons, BACK in the page header, finger-scrollable lists and a
//! touch-control page instead of the keyboard bindings table.

use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::ecs::system::EntityCommands;
use bevy::prelude::*;

use crate::audio::radio::RadioSkip;
use crate::game::GameState;
use crate::world::lighting::LightingQuality;

use super::{
    AudioMenuPanel, BORDER_NORMAL, BUTTON_NORMAL, ControlsMenuPanel, CurrentRadioTrackText,
    GraphicsMenuPanel, GraphicsQualityButton, GraphicsQualityDescription, GraphicsScroll,
    MainMenuLogo, MainMenuPanel, MaterialButtonTone, MenuAction, MenuActionColumn, MenuCopy,
    PRIMARY, PauseOverlay, RadioControlButton, SCRIM, SURFACE, SURFACE_LOW, SettingKind, TEXT_MAIN,
    TEXT_MUTED, TouchScroll, accent_bar, action_button_sized, column_header, control_shadow,
    menu_font, panel_shadow, quality_description, quality_label, setting_row,
};

/// Widest a panel grows on large phones and tablets.
const PANEL_MAX_WIDTH: f32 = 1100.0;
/// Height of the full-width action buttons.
const ACTION_HEIGHT: f32 = 48.0;

/// An action that only makes sense during a race ("Exit to main menu").
#[derive(Component)]
pub(crate) struct PauseOnlyAction;

/// "Straight ahead: SET CURRENT" on the touch-controls page.
#[derive(Component)]
pub(crate) struct TiltCentreButton;

/// Live tilt-sensor read-out on the touch-controls page.
#[derive(Component)]
pub(crate) struct TiltStatusText;

fn panel_frame() -> Node {
    Node {
        width: percent(100.0),
        max_width: px(PANEL_MAX_WIDTH),
        max_height: percent(100.0),
        flex_direction: FlexDirection::Column,
        row_gap: px(8),
        padding: UiRect::axes(px(18), px(14)),
        border: UiRect::all(px(1)),
        border_radius: BorderRadius::ZERO,
        ..default()
    }
}

fn panel_bundle() -> impl Bundle {
    (
        BackgroundColor(SURFACE),
        BorderColor::all(Color::srgba(0.52, 0.68, 0.86, 0.30)),
        panel_shadow(),
    )
}

/// Two side-by-side columns; `left` is the width share of the first one.
fn columns(
    parent: &mut ChildSpawnerCommands,
    left: f32,
    build_left: impl FnOnce(&mut ChildSpawnerCommands),
    build_right: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn(Node {
            width: percent(100.0),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::FlexStart,
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|row| {
            row.spawn(Node {
                width: percent(left),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                ..default()
            })
            .with_children(build_left);
            row.spawn(Node {
                width: percent(100.0 - left - 3.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                ..default()
            })
            .with_children(build_right);
        });
}

fn sized_action<'a>(
    parent: &'a mut ChildSpawnerCommands,
    title: &str,
    action: MenuAction,
    width: Val,
    font: &Handle<Font>,
) -> EntityCommands<'a> {
    let mut button = action_button_sized(parent, title, action, width, font);
    button.entry::<Node>().and_modify(|mut node| {
        node.height = px(ACTION_HEIGHT);
        node.flex_shrink = 0.0;
    });
    button
}

/// Page title on the left, BACK on the right, saving a row of height.
fn page_title_row(
    parent: &mut ChildSpawnerCommands,
    eyebrow: &str,
    title: &str,
    description: &str,
    bold: &Handle<Font>,
    regular: &Handle<Font>,
) {
    accent_bar(parent);
    parent
        .spawn(Node {
            width: percent(100.0),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|row| {
            row.spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(2),
                ..default()
            })
            .with_children(|texts| {
                texts.spawn((
                    Text::new(eyebrow),
                    menu_font(bold, 12.0),
                    TextColor(PRIMARY),
                ));
                texts.spawn((
                    Text::new(title),
                    menu_font(bold, 28.0),
                    TextColor(TEXT_MAIN),
                ));
                if !description.is_empty() {
                    texts.spawn((
                        Text::new(description),
                        menu_font(regular, 13.0),
                        TextColor(TEXT_MUTED),
                    ));
                }
            });
            sized_action(row, "BACK", MenuAction::Back, px(210), bold);
        });
}

fn radio_button(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    direction: RadioSkip,
    font: &Handle<Font>,
) {
    parent
        .spawn((
            Button,
            RadioControlButton(direction),
            MaterialButtonTone::Tonal,
            Node {
                width: percent(48.0),
                height: px(48),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(BORDER_NORMAL),
        ))
        .with_child((
            Text::new(title),
            menu_font(font, 14.0),
            TextColor(TEXT_MAIN),
        ));
}

fn hint(parent: &mut ChildSpawnerCommands, text: &str, font: &Handle<Font>) {
    parent.spawn((
        Text::new(text),
        menu_font(font, 13.0),
        TextColor(TEXT_MUTED),
        Node {
            flex_shrink: 0.0,
            ..default()
        },
    ));
}

pub(super) fn spawn_phone_menu(
    commands: &mut Commands,
    regular: &Handle<Font>,
    bold: &Handle<Font>,
    logo: &Handle<Image>,
) {
    commands
        .spawn((
            Name::new("PauseOverlay"),
            PauseOverlay,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.0),
                height: percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::all(px(10)),
                ..default()
            },
            BackgroundColor(SCRIM),
            Visibility::Hidden,
            ZIndex(100),
        ))
        .with_children(|overlay| {
            spawn_main_panel(overlay, regular, bold, logo);
            spawn_graphics_panel(overlay, regular, bold);
            spawn_controls_panel(overlay, regular, bold);
            spawn_audio_panel(overlay, regular, bold);
        });
}

fn spawn_main_panel(
    overlay: &mut ChildSpawnerCommands,
    regular: &Handle<Font>,
    bold: &Handle<Font>,
    logo: &Handle<Image>,
) {
    overlay
        .spawn((
            MainMenuPanel,
            TouchScroll,
            ScrollPosition::default(),
            Node {
                max_width: px(980),
                overflow: Overflow::scroll_y(),
                ..panel_frame()
            },
            BackgroundColor(SURFACE),
            BorderColor::all(Color::srgba(0.39, 0.65, 0.82, 0.42)),
            panel_shadow(),
        ))
        .with_children(|panel| {
            accent_bar(panel);
            panel.spawn((
                Text::new("ONLYGAZ  /  MAIN MENU"),
                MenuCopy::Status,
                menu_font(bold, 13.0),
                TextColor(PRIMARY),
            ));
            panel
                .spawn(Node {
                    width: percent(100.0),
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    column_gap: px(20),
                    flex_shrink: 0.0,
                    ..default()
                })
                .with_children(|content| {
                    content
                        .spawn((
                            Node {
                                width: percent(44.0),
                                flex_direction: FlexDirection::Column,
                                justify_content: JustifyContent::Center,
                                row_gap: px(8),
                                padding: UiRect::all(px(16)),
                                border: UiRect::all(px(1)),
                                ..default()
                            },
                            BackgroundColor(SURFACE_LOW),
                            BorderColor::all(Color::srgba(0.24, 0.38, 0.52, 0.60)),
                        ))
                        .with_children(|hero| {
                            hero.spawn((
                                Text::new("DRIVE / READY"),
                                MenuCopy::HeroLabel,
                                menu_font(bold, 12.0),
                                TextColor(PRIMARY),
                            ));
                            hero.spawn((
                                Name::new("OnlyGAZ main menu logo"),
                                MainMenuLogo,
                                ImageNode::new(logo.clone()),
                                Node {
                                    width: px(168),
                                    height: px(112),
                                    align_self: AlignSelf::Center,
                                    flex_shrink: 0.0,
                                    ..default()
                                },
                            ));
                            hero.spawn((
                                Text::new("START YOUR\nENGINE"),
                                MenuCopy::HeroTitle,
                                menu_font(bold, 32.0),
                                TextColor(TEXT_MAIN),
                            ));
                            hero.spawn((
                                Text::new("Tune the experience, then press Play to start."),
                                MenuCopy::HeroSubtitle,
                                menu_font(regular, 14.0),
                                TextColor(TEXT_MUTED),
                            ));
                            hero.spawn((
                                Text::new("ENVIRONMENT"),
                                menu_font(bold, 12.0),
                                TextColor(PRIMARY),
                            ));
                            setting_row(hero, "Time of day", SettingKind::TimeOfDay, regular);
                        });
                    content
                        .spawn((
                            MenuActionColumn,
                            Node {
                                width: percent(53.0),
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Stretch,
                                justify_content: JustifyContent::Center,
                                row_gap: px(8),
                                ..default()
                            },
                        ))
                        .with_children(|actions| {
                            actions.spawn((
                                Text::new("MAIN MENU"),
                                MenuCopy::ActionsLabel,
                                menu_font(bold, 12.0),
                                TextColor(TEXT_MUTED),
                            ));
                            let full = percent(100.0);
                            sized_action(actions, "PLAY", MenuAction::Resume, full, bold);
                            sized_action(
                                actions,
                                "EXIT TO MAIN MENU",
                                MenuAction::MainMenu,
                                full,
                                bold,
                            )
                            .insert(PauseOnlyAction);
                            sized_action(actions, "GRAPHICS", MenuAction::OpenGraphics, full, bold);
                            sized_action(actions, "CONTROLS", MenuAction::OpenControls, full, bold);
                            sized_action(actions, "AUDIO", MenuAction::OpenAudio, full, bold);
                            sized_action(actions, "QUIT GAME", MenuAction::Quit, full, bold);
                        });
                });
        });
}

fn spawn_graphics_panel(
    overlay: &mut ChildSpawnerCommands,
    regular: &Handle<Font>,
    bold: &Handle<Font>,
) {
    overlay
        .spawn((
            GraphicsMenuPanel,
            Node {
                height: percent(100.0),
                ..panel_frame()
            },
            panel_bundle(),
            Visibility::Hidden,
        ))
        .with_children(|panel| {
            // Title, presets and BACK share one row so the settings list keeps
            // as much of the short landscape screen as possible.
            accent_bar(panel);
            panel
                .spawn(Node {
                    width: percent(100.0),
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    column_gap: px(10),
                    flex_shrink: 0.0,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(2),
                        ..default()
                    })
                    .with_children(|texts| {
                        texts.spawn((
                            Text::new("VISUAL EXPERIENCE  •  DRAG TO SCROLL"),
                            menu_font(bold, 12.0),
                            TextColor(PRIMARY),
                        ));
                        texts.spawn((
                            Text::new("GRAPHICS"),
                            menu_font(bold, 28.0),
                            TextColor(TEXT_MAIN),
                        ));
                    });
                    row.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(8),
                        ..default()
                    })
                    .with_children(|presets| {
                        for quality in LightingQuality::ALL {
                            presets
                                .spawn((
                                    Button,
                                    GraphicsQualityButton(quality),
                                    Node {
                                        width: px(146),
                                        height: px(46),
                                        flex_shrink: 0.0,
                                        align_items: AlignItems::Center,
                                        justify_content: JustifyContent::Center,
                                        border: UiRect::all(px(2)),
                                        border_radius: BorderRadius::ZERO,
                                        ..default()
                                    },
                                    BackgroundColor(BUTTON_NORMAL),
                                    BorderColor::all(BORDER_NORMAL),
                                    control_shadow(),
                                ))
                                .with_child((
                                    Text::new(quality_label(quality)),
                                    menu_font(bold, 13.0),
                                    TextColor(TEXT_MAIN),
                                ));
                        }
                    });
                    sized_action(row, "BACK", MenuAction::Back, px(160), bold);
                });
            panel
                .spawn((
                    GraphicsScroll,
                    TouchScroll,
                    ScrollPosition::default(),
                    Node {
                        width: percent(100.0),
                        flex_grow: 1.0,
                        flex_basis: px(0),
                        min_height: px(0),
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                ))
                .with_children(|list| {
                    columns(
                        list,
                        48.5,
                        |column| {
                            column_header(column, "WORLD", bold);
                            setting_row(
                                column,
                                "Anti-aliasing",
                                SettingKind::AntiAliasing,
                                regular,
                            );
                            setting_row(
                                column,
                                "View distance",
                                SettingKind::ViewDistance,
                                regular,
                            );
                            setting_row(column, "Traffic", SettingKind::TrafficDensity, regular);
                            setting_row(column, "Shadows", SettingKind::Shadows, regular);
                            setting_row(
                                column,
                                "Contact shadows",
                                SettingKind::ContactShadows,
                                regular,
                            );
                            setting_row(column, "Tyre marks", SettingKind::TireMarks, regular);
                            setting_row(column, "Tyre smoke", SettingKind::TireSmoke, regular);
                            setting_row(
                                column,
                                "Rear-view mirrors",
                                SettingKind::RearviewMirrors,
                                regular,
                            );
                            setting_row(column, "Exposure", SettingKind::Exposure, regular);
                        },
                        |column| {
                            column_header(column, "EFFECTS", bold);
                            setting_row(
                                column,
                                "Speed & nitro FX",
                                SettingKind::SpeedEffect,
                                regular,
                            );
                            setting_row(
                                column,
                                "Ambient occlusion",
                                SettingKind::AmbientOcclusion,
                                regular,
                            );
                            setting_row(column, "Reflections", SettingKind::Reflections, regular);
                            setting_row(
                                column,
                                "Atmospheric haze",
                                SettingKind::Atmosphere,
                                regular,
                            );
                            setting_row(column, "Bloom", SettingKind::Bloom, regular);
                            setting_row(column, "Motion blur", SettingKind::MotionBlur, regular);
                            setting_row(
                                column,
                                "Collision blur",
                                SettingKind::CollisionEffect,
                                regular,
                            );
                            setting_row(column, "Sharpening", SettingKind::Sharpening, regular);
                        },
                    );
                });
            panel.spawn((
                Text::new(quality_description(LightingQuality::Performance)),
                GraphicsQualityDescription,
                menu_font(regular, 12.0),
                TextColor(TEXT_MUTED),
                Node {
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
        });
}

fn spawn_controls_panel(
    overlay: &mut ChildSpawnerCommands,
    regular: &Handle<Font>,
    bold: &Handle<Font>,
) {
    overlay
        .spawn((
            ControlsMenuPanel,
            TouchScroll,
            ScrollPosition::default(),
            Node {
                overflow: Overflow::scroll_y(),
                ..panel_frame()
            },
            panel_bundle(),
            Visibility::Hidden,
        ))
        .with_children(|panel| {
            page_title_row(
                panel,
                "TOUCH CONTROLS",
                "CONTROLS",
                "Choose how to steer. Changes apply immediately.",
                bold,
                regular,
            );
            columns(
                panel,
                52.0,
                |column| {
                    column_header(column, "STEERING", bold);
                    setting_row(column, "Steering", SettingKind::SteeringMode, regular);
                    setting_row(
                        column,
                        "Tilt sensitivity",
                        SettingKind::TiltSensitivity,
                        regular,
                    );
                    column
                        .spawn((
                            Node {
                                width: percent(100.0),
                                height: px(52),
                                flex_shrink: 0.0,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::SpaceBetween,
                                padding: UiRect::horizontal(px(12)),
                                margin: UiRect::vertical(px(2)),
                                border: UiRect::all(px(1)),
                                ..default()
                            },
                            BackgroundColor(SURFACE_LOW),
                            BorderColor::all(Color::srgba(0.23, 0.32, 0.43, 0.40)),
                        ))
                        .with_children(|row| {
                            row.spawn((
                                Text::new("Straight ahead"),
                                menu_font(regular, 15.0),
                                TextColor(TEXT_MUTED),
                            ));
                            row.spawn((
                                Button,
                                TiltCentreButton,
                                MaterialButtonTone::Tonal,
                                Node {
                                    width: px(210),
                                    height: px(36),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    border: UiRect::all(px(1)),
                                    ..default()
                                },
                                BackgroundColor(BUTTON_NORMAL),
                                BorderColor::all(BORDER_NORMAL),
                            ))
                            .with_child((
                                Text::new("SET CURRENT"),
                                menu_font(bold, 13.0),
                                TextColor(TEXT_MAIN),
                            ));
                        });
                    column.spawn((
                        Text::new("Tilt sensor: waiting"),
                        TiltStatusText,
                        menu_font(regular, 13.0),
                        TextColor(TEXT_MAIN),
                        Node {
                            flex_shrink: 0.0,
                            margin: UiRect::top(px(4)),
                            ..default()
                        },
                    ));
                },
                |column| {
                    column_header(column, "ON-SCREEN CONTROLS", bold);
                    hint(
                        column,
                        "Right thumb: GAS, NITRO above it and BRAKE beside it. Hold BRAKE \
                         at a standstill to reverse.",
                        regular,
                    );
                    hint(
                        column,
                        "BUTTONS: steer with the arrows under the left thumb.",
                        regular,
                    );
                    hint(
                        column,
                        "TILT: hold the phone sideways and turn it like a steering \
                         wheel; BRAKE moves under the left thumb.",
                        regular,
                    );
                    hint(
                        column,
                        "Top right: reset (the R key on PC), camera and pause / menu. \
                         The phone's Back gesture pauses too.",
                        regular,
                    );
                },
            );
        });
}

fn spawn_audio_panel(
    overlay: &mut ChildSpawnerCommands,
    regular: &Handle<Font>,
    bold: &Handle<Font>,
) {
    overlay
        .spawn((
            AudioMenuPanel,
            TouchScroll,
            ScrollPosition::default(),
            Node {
                overflow: Overflow::scroll_y(),
                ..panel_frame()
            },
            panel_bundle(),
            Visibility::Hidden,
        ))
        .with_children(|panel| {
            page_title_row(
                panel,
                "SOUND MIXER",
                "AUDIO",
                "Balance the race soundtrack to your preference.",
                bold,
                regular,
            );
            columns(
                panel,
                52.0,
                |column| {
                    column_header(column, "CHANNEL VOLUME", bold);
                    setting_row(
                        column,
                        "Effects volume",
                        SettingKind::EffectsVolume,
                        regular,
                    );
                    setting_row(column, "Engine volume", SettingKind::EngineVolume, regular);
                    setting_row(column, "Music volume", SettingKind::MusicVolume, regular);
                },
                |column| {
                    column_header(column, "IN-CAR RADIO", bold);
                    column.spawn((
                        Text::new("Radio starts while driving"),
                        CurrentRadioTrackText,
                        menu_font(bold, 16.0),
                        TextColor(TEXT_MAIN),
                        Node {
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                    column
                        .spawn(Node {
                            width: percent(100.0),
                            justify_content: JustifyContent::SpaceBetween,
                            flex_shrink: 0.0,
                            ..default()
                        })
                        .with_children(|row| {
                            radio_button(row, "‹  PREVIOUS", RadioSkip::Previous, bold);
                            radio_button(row, "NEXT  ›", RadioSkip::Next, bold);
                        });
                    hint(
                        column,
                        "Tracks can be changed here while the race is paused.",
                        regular,
                    );
                },
            );
        });
}

/// "Exit to main menu" exists only in the pause menu, not on the launch screen.
pub(crate) fn sync_pause_only_actions(
    state: Res<State<GameState>>,
    mut actions: Query<&mut Node, With<PauseOnlyAction>>,
) {
    let display = if *state.get() == GameState::Paused {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut actions {
        if node.display != display {
            node.display = display;
        }
    }
}
