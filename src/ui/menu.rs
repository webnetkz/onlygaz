//! Pause menu and live, per-feature graphics settings.

use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::ecs::system::EntityCommands;
use bevy::prelude::*;
use bevy::text::FontSize;
use bevy::ui::RelativeCursorPosition;
use bevy::window::{
    Monitor, MonitorSelection, PresentMode, PrimaryMonitor, PrimaryWindow, VideoModeSelection,
    WindowMode,
};
use serde::{Deserialize, Serialize};

use crate::audio::radio::{RadioPlaylist, RadioSkip};
use crate::game::GameState;
use crate::mobile::TouchControlSettings;
use crate::vehicle::controller::{ControlAction, KeyBindings, KeySlot};
use crate::world::lighting::{
    AntiAliasingQuality, ExposureAdjustment, GraphicsLevel, LightingQuality, LightingSettings,
    ViewDistance,
};
use crate::world::traffic::TrafficSettings;

#[cfg(feature = "mobile")]
mod phone;
#[cfg(feature = "mobile")]
pub(crate) use phone::{TiltCentreButton, TiltStatusText, sync_pause_only_actions};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Resource)]
pub enum PauseMenuPage {
    #[default]
    Main,
    Graphics,
    Controls,
    Audio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DisplayModeSetting {
    Windowed,
    #[default]
    Fullscreen,
}

impl DisplayModeSetting {
    fn next(self) -> Self {
        match self {
            Self::Windowed => Self::Fullscreen,
            Self::Fullscreen => Self::Windowed,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Windowed => "WINDOWED",
            Self::Fullscreen => "FULLSCREEN",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ResolutionPreset {
    Hd,
    HdPlus,
    #[default]
    FullHd,
    Qhd,
    Uhd,
}

impl ResolutionPreset {
    fn next(self) -> Self {
        match self {
            Self::Hd => Self::HdPlus,
            Self::HdPlus => Self::FullHd,
            Self::FullHd => Self::Qhd,
            Self::Qhd => Self::Uhd,
            Self::Uhd => Self::Hd,
        }
    }

    fn size(self) -> (u32, u32) {
        match self {
            Self::Hd => (1280, 720),
            Self::HdPlus => (1600, 900),
            Self::FullHd => (1920, 1080),
            Self::Qhd => (2560, 1440),
            Self::Uhd => (3840, 2160),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Hd => "1280 × 720",
            Self::HdPlus => "1600 × 900",
            Self::FullHd => "1920 × 1080",
            Self::Qhd => "2560 × 1440",
            Self::Uhd => "3840 × 2160",
        }
    }
}

#[derive(Debug, Clone, Resource, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplaySettings {
    pub mode: DisplayModeSetting,
    pub resolution: ResolutionPreset,
    pub vsync: bool,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            mode: DisplayModeSetting::Fullscreen,
            resolution: ResolutionPreset::FullHd,
            vsync: true,
        }
    }
}

#[derive(Component)]
pub(crate) struct PauseOverlay;

#[derive(Component)]
pub(crate) struct MainMenuPanel;

#[derive(Component)]
pub(crate) struct MainMenuLogo;

#[derive(Component)]
pub(crate) struct MenuActionColumn;

#[derive(Component, Clone, Copy)]
pub(crate) enum MenuCopy {
    Status,
    HeroLabel,
    HeroTitle,
    HeroSubtitle,
    ActionsLabel,
    PrimaryAction,
    Footer,
}

#[derive(Component)]
pub(crate) struct GraphicsMenuPanel;

#[derive(Component)]
pub(crate) struct ControlsMenuPanel;

#[derive(Component)]
pub(crate) struct AudioMenuPanel;

#[derive(Component)]
pub(crate) struct CurrentRadioTrackText;

#[derive(Component)]
pub(crate) struct RadioControlButton(pub RadioSkip);

#[derive(Component)]
pub struct GraphicsQualityButton(pub LightingQuality);

#[derive(Component)]
pub(crate) struct GraphicsQualityDescription;

#[derive(Debug, Clone, Copy, Component)]
pub(crate) enum MenuAction {
    Resume,
    OpenGraphics,
    OpenControls,
    OpenAudio,
    Back,
    Quit,
    /// Leave the current race for the main menu (phone pause menu).
    #[cfg_attr(not(feature = "mobile"), allow(dead_code))]
    MainMenu,
}

fn menu_is_visible(state: GameState) -> bool {
    matches!(state, GameState::MainMenu | GameState::Paused)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub(crate) enum MaterialButtonTone {
    Primary,
    Tonal,
    Outline,
    Danger,
    Icon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub(crate) enum SettingKind {
    Resolution,
    DisplayMode,
    Vsync,
    AntiAliasing,
    ViewDistance,
    TrafficDensity,
    Shadows,
    SpeedEffect,
    AmbientOcclusion,
    ContactShadows,
    Reflections,
    Atmosphere,
    Bloom,
    MotionBlur,
    CollisionEffect,
    RearviewMirrors,
    TireMarks,
    TireSmoke,
    Sharpening,
    Exposure,
    TimeOfDay,
    EffectsVolume,
    EngineVolume,
    MusicVolume,
    /// Phone build: on-screen buttons or tilt steering.
    #[cfg_attr(not(feature = "mobile"), allow(dead_code))]
    SteeringMode,
    /// Phone build: roll needed for full lock.
    #[cfg_attr(not(feature = "mobile"), allow(dead_code))]
    TiltSensitivity,
}

#[derive(Component)]
pub(crate) struct SettingValueText(SettingKind);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectDirection {
    Previous,
    Next,
}

#[derive(Debug, Clone, Copy, Component)]
pub(crate) struct SettingSelectButton {
    pub kind: SettingKind,
    pub direction: SelectDirection,
}

#[derive(Debug, Clone, Copy, Component)]
pub(crate) struct SettingToggleButton(pub SettingKind);

#[derive(Debug, Clone, Copy, Component)]
pub(crate) struct SettingToggleThumb(pub SettingKind);

#[derive(Component)]
pub(crate) struct SettingSelectValueSlot;

#[derive(Debug, Clone, Copy, Component)]
pub(crate) struct SettingSlider(pub SettingKind);

#[derive(Debug, Clone, Copy, Component)]
pub(crate) struct SettingSliderFill(pub SettingKind);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PendingRebind {
    pub action: ControlAction,
    pub slot: KeySlot,
}

#[derive(Debug, Default, Resource)]
pub(crate) struct RebindState {
    pub pending: Option<PendingRebind>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, Component)]
pub(crate) struct ControlBindingButton {
    pub action: ControlAction,
    pub slot: KeySlot,
}

#[derive(Debug, Clone, Copy, Component)]
pub(crate) struct ControlBindingValueText {
    pub action: ControlAction,
    pub slot: KeySlot,
}

#[derive(Component)]
pub(crate) struct RebindPromptText;

#[derive(Component)]
pub(crate) struct GraphicsScroll;

/// A scrollable menu area that follows a dragged finger (phone build).
#[derive(Component, Default)]
#[cfg_attr(not(feature = "mobile"), allow(dead_code))]
pub(crate) struct TouchScroll;

pub(crate) fn scroll_graphics(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    page: Res<PauseMenuPage>,
    state: Res<State<GameState>>,
    mut panels: Query<(&mut ScrollPosition, &ComputedNode), With<GraphicsScroll>>,
) {
    let delta: f32 = wheel
        .read()
        .map(|event| {
            event.y
                * match event.unit {
                    bevy::input::mouse::MouseScrollUnit::Line => 36.0,
                    bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
                }
        })
        .sum();
    if !menu_is_visible(*state.get()) || *page != PauseMenuPage::Graphics {
        return;
    }
    for (mut scroll, node) in &mut panels {
        let max = ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
        scroll.y = (scroll.y - delta).clamp(0.0, max);
    }
}

// Square-edged translucent cockpit glass. A light scrim leaves the running
// game legible through the panels while dark neutral surfaces protect text.
const SCRIM: Color = Color::srgba(0.006, 0.010, 0.018, 0.28);
const SURFACE: Color = Color::srgba(0.025, 0.035, 0.055, 0.78);
const SURFACE_LOW: Color = Color::srgba(0.033, 0.047, 0.071, 0.69);
const SURFACE_HIGH: Color = Color::srgba(0.050, 0.080, 0.114, 0.70);
const SURFACE_BRIGHT: Color = Color::srgba(0.075, 0.136, 0.187, 0.86);
const PRIMARY: Color = Color::srgb(0.28, 0.72, 1.0);
const PRIMARY_HOVER: Color = Color::srgba(0.08, 0.34, 0.51, 0.95);
const PRIMARY_PRESSED: Color = Color::srgba(0.03, 0.23, 0.36, 0.96);
const PRIMARY_CONTAINER: Color = Color::srgba(0.04, 0.25, 0.39, 0.78);
const DANGER: Color = Color::srgba(0.30, 0.07, 0.11, 0.68);
const DANGER_HOVER: Color = Color::srgba(0.46, 0.10, 0.16, 0.88);
const BUTTON_NORMAL: Color = SURFACE_HIGH;
const BUTTON_HOVERED: Color = SURFACE_BRIGHT;
const BUTTON_SELECTED: Color = PRIMARY_CONTAINER;
const BORDER_NORMAL: Color = Color::srgba(0.48, 0.67, 0.82, 0.60);
const BORDER_SELECTED: Color = PRIMARY;
const TEXT_MAIN: Color = Color::srgb(0.95, 0.96, 1.0);
const TEXT_MUTED: Color = Color::srgb(0.70, 0.75, 0.84);
const MENU_FONT_REGULAR: &str = "fonts/ubuntu/Ubuntu-Regular.ttf";
const MENU_FONT_BOLD: &str = "fonts/ubuntu/Ubuntu-Bold.ttf";

fn menu_font(font: &Handle<Font>, size: f32) -> TextFont {
    TextFont {
        font: font.into(),
        font_size: FontSize::Px(size),
        ..default()
    }
}

fn accent_bar(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        Node {
            width: px(74),
            height: px(4),
            border_radius: BorderRadius::ZERO,
            ..default()
        },
        BackgroundColor(PRIMARY),
    ));
}

fn page_header(
    parent: &mut ChildSpawnerCommands,
    eyebrow: &str,
    title: &str,
    description: &str,
    bold: &Handle<Font>,
    regular: &Handle<Font>,
) {
    accent_bar(parent);
    parent.spawn((
        Text::new(eyebrow),
        menu_font(bold, 12.0),
        TextColor(PRIMARY),
    ));
    parent.spawn((
        Text::new(title),
        menu_font(bold, 30.0),
        TextColor(TEXT_MAIN),
    ));
    parent.spawn((
        Text::new(description),
        menu_font(regular, 13.0),
        TextColor(TEXT_MUTED),
    ));
}

fn quality_description(quality: LightingQuality) -> &'static str {
    match quality {
        LightingQuality::Performance => {
            "Maximum frame rate by default. You can enable any feature below."
        }
        LightingQuality::Balanced => "Detailed visuals with a balanced rendering budget.",
        LightingQuality::Cinematic => "Full shadows, atmosphere, reflections and visual effects.",
    }
}

fn selected_quality_description(settings: &LightingSettings) -> String {
    if settings.matches_selected_preset() {
        quality_description(settings.quality).to_owned()
    } else {
        format!(
            "CUSTOM CONFIGURATION  •  {} base preset. Individual changes are saved.",
            settings.quality.label()
        )
    }
}

fn quality_label(quality: LightingQuality) -> &'static str {
    match quality {
        LightingQuality::Performance => "PERFORMANCE",
        LightingQuality::Balanced => "BALANCED",
        LightingQuality::Cinematic => "CINEMATIC",
    }
}

fn toggle_label(value: bool) -> &'static str {
    if value { "ON" } else { "OFF" }
}

fn panel_shadow() -> BoxShadow {
    BoxShadow(vec![
        ShadowStyle {
            color: Color::srgba(0.0, 0.0, 0.0, 0.32),
            x_offset: px(0),
            y_offset: px(12),
            spread_radius: px(0),
            blur_radius: px(25),
        },
        ShadowStyle {
            color: Color::srgba(0.02, 0.35, 0.58, 0.12),
            x_offset: px(0),
            y_offset: px(0),
            spread_radius: px(0),
            blur_radius: px(10),
        },
    ])
}

fn control_shadow() -> BoxShadow {
    BoxShadow::new(
        Color::srgba(0.0, 0.0, 0.0, 0.22),
        px(0),
        px(3),
        px(0),
        px(8),
    )
}

fn action_tone(action: MenuAction) -> MaterialButtonTone {
    match action {
        MenuAction::Resume => MaterialButtonTone::Primary,
        MenuAction::Quit => MaterialButtonTone::Danger,
        MenuAction::Back => MaterialButtonTone::Outline,
        MenuAction::MainMenu => MaterialButtonTone::Outline,
        MenuAction::OpenGraphics | MenuAction::OpenControls | MenuAction::OpenAudio => {
            MaterialButtonTone::Tonal
        }
    }
}

fn tone_color(tone: MaterialButtonTone, interaction: Interaction) -> Color {
    match (tone, interaction) {
        (MaterialButtonTone::Primary, Interaction::Pressed) => PRIMARY_PRESSED,
        (MaterialButtonTone::Primary, Interaction::Hovered) => PRIMARY_HOVER,
        (MaterialButtonTone::Primary, Interaction::None) => PRIMARY_CONTAINER,
        (MaterialButtonTone::Danger, Interaction::Pressed) => Color::srgba(0.36, 0.06, 0.12, 0.96),
        (MaterialButtonTone::Danger, Interaction::Hovered) => DANGER_HOVER,
        (MaterialButtonTone::Danger, Interaction::None) => DANGER,
        (MaterialButtonTone::Outline, Interaction::Pressed) => PRIMARY_CONTAINER,
        (MaterialButtonTone::Outline, Interaction::Hovered) => SURFACE_BRIGHT,
        (MaterialButtonTone::Outline, Interaction::None) => Color::srgba(0.01, 0.02, 0.035, 0.42),
        (MaterialButtonTone::Tonal, Interaction::Pressed) => PRIMARY_CONTAINER,
        (MaterialButtonTone::Tonal, Interaction::Hovered) => SURFACE_BRIGHT,
        (MaterialButtonTone::Tonal, Interaction::None) => SURFACE_HIGH,
        (MaterialButtonTone::Icon, Interaction::Pressed) => PRIMARY_CONTAINER,
        (MaterialButtonTone::Icon, Interaction::Hovered) => SURFACE_BRIGHT,
        (MaterialButtonTone::Icon, Interaction::None) => Color::srgba(0.02, 0.08, 0.12, 0.34),
    }
}

fn tone_border(tone: MaterialButtonTone, interaction: Interaction) -> Color {
    match tone {
        MaterialButtonTone::Primary => {
            if interaction == Interaction::Hovered {
                Color::srgb(0.55, 0.86, 1.0)
            } else {
                PRIMARY
            }
        }
        MaterialButtonTone::Danger => Color::srgba(1.0, 0.37, 0.40, 0.84),
        _ => {
            if interaction == Interaction::Hovered {
                PRIMARY
            } else {
                BORDER_NORMAL
            }
        }
    }
}

fn action_button(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    action: MenuAction,
    width: f32,
    font: &Handle<Font>,
) {
    action_button_sized(parent, title, action, px(width), font);
}

fn action_button_sized<'a>(
    parent: &'a mut ChildSpawnerCommands,
    title: &str,
    action: MenuAction,
    width: Val,
    font: &Handle<Font>,
) -> EntityCommands<'a> {
    let tone = action_tone(action);
    let mut button = parent.spawn((
        Button,
        action,
        tone,
        Node {
            width,
            height: px(54),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            padding: UiRect::horizontal(px(20)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::ZERO,
            ..default()
        },
        BackgroundColor(tone_color(tone, Interaction::None)),
        BorderColor::all(tone_border(tone, Interaction::None)),
        control_shadow(),
    ));
    button.with_children(|button| {
        let label_color = TEXT_MAIN;
        button.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: px(4),
                height: percent(100.0),
                ..default()
            },
            BackgroundColor(if tone == MaterialButtonTone::Danger {
                Color::srgb(1.0, 0.37, 0.40)
            } else {
                PRIMARY
            }),
        ));
        let mut label = button.spawn((
            Text::new(title),
            menu_font(font, 15.0),
            TextColor(label_color),
        ));
        if matches!(action, MenuAction::Resume) {
            label.insert(MenuCopy::PrimaryAction);
        }
        button.spawn((
            Text::new(if matches!(action, MenuAction::Back) {
                "‹"
            } else {
                "›"
            }),
            menu_font(font, 25.0),
            TextColor(label_color),
        ));
    });
    button
}

fn radio_control_button(
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
                width: px(200),
                height: px(44),
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

fn setting_row(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    kind: SettingKind,
    font: &Handle<Font>,
) {
    parent
        .spawn((
            Node {
                width: percent(100.0),
                height: px(52),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::horizontal(px(12)),
                margin: UiRect::vertical(px(2)),
                border_radius: BorderRadius::ZERO,
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(SURFACE_LOW),
            BorderColor::all(Color::srgba(0.23, 0.32, 0.43, 0.40)),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(title),
                menu_font(font, 15.0),
                TextColor(TEXT_MUTED),
            ));
            if matches!(
                kind,
                SettingKind::EffectsVolume | SettingKind::EngineVolume | SettingKind::MusicVolume
            ) {
                setting_slider(row, kind, font);
            } else if is_boolean_setting(kind) {
                setting_toggle(row, kind, font);
            } else {
                setting_select(row, kind, font);
            }
        });
}

fn is_boolean_setting(kind: SettingKind) -> bool {
    matches!(
        kind,
        SettingKind::Vsync
            | SettingKind::ContactShadows
            | SettingKind::Bloom
            | SettingKind::MotionBlur
            | SettingKind::CollisionEffect
            | SettingKind::RearviewMirrors
            | SettingKind::TireMarks
            | SettingKind::TireSmoke
            | SettingKind::Sharpening
    )
}

fn setting_toggle(parent: &mut ChildSpawnerCommands, kind: SettingKind, font: &Handle<Font>) {
    parent
        .spawn((
            kind,
            Node {
                width: px(210),
                height: px(34),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexEnd,
                ..default()
            },
        ))
        .with_children(|container| {
            container
                .spawn((
                    Button,
                    SettingSelectButton {
                        kind,
                        direction: SelectDirection::Next,
                    },
                    SettingToggleButton(kind),
                    Node {
                        width: px(82),
                        height: px(32),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::FlexStart,
                        padding: UiRect::horizontal(px(10)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(16)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.08, 0.11, 0.16, 0.92)),
                    BorderColor::all(BORDER_NORMAL),
                ))
                .with_children(|toggle| {
                    toggle.spawn((
                        SettingToggleThumb(kind),
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(4),
                            top: px(3),
                            width: px(24),
                            height: px(24),
                            border_radius: BorderRadius::all(px(12)),
                            ..default()
                        },
                        BackgroundColor(TEXT_MUTED),
                    ));
                    toggle.spawn((
                        Text::new("OFF"),
                        SettingValueText(kind),
                        menu_font(font, 11.0),
                        TextColor(TEXT_MAIN),
                    ));
                });
        });
}

fn setting_select(parent: &mut ChildSpawnerCommands, kind: SettingKind, font: &Handle<Font>) {
    parent
        .spawn((
            kind,
            Node {
                width: px(210),
                height: px(34),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::ZERO,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(SURFACE),
            BorderColor::all(BORDER_NORMAL),
        ))
        .with_children(|select| {
            for (direction, glyph) in [
                (SelectDirection::Previous, "‹"),
                (SelectDirection::Next, "›"),
            ] {
                let mut button = select.spawn((
                    Button,
                    SettingSelectButton { kind, direction },
                    MaterialButtonTone::Icon,
                    Node {
                        width: px(38),
                        height: percent(100.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ));
                button.with_child((
                    Text::new(glyph),
                    menu_font(font, 22.0),
                    TextColor(TEXT_MAIN),
                ));
            }
            select
                .spawn((
                    SettingSelectValueSlot,
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(38),
                        right: px(38),
                        top: px(0),
                        bottom: px(0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                ))
                .with_child((
                    Text::new("—"),
                    SettingValueText(kind),
                    menu_font(font, 13.0),
                    TextColor(TEXT_MAIN),
                    Node {
                        align_self: AlignSelf::Center,
                        ..default()
                    },
                ));
        });
}

fn setting_slider(parent: &mut ChildSpawnerCommands, kind: SettingKind, font: &Handle<Font>) {
    parent
        .spawn((
            kind,
            Node {
                width: px(210),
                height: px(34),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(10),
                ..default()
            },
        ))
        .with_children(|control| {
            control.spawn((
                Text::new("—"),
                SettingValueText(kind),
                menu_font(font, 13.0),
                TextColor(TEXT_MAIN),
                Node {
                    width: px(48),
                    justify_content: JustifyContent::FlexEnd,
                    ..default()
                },
            ));
            control
                .spawn((
                    Button,
                    SettingSlider(kind),
                    RelativeCursorPosition::default(),
                    Node {
                        width: px(152),
                        height: px(14),
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::ZERO,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(SURFACE),
                    BorderColor::all(BORDER_NORMAL),
                ))
                .with_child((
                    SettingSliderFill(kind),
                    Node {
                        width: percent(0.0),
                        height: percent(100.0),
                        border_radius: BorderRadius::ZERO,
                        ..default()
                    },
                    BackgroundColor(PRIMARY),
                ));
        });
}

fn column_header(parent: &mut ChildSpawnerCommands, title: &str, font: &Handle<Font>) {
    parent.spawn((
        Text::new(title),
        menu_font(font, 14.0),
        TextColor(PRIMARY),
        Node {
            margin: UiRect::axes(px(10), px(8)),
            ..default()
        },
    ));
}

fn key_label(key: KeyCode) -> String {
    let debug = format!("{key:?}");
    match debug.as_str() {
        "Space" => "SPACE".to_string(),
        "Escape" => "ESC".to_string(),
        "Enter" => "ENTER".to_string(),
        "Tab" => "TAB".to_string(),
        "Backspace" => "BACKSPACE".to_string(),
        "Delete" => "DELETE".to_string(),
        "Insert" => "INSERT".to_string(),
        "Home" => "HOME".to_string(),
        "End" => "END".to_string(),
        "PageUp" => "PAGE UP".to_string(),
        "PageDown" => "PAGE DOWN".to_string(),
        "ArrowUp" => "↑".to_string(),
        "ArrowDown" => "↓".to_string(),
        "ArrowLeft" => "←".to_string(),
        "ArrowRight" => "→".to_string(),
        "ShiftLeft" => "L-SHIFT".to_string(),
        "ShiftRight" => "R-SHIFT".to_string(),
        "ControlLeft" => "L-CTRL".to_string(),
        "ControlRight" => "R-CTRL".to_string(),
        "AltLeft" => "L-ALT".to_string(),
        "AltRight" => "R-ALT".to_string(),
        "SuperLeft" => "L-CMD".to_string(),
        "SuperRight" => "R-CMD".to_string(),
        "Comma" => "<".to_string(),
        "Period" => ">".to_string(),
        _ if debug.starts_with("Key") => debug[3..].to_string(),
        _ if debug.starts_with("Digit") => debug[5..].to_string(),
        _ => debug,
    }
}

fn slot_label(slot: KeySlot) -> &'static str {
    match slot {
        KeySlot::Primary => "primary",
        KeySlot::Alternate => "alternate",
    }
}

fn binding_button(
    parent: &mut ChildSpawnerCommands,
    action: ControlAction,
    slot: KeySlot,
    font: &Handle<Font>,
) {
    parent
        .spawn((
            Button,
            ControlBindingButton { action, slot },
            Node {
                width: px(128),
                height: px(32),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::ZERO,
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(BORDER_NORMAL),
            control_shadow(),
        ))
        .with_child((
            Text::new("—"),
            ControlBindingValueText { action, slot },
            menu_font(font, 13.0),
            TextColor(TEXT_MAIN),
        ));
}

fn control_row(
    parent: &mut ChildSpawnerCommands,
    action: ControlAction,
    regular_font: &Handle<Font>,
    bold_font: &Handle<Font>,
) {
    parent
        .spawn((
            Node {
                width: percent(100.0),
                height: px(45),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::horizontal(px(12)),
                margin: UiRect::vertical(px(1)),
                border_radius: BorderRadius::ZERO,
                ..default()
            },
            BackgroundColor(SURFACE_LOW),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(action.label()),
                menu_font(regular_font, 15.0),
                TextColor(TEXT_MUTED),
                Node {
                    width: px(235),
                    ..default()
                },
            ));
            row.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(8),
                ..default()
            })
            .with_children(|bindings| {
                binding_button(bindings, action, KeySlot::Primary, bold_font);
                binding_button(bindings, action, KeySlot::Alternate, regular_font);
            });
        });
}

pub(crate) fn spawn_menu(
    mut commands: Commands,
    asset_server: Option<Res<AssetServer>>,
    layout: Option<Res<crate::ui::UiLayout>>,
) {
    // The real game always has an AssetServer. Keeping this optional makes the
    // menu's small ECS unit tests renderable without booting the full asset
    // pipeline, while the running game still receives the bundled Ubuntu font.
    let regular_font: Handle<Font> = asset_server
        .as_ref()
        .map(|server| server.load(MENU_FONT_REGULAR))
        .unwrap_or_default();
    let bold_font: Handle<Font> = asset_server
        .as_ref()
        .map(|server| server.load(MENU_FONT_BOLD))
        .unwrap_or_default();
    let main_logo: Handle<Image> = asset_server
        .as_ref()
        .map(|server| server.load("ui/onlygaz_logo.png"))
        .unwrap_or_default();

    #[cfg(feature = "mobile")]
    if layout.as_deref() == Some(&crate::ui::UiLayout::Phone) {
        phone::spawn_phone_menu(&mut commands, &regular_font, &bold_font, &main_logo);
        return;
    }
    #[cfg(not(feature = "mobile"))]
    let _ = layout;

    commands
        .spawn((
            Name::new("PauseOverlay"),
            PauseOverlay,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.0),
                height: percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(SCRIM),
            Visibility::Hidden,
            ZIndex(100),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    MainMenuPanel,
                    Node {
                        width: px(930),
                        max_width: percent(94.0),
                        min_height: px(548),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(18),
                        padding: UiRect::axes(px(30), px(28)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::ZERO,
                        ..default()
                    },
                    BackgroundColor(SURFACE),
                    BorderColor::all(Color::srgba(0.39, 0.65, 0.82, 0.42)),
                    panel_shadow(),
                ))
                .with_children(|panel| {
                    accent_bar(panel);
                    panel.spawn((
                        Text::new("ONLYGAZ  /  SESSION 01"),
                        MenuCopy::Status,
                        menu_font(&bold_font, 13.0),
                        TextColor(PRIMARY),
                    ));
                    panel.spawn(Node {
                        width: percent(100.0),
                        flex_grow: 1.0,
                        column_gap: px(24),
                        ..default()
                    })
                    .with_children(|content| {
                        content.spawn((
                            Node {
                                width: percent(44.0),
                                min_height: px(430),
                                flex_direction: FlexDirection::Column,
                                justify_content: JustifyContent::SpaceBetween,
                                padding: UiRect::all(px(24)),
                                border_radius: BorderRadius::ZERO,
                                border: UiRect::all(px(1)),
                                ..default()
                            },
                            BackgroundColor(SURFACE_LOW),
                            BorderColor::all(Color::srgba(0.24, 0.38, 0.52, 0.60)),
                        ))
                        .with_children(|hero| {
                            hero.spawn((
                                Text::new("DRIVE / PAUSED"),
                                MenuCopy::HeroLabel,
                                menu_font(&bold_font, 13.0),
                                TextColor(PRIMARY),
                            ));
                            hero.spawn((
                                Name::new("OnlyGAZ main menu logo"),
                                MainMenuLogo,
                                ImageNode::new(main_logo.clone()),
                                Node {
                                    width: px(246),
                                    height: px(164),
                                    align_self: AlignSelf::Center,
                                    ..default()
                                },
                            ));
                            hero.spawn((
                                Text::new("THE ROAD\nAWAITS"),
                                MenuCopy::HeroTitle,
                                menu_font(&bold_font, 48.0),
                                TextColor(TEXT_MAIN),
                            ));
                            hero.spawn((
                                Text::new("Tune the experience, then return to the track."),
                                MenuCopy::HeroSubtitle,
                                menu_font(&regular_font, 16.0),
                                TextColor(TEXT_MUTED),
                            ));
                            hero.spawn((
                                Text::new("ENVIRONMENT"),
                                menu_font(&bold_font, 12.0),
                                TextColor(PRIMARY),
                            ));
                            setting_row(hero, "Time of day", SettingKind::TimeOfDay, &regular_font);
                        });
                        content.spawn((
                            MenuActionColumn,
                            Node {
                                width: percent(53.0),
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                row_gap: px(10),
                                ..default()
                            },
                        ))
                        .with_children(|actions| {
                            actions.spawn((
                                Text::new("SESSION CONTROLS"),
                                MenuCopy::ActionsLabel,
                                menu_font(&bold_font, 12.0),
                                TextColor(TEXT_MUTED),
                            ));
                            action_button(actions, "RESUME DRIVE", MenuAction::Resume, 400.0, &bold_font);
                            action_button(actions, "GRAPHICS", MenuAction::OpenGraphics, 400.0, &bold_font);
                            action_button(actions, "CONTROLS", MenuAction::OpenControls, 400.0, &bold_font);
                            action_button(actions, "AUDIO", MenuAction::OpenAudio, 400.0, &bold_font);
                            action_button(actions, "QUIT GAME", MenuAction::Quit, 400.0, &bold_font);
                        });
                    });
                    panel.spawn((
                        Text::new("ESC  RESUME    •    C  CAMERA    •    F3  TELEMETRY"),
                        MenuCopy::Footer,
                        menu_font(&regular_font, 12.0),
                        TextColor(TEXT_MUTED),
                    ));
                });

            overlay
                .spawn((
                    GraphicsMenuPanel,
                    Node {
                        width: px(1080),
                        height: percent(92.0),
                        max_height: px(760),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: px(10),
                        padding: UiRect::axes(px(32), px(22)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::ZERO,
                        ..default()
                    },
                    BackgroundColor(SURFACE),
                    BorderColor::all(Color::srgba(0.52, 0.68, 0.86, 0.30)),
                    panel_shadow(),
                    Visibility::Hidden,
                ))
                .with_children(|panel| {
                    page_header(panel, "VISUAL EXPERIENCE", "GRAPHICS SETTINGS", "Choose a starting preset, then build your own graphics configuration.", &bold_font, &regular_font);
                    panel.spawn((
                        Text::new("QUALITY PRESET"),
                        menu_font(&regular_font, 13.0),
                        TextColor(TEXT_MUTED),
                    ));
                    panel
                        .spawn(Node {
                            width: percent(100.0),
                            justify_content: JustifyContent::Center,
                            column_gap: px(12),
                            ..default()
                        })
                        .with_children(|row| {
                            for quality in LightingQuality::ALL {
                                row.spawn((
                                    Button,
                                    GraphicsQualityButton(quality),
                                    Node {
                                        width: px(285),
                                        height: px(48),
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
                                    menu_font(&bold_font, 13.0),
                                    TextColor(TEXT_MAIN),
                                ));
                            }
                        });
                    panel.spawn((
                        Text::new(quality_description(LightingQuality::Performance)),
                        GraphicsQualityDescription,
                        menu_font(&regular_font, 13.0),
                        TextColor(TEXT_MUTED),
                    ));

                    panel
                        .spawn((GraphicsScroll, ScrollPosition::default(), Node {
                            width: percent(100.0),
                            flex_grow: 1.0,
                            flex_basis: px(0),
                            min_height: px(0),
                            overflow: Overflow::scroll_y(),
                            column_gap: px(34),
                            justify_content: JustifyContent::Center,
                            ..default()
                        }))
                        .with_children(|columns| {
                            columns
                                .spawn(Node {
                                    width: percent(48.0),
                                    flex_direction: FlexDirection::Column,
                                    ..default()
                                })
                                .with_children(|column| {
                                    column_header(column, "DISPLAY & WORLD", &bold_font);
                                    setting_row(column, "Resolution", SettingKind::Resolution, &regular_font);
                                    setting_row(column, "Display mode", SettingKind::DisplayMode, &regular_font);
                                    setting_row(column, "V-Sync", SettingKind::Vsync, &regular_font);
                                    setting_row(column, "Anti-aliasing", SettingKind::AntiAliasing, &regular_font);
                                    setting_row(column, "View distance", SettingKind::ViewDistance, &regular_font);
                                    setting_row(column, "Traffic density", SettingKind::TrafficDensity, &regular_font);
                                    setting_row(column, "Shadow detail", SettingKind::Shadows, &regular_font);
                                    setting_row(column, "Contact shadows", SettingKind::ContactShadows, &regular_font);
                                });
                            columns
                                .spawn(Node {
                                    width: percent(48.0),
                                    flex_direction: FlexDirection::Column,
                                    ..default()
                                })
                                .with_children(|column| {
                                    column_header(column, "EFFECTS & POST-PROCESSING", &bold_font);
                                    setting_row(column, "Speed & nitro FX", SettingKind::SpeedEffect, &regular_font);
                                    setting_row(column, "Rear-view mirrors", SettingKind::RearviewMirrors, &regular_font);
                                    setting_row(column, "Tyre marks", SettingKind::TireMarks, &regular_font);
                                    setting_row(column, "Tyre smoke", SettingKind::TireSmoke, &regular_font);
                                    setting_row(column, "Ambient occlusion (SSAO)", SettingKind::AmbientOcclusion, &regular_font);
                                    setting_row(column, "Reflections", SettingKind::Reflections, &regular_font);
                                    setting_row(column, "Atmospheric haze", SettingKind::Atmosphere, &regular_font);
                                    setting_row(column, "Bloom", SettingKind::Bloom, &regular_font);
                                    setting_row(column, "Motion blur", SettingKind::MotionBlur, &regular_font);
                                    setting_row(column, "Collision blur", SettingKind::CollisionEffect, &regular_font);
                                    setting_row(column, "Sharpening", SettingKind::Sharpening, &regular_font);
                                    setting_row(column, "Exposure", SettingKind::Exposure, &regular_font);
                                });
                        });
                    action_button(panel, "BACK", MenuAction::Back, 250.0, &bold_font);
                    panel.spawn((
                        Text::new("SCROLL  Mouse wheel / trackpad    •    Changes apply immediately and are saved"),
                        menu_font(&regular_font, 12.0),
                        TextColor(TEXT_MUTED),
                    ));
                });

            overlay
                .spawn((
                    ControlsMenuPanel,
                    Node {
                        width: px(1080),
                        height: percent(92.0),
                        max_height: px(760),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: px(10),
                        padding: UiRect::axes(px(32), px(22)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::ZERO,
                        ..default()
                    },
                    BackgroundColor(SURFACE),
                    BorderColor::all(Color::srgba(0.52, 0.68, 0.86, 0.30)),
                    panel_shadow(),
                    Visibility::Hidden,
                ))
                .with_children(|panel| {
                    page_header(panel, "INPUT CONFIGURATION", "CONTROLS", "Assign a primary and alternate key to each action.", &bold_font, &regular_font);
                    panel.spawn((
                        Text::new("KEY BINDINGS"),
                        menu_font(&regular_font, 13.0),
                        TextColor(TEXT_MUTED),
                    ));
                    panel
                        .spawn(Node {
                            width: percent(100.0),
                            justify_content: JustifyContent::SpaceBetween,
                            padding: UiRect::horizontal(px(4)),
                            ..default()
                        })
                        .with_children(|row| {
                            row.spawn((
                                Text::new("ACTION"),
                                menu_font(&bold_font, 12.0),
                                TextColor(PRIMARY),
                            ));
                            row.spawn(Node {
                                width: px(264),
                                ..default()
                            })
                            .with_children(|slots| {
                                slots.spawn((
                                    Text::new("PRIMARY"),
                                    menu_font(&bold_font, 12.0),
                                    TextColor(PRIMARY),
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: px(0),
                                        ..default()
                                    },
                                ));
                                slots.spawn((
                                    Text::new("ALTERNATE"),
                                    menu_font(&bold_font, 12.0),
                                    TextColor(PRIMARY),
                                    Node {
                                        position_type: PositionType::Absolute,
                                        right: px(0),
                                        ..default()
                                    },
                                ));
                            });
                        });
                    panel
                        .spawn(Node {
                            width: percent(100.0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Row,
                            column_gap: px(28),
                            ..default()
                        })
                        .with_children(|columns| {
                            columns
                                .spawn(Node {
                                    width: percent(50.0),
                                    flex_direction: FlexDirection::Column,
                                    ..default()
                                })
                                .with_children(|column| {
                                    for action in ControlAction::ALL.into_iter().take(11) {
                                        control_row(column, action, &regular_font, &bold_font);
                                    }
                                });
                            columns
                                .spawn(Node {
                                    width: percent(50.0),
                                    flex_direction: FlexDirection::Column,
                                    ..default()
                                })
                                .with_children(|column| {
                                    for action in ControlAction::ALL.into_iter().skip(11) {
                                        control_row(column, action, &regular_font, &bold_font);
                                    }
                                });
                        });
                    panel.spawn((
                        Text::new("Select a slot, then press a new key. ESC cancels rebinding."),
                        RebindPromptText,
                        menu_font(&regular_font, 12.0),
                        TextColor(TEXT_MUTED),
                    ));
                    action_button(panel, "BACK", MenuAction::Back, 250.0, &bold_font);
                });

            overlay
                .spawn((
                    AudioMenuPanel,
                    Node {
                        width: px(720),
                        height: px(650),
                        max_height: percent(92.0),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: px(12),
                        padding: UiRect::axes(px(42), px(30)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::ZERO,
                        ..default()
                    },
                    BackgroundColor(SURFACE),
                    BorderColor::all(Color::srgba(0.52, 0.68, 0.86, 0.30)),
                    panel_shadow(),
                    Visibility::Hidden,
                ))
                .with_children(|panel| {
                    page_header(panel, "SOUND MIXER", "AUDIO SETTINGS", "Balance the race soundtrack to your preference.", &bold_font, &regular_font);
                    panel.spawn((
                        Text::new("CHANNEL VOLUME"),
                        menu_font(&regular_font, 13.0),
                        TextColor(TEXT_MUTED),
                    ));
                    setting_row(
                        panel,
                        "Effects volume",
                        SettingKind::EffectsVolume,
                        &regular_font,
                    );
                    setting_row(
                        panel,
                        "Engine volume",
                        SettingKind::EngineVolume,
                        &regular_font,
                    );
                    setting_row(
                        panel,
                        "Music volume",
                        SettingKind::MusicVolume,
                        &regular_font,
                    );
                    panel.spawn((
                        Text::new("IN-CAR RADIO"),
                        menu_font(&regular_font, 13.0),
                        TextColor(PRIMARY),
                    ));
                    panel.spawn((
                        Text::new("Radio starts while driving"),
                        CurrentRadioTrackText,
                        menu_font(&bold_font, 17.0),
                        TextColor(TEXT_MAIN),
                    ));
                    panel
                        .spawn(Node {
                            width: percent(100.0),
                            justify_content: JustifyContent::Center,
                            column_gap: px(12),
                            ..default()
                        })
                        .with_children(|row| {
                            radio_control_button(row, "‹  PREVIOUS", RadioSkip::Previous, &bold_font);
                            radio_control_button(row, "NEXT  ›", RadioSkip::Next, &bold_font);
                        });
                    panel.spawn((
                        Text::new("9 / 0 change tracks while driving. Reassign them in Controls."),
                        menu_font(&regular_font, 12.0),
                        TextColor(TEXT_MUTED),
                    ));
                    action_button(panel, "BACK", MenuAction::Back, 250.0, &bold_font);
                });
        });
}

pub(crate) fn handle_menu_buttons(
    state: Res<State<GameState>>,
    mut page: ResMut<PauseMenuPage>,
    mut rebind: ResMut<RebindState>,
    mut next_state: ResMut<NextState<GameState>>,
    mut exit: MessageWriter<AppExit>,
    buttons: Query<(&Interaction, &MenuAction), (Changed<Interaction>, With<Button>)>,
) {
    if !menu_is_visible(*state.get()) {
        return;
    }
    for (interaction, action) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        // Leaving the controls page must cancel an in-progress capture, or a
        // later gameplay key could remain swallowed by the rebinding system.
        rebind.pending = None;
        match action {
            MenuAction::Resume => next_state.set(if *state.get() == GameState::MainMenu {
                GameState::ModeSelection
            } else {
                GameState::Driving
            }),
            MenuAction::OpenGraphics => *page = PauseMenuPage::Graphics,
            MenuAction::OpenControls => *page = PauseMenuPage::Controls,
            MenuAction::OpenAudio => *page = PauseMenuPage::Audio,
            MenuAction::Back => *page = PauseMenuPage::Main,
            MenuAction::Quit => {
                exit.write(AppExit::Success);
            }
            MenuAction::MainMenu => {
                *page = PauseMenuPage::Main;
                next_state.set(GameState::MainMenu);
            }
        }
    }
}

pub(crate) fn handle_graphics_quality_buttons(
    state: Res<State<GameState>>,
    mut settings: ResMut<LightingSettings>,
    buttons: Query<(&Interaction, &GraphicsQualityButton), (Changed<Interaction>, With<Button>)>,
) {
    if !menu_is_visible(*state.get()) {
        return;
    }
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed {
            settings.apply_preset(button.0);
            info!("graphics preset selected: {}", button.0.label());
        }
    }
}

pub(crate) fn handle_setting_buttons(
    state: Res<State<GameState>>,
    mut display: ResMut<DisplaySettings>,
    mut lighting: ResMut<LightingSettings>,
    mut traffic: ResMut<TrafficSettings>,
    mut touch: Option<ResMut<TouchControlSettings>>,
    buttons: Query<(&Interaction, &SettingSelectButton), (Changed<Interaction>, With<Button>)>,
) {
    if !menu_is_visible(*state.get()) {
        return;
    }
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let previous = button.direction == SelectDirection::Previous;
        match button.kind {
            SettingKind::Resolution => {
                display.resolution = if previous {
                    previous_resolution(display.resolution)
                } else {
                    display.resolution.next()
                };
            }
            SettingKind::DisplayMode => display.mode = display.mode.next(),
            SettingKind::Vsync => display.vsync = !display.vsync,
            SettingKind::AntiAliasing => {
                let step = |value: AntiAliasingQuality| {
                    if previous {
                        previous_anti_aliasing(value)
                    } else {
                        value.next()
                    }
                };
                let mut value = step(lighting.anti_aliasing);
                // The phone build (the only one with touch settings) skips
                // 2x MSAA, which many mobile GPUs do not support.
                if touch.is_some() && value == AntiAliasingQuality::Msaa2 {
                    value = step(value);
                }
                lighting.anti_aliasing = value;
            }
            SettingKind::ViewDistance => {
                lighting.view_distance = if previous {
                    previous_view_distance(lighting.view_distance)
                } else {
                    lighting.view_distance.next()
                };
            }
            SettingKind::TrafficDensity => {
                traffic.density = if previous {
                    traffic.density.previous()
                } else {
                    traffic.density.next()
                };
            }
            SettingKind::Shadows => {
                lighting.shadow_quality = cycle_graphics_level(lighting.shadow_quality, previous);
            }
            SettingKind::SpeedEffect => {
                lighting.speed_effect = cycle_graphics_level(lighting.speed_effect, previous);
            }
            SettingKind::AmbientOcclusion => {
                lighting.ambient_occlusion =
                    cycle_graphics_level(lighting.ambient_occlusion, previous);
            }
            SettingKind::ContactShadows => lighting.contact_shadows = !lighting.contact_shadows,
            SettingKind::Reflections => {
                lighting.reflection_quality =
                    cycle_graphics_level(lighting.reflection_quality, previous);
            }
            SettingKind::Atmosphere => {
                lighting.atmosphere_quality =
                    cycle_graphics_level(lighting.atmosphere_quality, previous);
            }
            SettingKind::Bloom => lighting.bloom = !lighting.bloom,
            SettingKind::RearviewMirrors => lighting.rearview_mirrors = !lighting.rearview_mirrors,
            SettingKind::TireMarks => lighting.tire_marks = !lighting.tire_marks,
            SettingKind::TireSmoke => lighting.tire_smoke = !lighting.tire_smoke,
            SettingKind::MotionBlur => lighting.motion_blur = !lighting.motion_blur,
            SettingKind::CollisionEffect => lighting.collision_effect = !lighting.collision_effect,
            SettingKind::Sharpening => lighting.sharpening = !lighting.sharpening,
            SettingKind::Exposure => {
                lighting.exposure_adjustment = if previous {
                    previous_exposure(lighting.exposure_adjustment)
                } else {
                    lighting.exposure_adjustment.next()
                };
            }
            SettingKind::TimeOfDay => lighting.time_of_day = lighting.time_of_day.next(),
            SettingKind::EffectsVolume | SettingKind::EngineVolume | SettingKind::MusicVolume => {}
            SettingKind::SteeringMode => {
                if let Some(touch) = touch.as_mut() {
                    touch.steering = touch.steering.next();
                }
            }
            SettingKind::TiltSensitivity => {
                if let Some(touch) = touch.as_mut() {
                    touch.tilt_sensitivity = if previous {
                        touch.tilt_sensitivity.previous()
                    } else {
                        touch.tilt_sensitivity.next()
                    };
                }
            }
        }
    }
}

fn previous_resolution(value: ResolutionPreset) -> ResolutionPreset {
    match value {
        ResolutionPreset::Hd => ResolutionPreset::Uhd,
        ResolutionPreset::HdPlus => ResolutionPreset::Hd,
        ResolutionPreset::FullHd => ResolutionPreset::HdPlus,
        ResolutionPreset::Qhd => ResolutionPreset::FullHd,
        ResolutionPreset::Uhd => ResolutionPreset::Qhd,
    }
}

fn previous_anti_aliasing(value: AntiAliasingQuality) -> AntiAliasingQuality {
    match value {
        AntiAliasingQuality::Off => AntiAliasingQuality::Taa,
        AntiAliasingQuality::Msaa2 => AntiAliasingQuality::Off,
        AntiAliasingQuality::Msaa4 => AntiAliasingQuality::Msaa2,
        AntiAliasingQuality::Taa => AntiAliasingQuality::Msaa4,
    }
}

fn previous_view_distance(value: ViewDistance) -> ViewDistance {
    match value {
        ViewDistance::Near => ViewDistance::Ultra,
        ViewDistance::Medium => ViewDistance::Near,
        ViewDistance::Far => ViewDistance::Medium,
        ViewDistance::Ultra => ViewDistance::Far,
    }
}

fn cycle_graphics_level(value: GraphicsLevel, previous: bool) -> GraphicsLevel {
    if !previous {
        return value.next();
    }
    match value {
        GraphicsLevel::Off => GraphicsLevel::Ultra,
        GraphicsLevel::Low => GraphicsLevel::Off,
        GraphicsLevel::Medium => GraphicsLevel::Low,
        GraphicsLevel::High => GraphicsLevel::Medium,
        GraphicsLevel::Ultra => GraphicsLevel::High,
    }
}

fn previous_exposure(value: ExposureAdjustment) -> ExposureAdjustment {
    match value {
        ExposureAdjustment::Darker => ExposureAdjustment::Brighter,
        ExposureAdjustment::Normal => ExposureAdjustment::Darker,
        ExposureAdjustment::Brighter => ExposureAdjustment::Normal,
    }
}

fn slider_value_from_cursor(position: &RelativeCursorPosition) -> Option<f32> {
    position
        .normalized
        .map(|cursor| (cursor.x + 0.5).clamp(0.0, 1.0))
}

pub(crate) fn handle_setting_sliders(
    state: Res<State<GameState>>,
    mut audio: ResMut<crate::audio::AudioSettings>,
    sliders: Query<(&Interaction, &RelativeCursorPosition, &SettingSlider), With<Button>>,
) {
    if !menu_is_visible(*state.get()) {
        return;
    }
    for (interaction, cursor, slider) in &sliders {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(value) = slider_value_from_cursor(cursor) else {
            continue;
        };
        // Percent precision feels continuous while keeping persisted values
        // stable and avoiding insignificant floating-point changes every frame.
        let value = (value * 100.0).round() / 100.0;
        match slider.0 {
            SettingKind::EffectsVolume => audio.effects_volume = value,
            SettingKind::EngineVolume => audio.engine_volume = value,
            SettingKind::MusicVolume => audio.music_volume = value,
            _ => {}
        }
    }
}

pub(crate) fn handle_rebind_buttons(
    state: Res<State<GameState>>,
    mut rebind: ResMut<RebindState>,
    buttons: Query<(&Interaction, &ControlBindingButton), (Changed<Interaction>, With<Button>)>,
) {
    if !menu_is_visible(*state.get()) {
        return;
    }
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        rebind.pending = Some(PendingRebind {
            action: button.action,
            slot: button.slot,
        });
        rebind.message = Some(format!(
            "Press a key for the {} binding of {}",
            slot_label(button.slot),
            button.action.label()
        ));
    }
}

pub(crate) fn handle_radio_buttons(
    state: Res<State<GameState>>,
    page: Res<PauseMenuPage>,
    mut radio: Option<ResMut<RadioPlaylist>>,
    buttons: Query<(&Interaction, &RadioControlButton), (Changed<Interaction>, With<Button>)>,
) {
    if *state.get() != GameState::Paused || *page != PauseMenuPage::Audio {
        return;
    }
    let Some(ref mut radio) = radio else {
        return;
    };
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed {
            radio.request_skip(button.0);
        }
    }
}

pub(crate) fn sync_radio_menu(
    state: Res<State<GameState>>,
    radio: Option<Res<RadioPlaylist>>,
    mut labels: Query<&mut Text, With<CurrentRadioTrackText>>,
    mut buttons: Query<&mut Node, With<RadioControlButton>>,
) {
    let paused = *state.get() == GameState::Paused;
    let value = if paused {
        radio
            .and_then(|radio| radio.current_title())
            .map(|title| format!("NOW PLAYING: {title}"))
            .unwrap_or_else(|| "Radio starts while driving".to_string())
    } else {
        "Radio starts while driving".to_string()
    };
    for mut label in &mut labels {
        if label.as_str() != value {
            **label = value.clone();
        }
    }
    for mut node in &mut buttons {
        node.display = if paused { Display::Flex } else { Display::None };
    }
}

pub(crate) fn apply_rebind_key(
    state: Res<State<GameState>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut bindings: ResMut<KeyBindings>,
    mut rebind: ResMut<RebindState>,
) {
    if !menu_is_visible(*state.get()) {
        return;
    }
    let Some(pending) = rebind.pending else {
        return;
    };
    let Some(&key) = keys.get_just_pressed().next() else {
        return;
    };

    let own_slot_conflict = match pending.slot {
        KeySlot::Primary => bindings.alternate(pending.action) == Some(key),
        KeySlot::Alternate => bindings.primary(pending.action) == key,
    };
    if own_slot_conflict || bindings.is_bound_to_other(pending.action, key) {
        rebind.message = Some(format!(
            "Key {} is already assigned. Choose another.",
            key_label(key)
        ));
        return;
    }

    bindings.set(pending.action, pending.slot, Some(key));
    rebind.pending = None;
    rebind.message = Some(format!(
        "Assigned: {} — {}",
        pending.action.label(),
        key_label(key)
    ));
}

pub(crate) fn apply_display_settings(
    settings: Res<DisplaySettings>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    monitors: Query<&Monitor, With<PrimaryMonitor>>,
) {
    if !settings.is_changed() {
        return;
    }
    for mut window in &mut windows {
        window.present_mode = if settings.vsync {
            PresentMode::AutoVsync
        } else {
            PresentMode::AutoNoVsync
        };
        window.mode = match settings.mode {
            DisplayModeSetting::Windowed => WindowMode::Windowed,
            DisplayModeSetting::Fullscreen => {
                let (width, height) = settings.resolution.size();
                let supported = monitors
                    .iter()
                    .flat_map(|monitor| monitor.video_modes.iter())
                    .filter(|mode| mode.physical_size.x == width && mode.physical_size.y == height)
                    .max_by_key(|mode| mode.refresh_rate_millihertz)
                    .copied();
                supported.map_or(
                    WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
                    |mode| {
                        WindowMode::Fullscreen(
                            MonitorSelection::Primary,
                            VideoModeSelection::Specific(mode),
                        )
                    },
                )
            }
        };
        if settings.mode == DisplayModeSetting::Windowed {
            let (width, height) = settings.resolution.size();
            window.resolution.set_physical_resolution(width, height);
        }
    }
}

fn setting_value(
    kind: SettingKind,
    display: &DisplaySettings,
    lighting: &LightingSettings,
    traffic: &TrafficSettings,
    audio: &crate::audio::AudioSettings,
    touch: Option<&TouchControlSettings>,
) -> String {
    match kind {
        SettingKind::Resolution => display.resolution.label().to_string(),
        SettingKind::DisplayMode => display.mode.label().to_string(),
        SettingKind::Vsync => toggle_label(display.vsync).to_string(),
        SettingKind::AntiAliasing => lighting.anti_aliasing.label().to_string(),
        SettingKind::ViewDistance => lighting.view_distance.label().to_string(),
        SettingKind::TrafficDensity => traffic.density.label().to_string(),
        SettingKind::Shadows => lighting.shadow_quality.label().to_string(),
        SettingKind::SpeedEffect => lighting.speed_effect.label().to_string(),
        SettingKind::AmbientOcclusion => lighting.ambient_occlusion.label().to_string(),
        SettingKind::ContactShadows => toggle_label(lighting.contact_shadows).to_string(),
        SettingKind::Reflections => lighting.reflection_quality.label().to_string(),
        SettingKind::Atmosphere => lighting.atmosphere_quality.label().to_string(),
        SettingKind::Bloom => toggle_label(lighting.bloom).to_string(),
        SettingKind::RearviewMirrors => toggle_label(lighting.rearview_mirrors).to_string(),
        SettingKind::TireMarks => toggle_label(lighting.tire_marks).to_string(),
        SettingKind::TireSmoke => toggle_label(lighting.tire_smoke).to_string(),
        SettingKind::MotionBlur => toggle_label(lighting.motion_blur).to_string(),
        SettingKind::CollisionEffect => toggle_label(lighting.collision_effect).to_string(),
        SettingKind::Sharpening => toggle_label(lighting.sharpening).to_string(),
        SettingKind::Exposure => lighting.exposure_adjustment.label().to_string(),
        SettingKind::TimeOfDay => lighting.time_of_day.label().to_string(),
        SettingKind::EffectsVolume => format_volume(audio.effects_volume),
        SettingKind::EngineVolume => format_volume(audio.engine_volume),
        SettingKind::MusicVolume => format_volume(audio.music_volume),
        SettingKind::SteeringMode => touch
            .map(|touch| touch.steering.label())
            .unwrap_or("—")
            .to_string(),
        SettingKind::TiltSensitivity => touch
            .map(|touch| touch.tilt_sensitivity.label())
            .unwrap_or("—")
            .to_string(),
    }
}

fn boolean_setting_value(
    kind: SettingKind,
    display: &DisplaySettings,
    lighting: &LightingSettings,
) -> Option<bool> {
    match kind {
        SettingKind::Vsync => Some(display.vsync),
        SettingKind::ContactShadows => Some(lighting.contact_shadows),
        SettingKind::Bloom => Some(lighting.bloom),
        SettingKind::RearviewMirrors => Some(lighting.rearview_mirrors),
        SettingKind::TireMarks => Some(lighting.tire_marks),
        SettingKind::TireSmoke => Some(lighting.tire_smoke),
        SettingKind::MotionBlur => Some(lighting.motion_blur),
        SettingKind::CollisionEffect => Some(lighting.collision_effect),
        SettingKind::Sharpening => Some(lighting.sharpening),
        _ => None,
    }
}

fn format_volume(value: f32) -> String {
    format!("{:.0}%", value.clamp(0.0, 1.0) * 100.0)
}

fn setting_slider_value(kind: SettingKind, audio: &crate::audio::AudioSettings) -> Option<f32> {
    match kind {
        SettingKind::EffectsVolume => Some(audio.effects_volume),
        SettingKind::EngineVolume => Some(audio.engine_volume),
        SettingKind::MusicVolume => Some(audio.music_volume),
        _ => None,
    }
}

pub(crate) fn sync_graphics_menu(
    display: Res<DisplaySettings>,
    lighting: Res<LightingSettings>,
    traffic: Res<TrafficSettings>,
    audio: Res<crate::audio::AudioSettings>,
    touch: Option<Res<TouchControlSettings>>,
    mut quality_buttons: Query<
        (
            &GraphicsQualityButton,
            &Interaction,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        (With<Button>, Without<SettingToggleButton>),
    >,
    mut values: Query<(&SettingValueText, &mut Text), Without<GraphicsQualityDescription>>,
    mut slider_fills: Query<
        (&SettingSliderFill, &mut Node),
        (Without<SettingToggleButton>, Without<SettingToggleThumb>),
    >,
    mut sliders: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        (
            With<SettingSlider>,
            Without<GraphicsQualityButton>,
            Without<SettingToggleButton>,
            Without<SettingToggleThumb>,
        ),
    >,
    mut toggles: Query<
        (
            &SettingToggleButton,
            &Interaction,
            &mut Node,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        (
            With<Button>,
            Without<SettingToggleThumb>,
            Without<SettingSliderFill>,
            Without<GraphicsQualityButton>,
        ),
    >,
    mut toggle_thumbs: Query<
        (&SettingToggleThumb, &mut Node, &mut BackgroundColor),
        (
            Without<SettingToggleButton>,
            Without<SettingSliderFill>,
            Without<GraphicsQualityButton>,
        ),
    >,
    mut descriptions: Query<&mut Text, With<GraphicsQualityDescription>>,
) {
    for (button, interaction, mut background, mut border) in &mut quality_buttons {
        let selected = button.0 == lighting.quality;
        background.0 = if selected {
            BUTTON_SELECTED
        } else if *interaction == Interaction::Hovered {
            BUTTON_HOVERED
        } else {
            BUTTON_NORMAL
        };
        *border = BorderColor::all(if selected {
            BORDER_SELECTED
        } else {
            BORDER_NORMAL
        });
    }
    for (kind, mut text) in &mut values {
        let value = setting_value(
            kind.0,
            &display,
            &lighting,
            &traffic,
            &audio,
            touch.as_deref(),
        );
        if text.as_str() != value {
            **text = value;
        }
    }
    for (fill, mut node) in &mut slider_fills {
        if let Some(value) = setting_slider_value(fill.0, &audio) {
            node.width = percent(value.clamp(0.0, 1.0) * 100.0);
        }
    }
    for (interaction, mut background, mut border) in &mut sliders {
        let active = *interaction != Interaction::None;
        background.0 = if active { SURFACE_BRIGHT } else { SURFACE };
        *border = BorderColor::all(if active { PRIMARY } else { BORDER_NORMAL });
    }
    for (toggle, interaction, mut node, mut background, mut border) in &mut toggles {
        let enabled = boolean_setting_value(toggle.0, &display, &lighting).unwrap_or(false);
        node.justify_content = if enabled {
            JustifyContent::FlexStart
        } else {
            JustifyContent::FlexEnd
        };
        background.0 = match (enabled, *interaction) {
            (true, Interaction::Pressed) => PRIMARY_PRESSED,
            (true, Interaction::Hovered) => PRIMARY_HOVER,
            (true, Interaction::None) => PRIMARY_CONTAINER,
            (false, Interaction::Hovered) => SURFACE_BRIGHT,
            (false, _) => Color::srgba(0.08, 0.11, 0.16, 0.92),
        };
        *border = BorderColor::all(if enabled || *interaction == Interaction::Hovered {
            PRIMARY
        } else {
            BORDER_NORMAL
        });
    }
    for (thumb, mut node, mut background) in &mut toggle_thumbs {
        let enabled = boolean_setting_value(thumb.0, &display, &lighting).unwrap_or(false);
        node.left = if enabled { px(53) } else { px(4) };
        background.0 = if enabled { Color::WHITE } else { TEXT_MUTED };
    }
    for mut description in &mut descriptions {
        let value = selected_quality_description(&lighting);
        if description.as_str() != value {
            **description = value;
        }
    }
}

pub(crate) fn sync_button_hover(
    mut buttons: Query<
        (
            &Interaction,
            Option<&MaterialButtonTone>,
            &mut BackgroundColor,
            Option<&mut BorderColor>,
        ),
        (
            With<Button>,
            Without<GraphicsQualityButton>,
            Without<ControlBindingButton>,
            Without<SettingSlider>,
            Without<SettingToggleButton>,
            Without<crate::ui::start_screen::StartAction>,
        ),
    >,
) {
    for (interaction, tone, mut background, border) in &mut buttons {
        background.0 = if let Some(tone) = tone {
            tone_color(*tone, *interaction)
        } else if *interaction == Interaction::Hovered {
            BUTTON_HOVERED
        } else {
            BUTTON_NORMAL
        };
        if let (Some(tone), Some(mut border)) = (tone, border) {
            *border = BorderColor::all(tone_border(*tone, *interaction));
        }
    }
}

pub(crate) fn sync_controls_menu(
    bindings: Res<KeyBindings>,
    rebind: Res<RebindState>,
    mut values: Query<(&ControlBindingValueText, &mut Text), Without<RebindPromptText>>,
    mut buttons: Query<(
        &ControlBindingButton,
        &Interaction,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut prompts: Query<&mut Text, (With<RebindPromptText>, Without<ControlBindingValueText>)>,
) {
    for (binding, mut text) in &mut values {
        let value = match binding.slot {
            KeySlot::Primary => key_label(bindings.primary(binding.action)),
            KeySlot::Alternate => bindings
                .alternate(binding.action)
                .map(key_label)
                .unwrap_or_else(|| "—".to_string()),
        };
        if text.as_str() != value {
            **text = value;
        }
    }

    for (button, interaction, mut background, mut border) in &mut buttons {
        let selected = rebind.pending
            == Some(PendingRebind {
                action: button.action,
                slot: button.slot,
            });
        background.0 = if selected {
            BUTTON_SELECTED
        } else if *interaction == Interaction::Hovered {
            BUTTON_HOVERED
        } else {
            BUTTON_NORMAL
        };
        *border = BorderColor::all(if selected {
            BORDER_SELECTED
        } else {
            BORDER_NORMAL
        });
    }

    let prompt = rebind
        .message
        .as_deref()
        .unwrap_or("Select a slot, then press a new key. ESC cancels rebinding.");
    for mut text in &mut prompts {
        if text.as_str() != prompt {
            **text = prompt.to_string();
        }
    }
}

pub(crate) fn sync_menu_visibility(
    state: Res<State<GameState>>,
    page: Res<PauseMenuPage>,
    mut overlays: Query<
        (&mut Visibility, &mut Node),
        (
            With<PauseOverlay>,
            Without<MainMenuPanel>,
            Without<GraphicsMenuPanel>,
            Without<ControlsMenuPanel>,
            Without<AudioMenuPanel>,
        ),
    >,
    mut main: Query<
        (&mut Visibility, &mut Node),
        (
            With<MainMenuPanel>,
            Without<PauseOverlay>,
            Without<GraphicsMenuPanel>,
            Without<ControlsMenuPanel>,
            Without<AudioMenuPanel>,
        ),
    >,
    mut graphics: Query<
        (&mut Visibility, &mut Node),
        (
            With<GraphicsMenuPanel>,
            Without<PauseOverlay>,
            Without<MainMenuPanel>,
            Without<ControlsMenuPanel>,
            Without<AudioMenuPanel>,
        ),
    >,
    mut controls: Query<
        (&mut Visibility, &mut Node),
        (
            With<ControlsMenuPanel>,
            Without<PauseOverlay>,
            Without<MainMenuPanel>,
            Without<GraphicsMenuPanel>,
            Without<AudioMenuPanel>,
        ),
    >,
    mut audio: Query<
        (&mut Visibility, &mut Node),
        (
            With<AudioMenuPanel>,
            Without<PauseOverlay>,
            Without<MainMenuPanel>,
            Without<GraphicsMenuPanel>,
            Without<ControlsMenuPanel>,
        ),
    >,
) {
    for (mut visibility, mut node) in &mut overlays {
        node.display = if menu_is_visible(*state.get()) {
            Display::Flex
        } else {
            Display::None
        };
        *visibility = if menu_is_visible(*state.get()) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    for (mut visibility, mut node) in &mut main {
        node.display = if *page == PauseMenuPage::Main {
            Display::Flex
        } else {
            Display::None
        };
        *visibility = if *page == PauseMenuPage::Main {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    for (mut visibility, mut node) in &mut graphics {
        node.display = if *page == PauseMenuPage::Graphics {
            Display::Flex
        } else {
            Display::None
        };
        *visibility = if *page == PauseMenuPage::Graphics {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    for (mut visibility, mut node) in &mut controls {
        node.display = if *page == PauseMenuPage::Controls {
            Display::Flex
        } else {
            Display::None
        };
        *visibility = if *page == PauseMenuPage::Controls {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    for (mut visibility, mut node) in &mut audio {
        node.display = if *page == PauseMenuPage::Audio {
            Display::Flex
        } else {
            Display::None
        };
        *visibility = if *page == PauseMenuPage::Audio {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

/// The launch menu and pause menu share the same layout, but use different
/// actions and copy. Keeping the copy in-place avoids a second overlay with
/// slightly different button positions.
pub(crate) fn sync_main_menu_copy(
    state: Res<State<GameState>>,
    mut copy: Query<(&MenuCopy, &mut Text)>,
    mut action_columns: Query<&mut Node, (With<MenuActionColumn>, Without<MainMenuLogo>)>,
    mut logos: Query<
        &mut Node,
        (
            With<MainMenuLogo>,
            Without<MenuActionColumn>,
            Without<Button>,
        ),
    >,
) {
    let launching = *state.get() == GameState::MainMenu;
    for mut node in &mut logos {
        node.display = if launching {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut action_columns {
        node.justify_content = if launching {
            JustifyContent::FlexStart
        } else {
            JustifyContent::Center
        };
    }
    for (slot, mut text) in &mut copy {
        let value = match (*slot, launching) {
            (MenuCopy::Status, true) => "ONLYGAZ  /  MAIN MENU",
            (MenuCopy::Status, false) => "ONLYGAZ  /  SESSION 01",
            (MenuCopy::HeroLabel, true) => "DRIVE / READY",
            (MenuCopy::HeroLabel, false) => "DRIVE / PAUSED",
            (MenuCopy::HeroTitle, true) => "START YOUR\nENGINE",
            (MenuCopy::HeroTitle, false) => "THE ROAD\nAWAITS",
            (MenuCopy::HeroSubtitle, true) => "Tune the experience, then press Play to start.",
            (MenuCopy::HeroSubtitle, false) => "Tune the experience, then return to the track.",
            (MenuCopy::ActionsLabel, true) => "MAIN MENU",
            (MenuCopy::ActionsLabel, false) => "SESSION CONTROLS",
            (MenuCopy::PrimaryAction, true) => "PLAY",
            (MenuCopy::PrimaryAction, false) => "RESUME DRIVE",
            (MenuCopy::Footer, true) => "PLAY  MODE  DRIVE    •    GRAPHICS  CONTROLS  AUDIO",
            (MenuCopy::Footer, false) => "ESC  RESUME    •    C  CAMERA    •    F3  TELEMETRY",
        };
        if text.as_str() != value {
            **text = value.into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_actions_have_clear_visual_hierarchy() {
        assert_eq!(action_tone(MenuAction::Resume), MaterialButtonTone::Primary);
        assert_eq!(action_tone(MenuAction::Quit), MaterialButtonTone::Danger);
        assert_eq!(
            action_tone(MenuAction::OpenGraphics),
            MaterialButtonTone::Tonal
        );
        assert_eq!(action_tone(MenuAction::Back), MaterialButtonTone::Outline);
        assert_ne!(
            tone_color(MaterialButtonTone::Primary, Interaction::None),
            tone_color(MaterialButtonTone::Primary, Interaction::Hovered)
        );
    }

    #[test]
    fn translucent_square_menu_style_preserves_panel_and_action_dimensions() {
        let mut app = App::new();
        app.add_systems(Startup, spawn_menu);
        app.update();
        let world = app.world_mut();
        let mut nodes = world.query::<(
            &Node,
            Option<&SettingToggleButton>,
            Option<&SettingToggleThumb>,
        )>();
        assert!(
            nodes
                .iter(world)
                .all(|(node, toggle, thumb)| toggle.is_some()
                    || thumb.is_some()
                    || node.border_radius == BorderRadius::ZERO),
            "only web-style ON/OFF switches may have rounded corners"
        );
        let panel = world
            .query_filtered::<&Node, With<MainMenuPanel>>()
            .single(world)
            .expect("main pause panel");
        assert_eq!(panel.width, px(930));
        let mut actions = world.query_filtered::<&Node, With<MenuAction>>();
        assert!(actions.iter(world).any(|node| node.width == px(400)));
        assert!(SURFACE.to_srgba().alpha < 0.85);
        assert!(SCRIM.to_srgba().alpha < 0.4);
    }

    #[test]
    fn only_the_selected_menu_page_participates_in_layout() {
        let mut app = App::new();
        app.insert_resource(State::new(GameState::Paused))
            .init_resource::<PauseMenuPage>()
            .add_systems(Startup, spawn_menu)
            .add_systems(Update, sync_menu_visibility);
        for page in [
            PauseMenuPage::Main,
            PauseMenuPage::Graphics,
            PauseMenuPage::Controls,
            PauseMenuPage::Audio,
        ] {
            *app.world_mut().resource_mut::<PauseMenuPage>() = page;
            app.update();
            let world = app.world_mut();
            let mut panels = world.query_filtered::<&Node, Or<(
                With<MainMenuPanel>,
                With<GraphicsMenuPanel>,
                With<ControlsMenuPanel>,
                With<AudioMenuPanel>,
            )>>();
            assert_eq!(
                panels
                    .iter(world)
                    .filter(|node| node.display != Display::None)
                    .count(),
                1
            );
        }
        app.insert_resource(State::new(GameState::Driving));
        app.update();
        let world = app.world_mut();
        let mut overlay = world.query_filtered::<&Node, With<PauseOverlay>>();
        assert_eq!(overlay.single(world).unwrap().display, Display::None);
    }

    #[test]
    fn pause_menu_exposes_every_graphics_preset() {
        let mut app = App::new();
        app.add_systems(Startup, spawn_menu);
        app.update();
        let world = app.world_mut();
        let mut query = world.query::<&GraphicsQualityButton>();
        let presets: Vec<_> = query.iter(world).map(|button| button.0).collect();
        assert_eq!(presets, LightingQuality::ALL);
    }

    #[test]
    fn graphics_menu_exposes_the_collision_effect_toggle() {
        let mut app = App::new();
        app.add_systems(Startup, spawn_menu);
        app.update();
        let world = app.world_mut();
        let mut settings = world.query::<&SettingKind>();
        assert_eq!(
            settings
                .iter(world)
                .filter(|kind| **kind == SettingKind::CollisionEffect)
                .count(),
            1
        );
    }

    #[test]
    fn graphics_menu_exposes_independent_tire_fx_toggles() {
        let mut app = App::new();
        app.add_systems(Startup, spawn_menu);
        app.update();
        let world = app.world_mut();
        let mut settings = world.query::<&SettingKind>();
        for kind in [SettingKind::TireMarks, SettingKind::TireSmoke] {
            assert_eq!(
                settings.iter(world).filter(|found| **found == kind).count(),
                1
            );
        }
    }

    #[test]
    fn performance_preset_allows_custom_tire_effects() {
        let mut app = App::new();
        app.insert_resource(State::new(GameState::Paused))
            .insert_resource(DisplaySettings::default())
            .insert_resource(LightingSettings::default())
            .insert_resource(TrafficSettings::default())
            .add_systems(Update, handle_setting_buttons);
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            SettingSelectButton {
                kind: SettingKind::TireSmoke,
                direction: SelectDirection::Next,
            },
        ));
        app.update();
        assert!(app.world().resource::<LightingSettings>().tire_smoke);
        assert!(
            !app.world()
                .resource::<LightingSettings>()
                .matches_selected_preset()
        );
        assert_eq!(
            setting_value(
                SettingKind::TireSmoke,
                app.world().resource::<DisplaySettings>(),
                app.world().resource::<LightingSettings>(),
                app.world().resource::<TrafficSettings>(),
                &crate::audio::AudioSettings::default(),
                None,
            ),
            "ON"
        );
    }

    #[test]
    fn every_optional_graphics_selector_is_editable_from_performance() {
        let mut app = App::new();
        app.insert_resource(State::new(GameState::Paused))
            .insert_resource(DisplaySettings::default())
            .insert_resource(LightingSettings::default())
            .insert_resource(TrafficSettings::default())
            .add_systems(Update, handle_setting_buttons);
        for kind in [
            SettingKind::SpeedEffect,
            SettingKind::RearviewMirrors,
            SettingKind::TireMarks,
            SettingKind::TireSmoke,
            SettingKind::CollisionEffect,
            SettingKind::AmbientOcclusion,
            SettingKind::ContactShadows,
            SettingKind::Reflections,
            SettingKind::Atmosphere,
            SettingKind::Bloom,
            SettingKind::MotionBlur,
        ] {
            app.world_mut().spawn((
                Button,
                Interaction::Pressed,
                SettingSelectButton {
                    kind,
                    direction: SelectDirection::Next,
                },
            ));
        }
        app.update();
        let settings = app.world().resource::<LightingSettings>();
        assert_eq!(settings.quality, LightingQuality::Performance);
        assert!(settings.speed_effect_strength() > 0.0);
        assert!(settings.mirrors_enabled());
        assert!(settings.tire_marks_enabled());
        assert!(settings.tire_smoke_enabled());
        assert!(settings.collision_effect_enabled());
        assert_ne!(settings.ambient_occlusion, GraphicsLevel::Off);
        assert!(settings.contact_shadows);
        assert!(settings.uses_reflections());
        assert_ne!(settings.atmosphere_quality, GraphicsLevel::Off);
        assert!(settings.bloom);
        assert!(settings.motion_blur);
        assert!(!settings.matches_selected_preset());
        assert!(selected_quality_description(settings).contains("CUSTOM CONFIGURATION"));
    }

    #[test]
    fn discrete_settings_use_selectors_and_volumes_use_sliders() {
        let mut app = App::new();
        app.add_systems(Startup, spawn_menu);
        app.update();
        let world = app.world_mut();

        let mut roots = world.query::<&SettingKind>();
        assert_eq!(roots.iter(world).count(), 24);

        let mut selectors =
            world.query_filtered::<&SettingSelectButton, Without<SettingToggleButton>>();
        assert_eq!(selectors.iter(world).count(), 12 * 2);
        assert!(selectors.iter(world).all(|button| !matches!(
            button.kind,
            SettingKind::EffectsVolume | SettingKind::EngineVolume | SettingKind::MusicVolume
        )));

        let mut toggles = world.query::<&SettingToggleButton>();
        assert_eq!(toggles.iter(world).count(), 9);
        assert!(
            toggles
                .iter(world)
                .all(|toggle| is_boolean_setting(toggle.0))
        );

        let mut sliders = world.query::<&SettingSlider>();
        let slider_kinds: Vec<_> = sliders.iter(world).map(|slider| slider.0).collect();
        assert_eq!(
            slider_kinds,
            [
                SettingKind::EffectsVolume,
                SettingKind::EngineVolume,
                SettingKind::MusicVolume,
            ]
        );
    }

    #[test]
    fn selectors_are_vertically_centered_and_boolean_controls_use_web_switches() {
        let mut app = App::new();
        app.insert_resource(DisplaySettings::default())
            .insert_resource(LightingSettings::default())
            .insert_resource(TrafficSettings::default())
            .insert_resource(crate::audio::AudioSettings::default())
            .add_systems(Startup, spawn_menu)
            .add_systems(Update, sync_graphics_menu);
        app.update();
        let world = app.world_mut();

        let mut slots = world.query_filtered::<&Node, With<SettingSelectValueSlot>>();
        assert!(slots.iter(world).all(|node| {
            node.top == px(0)
                && node.bottom == px(0)
                && node.align_items == AlignItems::Center
                && node.justify_content == JustifyContent::Center
        }));

        let mut switches = world.query::<(&SettingToggleButton, &Node)>();
        assert_eq!(switches.iter(world).count(), 9);
        assert!(switches.iter(world).all(|(_, node)| {
            node.width == px(82)
                && node.height == px(32)
                && node.border_radius == BorderRadius::all(px(16))
        }));
        let (_, vsync_node) = switches
            .iter(world)
            .find(|(toggle, _)| toggle.0 == SettingKind::Vsync)
            .expect("V-Sync web switch");
        assert_eq!(vsync_node.justify_content, JustifyContent::FlexStart);

        let mut thumbs = world.query::<(&SettingToggleThumb, &Node)>();
        let (_, vsync_thumb) = thumbs
            .iter(world)
            .find(|(thumb, _)| thumb.0 == SettingKind::Vsync)
            .expect("V-Sync switch thumb");
        assert_eq!(vsync_thumb.left, px(53));
    }

    #[test]
    fn selector_can_move_to_the_previous_value() {
        let mut app = App::new();
        app.insert_resource(State::new(GameState::Paused))
            .insert_resource(DisplaySettings::default())
            .insert_resource(LightingSettings::default())
            .insert_resource(TrafficSettings::default())
            .add_systems(Update, handle_setting_buttons);
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            SettingSelectButton {
                kind: SettingKind::Resolution,
                direction: SelectDirection::Previous,
            },
        ));
        app.update();

        assert_eq!(
            app.world().resource::<DisplaySettings>().resolution,
            ResolutionPreset::HdPlus
        );
    }

    #[test]
    fn traffic_selector_changes_the_live_density_setting() {
        use crate::world::traffic::TrafficDensity;

        let mut app = App::new();
        app.insert_resource(State::new(GameState::Paused))
            .insert_resource(DisplaySettings::default())
            .insert_resource(LightingSettings::default())
            .insert_resource(TrafficSettings::default())
            .add_systems(Update, handle_setting_buttons);
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            SettingSelectButton {
                kind: SettingKind::TrafficDensity,
                direction: SelectDirection::Next,
            },
        ));
        app.update();

        assert_eq!(
            app.world().resource::<TrafficSettings>().density,
            TrafficDensity::High
        );
    }

    #[test]
    fn volume_slider_uses_the_cursor_position() {
        let mut app = App::new();
        app.insert_resource(State::new(GameState::Paused))
            .insert_resource(crate::audio::AudioSettings::default())
            .add_systems(Update, handle_setting_sliders);
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::new(-0.25, 0.0)),
            },
            SettingSlider(SettingKind::EngineVolume),
        ));
        app.update();

        assert_eq!(
            app.world()
                .resource::<crate::audio::AudioSettings>()
                .engine_volume,
            0.25
        );
    }

    #[test]
    fn time_of_day_switch_is_on_the_main_pause_page() {
        let mut app = App::new();
        app.add_systems(Startup, spawn_menu);
        app.update();
        let world = app.world_mut();
        let mut settings = world.query::<(Entity, &SettingKind)>();
        let time_buttons: Vec<Entity> = settings
            .iter(world)
            .filter_map(|(entity, kind)| (*kind == SettingKind::TimeOfDay).then_some(entity))
            .collect();
        assert_eq!(time_buttons.len(), 1);

        let mut current = time_buttons[0];
        let mut on_main_page = false;
        while let Some(parent) = world.get::<ChildOf>(current) {
            current = parent.parent();
            if world.get::<MainMenuPanel>(current).is_some() {
                on_main_page = true;
                break;
            }
        }
        assert!(on_main_page, "time-of-day button must be in the main menu");
    }

    #[test]
    fn play_from_main_menu_opens_mode_selection_and_resume_still_drives() {
        for (state, expected) in [
            (GameState::MainMenu, GameState::ModeSelection),
            (GameState::Paused, GameState::Driving),
        ] {
            let mut app = App::new();
            app.insert_resource(State::new(state))
                .insert_resource(NextState::<GameState>::default())
                .init_resource::<PauseMenuPage>()
                .init_resource::<RebindState>()
                .add_message::<AppExit>()
                .add_systems(Update, handle_menu_buttons);
            app.world_mut()
                .spawn((Button, Interaction::Pressed, MenuAction::Resume));
            app.update();
            assert!(matches!(
                app.world().resource::<NextState<GameState>>(),
                NextState::Pending(value) if *value == expected
            ));
        }
    }

    #[test]
    fn launch_menu_shows_play_above_actions() {
        let mut app = App::new();
        app.insert_resource(State::new(GameState::MainMenu))
            .init_resource::<PauseMenuPage>()
            .add_systems(Startup, spawn_menu)
            .add_systems(Update, (sync_menu_visibility, sync_main_menu_copy).chain());
        app.update();
        let world = app.world_mut();
        let mut overlays = world.query_filtered::<&Visibility, With<PauseOverlay>>();
        assert_eq!(overlays.single(world).unwrap(), &Visibility::Inherited);
        let mut actions = world.query_filtered::<&Node, With<MenuActionColumn>>();
        assert_eq!(
            actions.single(world).unwrap().justify_content,
            JustifyContent::FlexStart
        );
        let mut copy = world.query::<(&MenuCopy, &Text)>();
        assert!(copy.iter(world).any(|(slot, text)| {
            matches!(slot, MenuCopy::PrimaryAction) && text.as_str() == "PLAY"
        }));
        let mut logo = world.query_filtered::<&Node, With<MainMenuLogo>>();
        assert_eq!(logo.single(world).unwrap().display, Display::Flex);
        assert!(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets/ui/onlygaz_logo.png")
                .is_file()
        );
    }

    #[test]
    fn controls_menu_exposes_primary_and_alternate_slots_for_every_action() {
        let mut app = App::new();
        app.add_systems(Startup, spawn_menu);
        app.update();
        let world = app.world_mut();
        let mut query = world.query::<&ControlBindingButton>();
        assert_eq!(query.iter(world).count(), ControlAction::ALL.len() * 2);
    }

    #[test]
    fn rebind_system_assigns_an_alternate_key() {
        let mut app = App::new();
        app.insert_resource(State::new(GameState::Paused))
            .insert_resource(KeyBindings::default())
            .insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(RebindState {
                pending: Some(PendingRebind {
                    action: ControlAction::Throttle,
                    slot: KeySlot::Alternate,
                }),
                message: None,
            })
            .add_systems(Update, apply_rebind_key);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyT);
        app.update();

        let world = app.world();
        assert_eq!(
            world
                .resource::<KeyBindings>()
                .alternate(ControlAction::Throttle),
            Some(KeyCode::KeyT)
        );
        assert!(world.resource::<RebindState>().pending.is_none());
    }

    #[test]
    fn rebind_system_rejects_a_key_used_by_another_action() {
        let mut app = App::new();
        app.insert_resource(State::new(GameState::Paused))
            .insert_resource(KeyBindings::default())
            .insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(RebindState {
                pending: Some(PendingRebind {
                    action: ControlAction::Throttle,
                    slot: KeySlot::Alternate,
                }),
                message: None,
            })
            .add_systems(Update, apply_rebind_key);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyS);
        app.update();

        let state = app.world().resource::<RebindState>();
        assert!(state.pending.is_some());
        assert!(
            state
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("assigned")
        );
        assert_eq!(
            app.world()
                .resource::<KeyBindings>()
                .alternate(ControlAction::Throttle),
            Some(KeyCode::ArrowUp)
        );
    }

    #[test]
    fn display_defaults_to_fullscreen() {
        assert_eq!(
            DisplaySettings::default().mode,
            DisplayModeSetting::Fullscreen
        );
    }

    #[test]
    fn every_resolution_has_valid_dimensions() {
        let mut resolution = ResolutionPreset::Hd;
        for _ in 0..5 {
            let (width, height) = resolution.size();
            assert!(width >= 1280 && height >= 720);
            resolution = resolution.next();
        }
        assert_eq!(resolution, ResolutionPreset::Hd);
    }

    #[test]
    fn preset_button_applies_the_complete_preset() {
        let mut app = App::new();
        app.insert_resource(State::new(GameState::Paused))
            .insert_resource(LightingSettings::default())
            .add_systems(Update, handle_graphics_quality_buttons);
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            GraphicsQualityButton(LightingQuality::Cinematic),
        ));
        app.update();
        let settings = app.world().resource::<LightingSettings>();
        assert_eq!(settings.quality, LightingQuality::Cinematic);
        assert_eq!(settings.shadow_quality, GraphicsLevel::Ultra);
        assert_eq!(settings.reflection_quality, GraphicsLevel::Ultra);
        assert!(settings.motion_blur);
        assert!(settings.collision_effect);
    }
}
