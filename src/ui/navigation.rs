//! Keyboard focus for every menu button.
//!
//! Bevy's `Interaction` is also the path used by the mouse handlers, so the
//! keyboard deliberately feeds that same path. This keeps Enter/Space exactly
//! equivalent to a click and lets all existing hover/pressed Material styling
//! remain the single visual source of truth.

use bevy::prelude::*;

use crate::game::GameState;

use super::menu::{GraphicsScroll, RebindState, SettingKind, SettingSlider};

#[derive(Resource, Default)]
pub(crate) struct KeyboardMenuNavigation {
    focused: Option<Entity>,
    keyboard_pressed: Option<Entity>,
}

#[derive(Debug, Clone, Copy)]
struct Candidate {
    entity: Entity,
    position: Vec2,
}

fn menu_accepts_navigation(state: GameState) -> bool {
    matches!(
        state,
        GameState::MainMenu | GameState::Paused | GameState::ModeSelection
    )
}

fn sort_reading_order(candidates: &mut [Candidate]) {
    candidates.sort_by(|left, right| {
        left.position
            .y
            .total_cmp(&right.position.y)
            .then_with(|| left.position.x.total_cmp(&right.position.x))
            .then_with(|| left.entity.cmp(&right.entity))
    });
}

fn next_focus_index(current: Option<usize>, count: usize, backwards: bool) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let current = current.unwrap_or(if backwards { 0 } else { count - 1 });
    Some(if backwards {
        (current + count - 1) % count
    } else {
        (current + 1) % count
    })
}

fn is_descendant_of(entity: Entity, ancestor: Entity, parents: &Query<&ChildOf>) -> bool {
    let mut current = entity;
    while let Ok(parent) = parents.get(current) {
        current = parent.parent();
        if current == ancestor {
            return true;
        }
    }
    false
}

/// Arrow keys move through the visible controls in visual reading order.
/// Enter and Space press the focused control through its normal interaction.
pub(crate) fn keyboard_menu_navigation(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<GameState>>,
    rebind: Res<RebindState>,
    mut audio: ResMut<crate::audio::AudioSettings>,
    mut navigation: ResMut<KeyboardMenuNavigation>,
    parents: Query<&ChildOf>,
    mut graphics_scroll: Query<
        (
            Entity,
            &mut ScrollPosition,
            &ComputedNode,
            &UiGlobalTransform,
        ),
        With<GraphicsScroll>,
    >,
    mut buttons: Query<
        (
            Entity,
            &mut Interaction,
            &ComputedNode,
            &UiGlobalTransform,
            &Node,
            Option<&InheritedVisibility>,
            Option<&SettingSlider>,
        ),
        With<Button>,
    >,
) {
    // Arrow keys are valid choices while rebinding controls and must not also
    // move UI focus underneath the capture prompt.
    if rebind.pending.is_some() || !menu_accepts_navigation(*state.get()) {
        if let Some(entity) = navigation.keyboard_pressed.take()
            && let Ok((_, mut interaction, ..)) = buttons.get_mut(entity)
            && *interaction == Interaction::Pressed
        {
            *interaction = Interaction::None;
        }
        navigation.focused = None;
        return;
    }

    // Release a synthetic keyboard press on the following frame. Mouse-held
    // presses are never put in this field and are therefore left untouched.
    if let Some(entity) = navigation.keyboard_pressed.take()
        && let Ok((_, mut interaction, ..)) = buttons.get_mut(entity)
        && *interaction == Interaction::Pressed
    {
        *interaction = Interaction::Hovered;
    }

    let mut candidates = Vec::new();
    for (entity, _, computed, transform, node, visibility, _) in &mut buttons {
        // UI nodes have InheritedVisibility, not ViewVisibility. Requiring the
        // latter silently excluded every real menu button from this query.
        if node.display == Display::None
            || visibility.is_some_and(|visibility| !visibility.get())
            || computed.is_empty()
        {
            continue;
        }
        let (_, _, position) = transform.to_scale_angle_translation();
        candidates.push(Candidate { entity, position });
    }
    sort_reading_order(&mut candidates);

    // Mouse hover is intentionally not used to reset keyboard focus. A static
    // cursor over the first item otherwise makes each Down key start from it.
    let valid_focus = navigation.focused.is_some_and(|entity| {
        candidates
            .iter()
            .any(|candidate| candidate.entity == entity)
    });
    let backwards = keys.just_pressed(KeyCode::ArrowUp);
    let forwards = keys.just_pressed(KeyCode::ArrowDown);
    if !valid_focus && !backwards && !forwards {
        navigation.focused = candidates.first().map(|candidate| candidate.entity);
    }

    if backwards || forwards {
        let current = navigation
            .focused
            .filter(|_| valid_focus)
            .and_then(|entity| {
                candidates
                    .iter()
                    .position(|candidate| candidate.entity == entity)
            });
        if let Some(index) = next_focus_index(current, candidates.len(), backwards) {
            navigation.focused = Some(candidates[index].entity);
        }
    }

    let focused = navigation.focused;
    if let Some(entity) = focused
        && let Ok((_, _, focused_node, focused_transform, ..)) = buttons.get(entity)
    {
        for (scroll_entity, mut scroll, viewport, viewport_transform) in &mut graphics_scroll {
            if !is_descendant_of(entity, scroll_entity, &parents) {
                continue;
            }
            let (_, _, focus_position) = focused_transform.to_scale_angle_translation();
            let (_, _, viewport_position) = viewport_transform.to_scale_angle_translation();
            let padding = 8.0;
            let upper = viewport_position.y - viewport.size().y * 0.5 + padding;
            let lower = viewport_position.y + viewport.size().y * 0.5 - padding;
            let focus_upper = focus_position.y - focused_node.size().y * 0.5;
            let focus_lower = focus_position.y + focused_node.size().y * 0.5;
            let max_scroll = ((viewport.content_size().y - viewport.size().y)
                * viewport.inverse_scale_factor())
            .max(0.0);
            if focus_upper < upper {
                scroll.y = (scroll.y - (upper - focus_upper) * viewport.inverse_scale_factor())
                    .clamp(0.0, max_scroll);
            } else if focus_lower > lower {
                scroll.y = (scroll.y + (focus_lower - lower) * viewport.inverse_scale_factor())
                    .clamp(0.0, max_scroll);
            }
        }
    }
    if let Some(entity) = focused
        && let Ok((_, mut interaction, _, _, _, _, slider)) = buttons.get_mut(entity)
    {
        let selected = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space);
        if let Some(slider) = slider {
            // Sliders have no cursor position when activated by keyboard.
            // Left/right adjust them; Enter advances one larger step.
            let delta = if keys.just_pressed(KeyCode::ArrowLeft) {
                -0.05
            } else if keys.just_pressed(KeyCode::ArrowRight) {
                0.05
            } else if selected {
                0.10
            } else {
                0.0
            };
            if delta != 0.0 {
                let value = match slider.0 {
                    SettingKind::EffectsVolume => &mut audio.effects_volume,
                    SettingKind::EngineVolume => &mut audio.engine_volume,
                    SettingKind::MusicVolume => &mut audio.music_volume,
                    _ => return,
                };
                *value = ((*value + delta).clamp(0.0, 1.0) * 100.0).round() / 100.0;
            }
        } else if selected {
            *interaction = Interaction::Pressed;
            navigation.keyboard_pressed = Some(entity);
        }
    }
}

/// Mouse hover and keyboard focus are independent. An outline sits outside
/// each control, so selected/toggled border colors remain legible underneath.
pub(crate) fn sync_menu_highlights(
    mut commands: Commands,
    navigation: Res<KeyboardMenuNavigation>,
    mut buttons: Query<
        (
            Entity,
            &Interaction,
            Option<&InheritedVisibility>,
            Option<&mut Outline>,
        ),
        With<Button>,
    >,
) {
    for (entity, interaction, visibility, outline) in &mut buttons {
        let visible = visibility.is_none_or(|visibility| visibility.get());
        let color = if !visible {
            Color::NONE
        } else if navigation.focused == Some(entity) {
            Color::srgb(0.55, 0.90, 1.0)
        } else if *interaction == Interaction::Pressed {
            Color::srgba(0.45, 0.82, 1.0, 0.88)
        } else if *interaction == Interaction::Hovered {
            Color::srgba(0.38, 0.73, 0.94, 0.58)
        } else {
            Color::NONE
        };
        let width = if navigation.focused == Some(entity) {
            px(2.0)
        } else {
            px(1.0)
        };
        if let Some(mut outline) = outline {
            outline.color = color;
            outline.width = width;
        } else {
            commands
                .entity(entity)
                .insert(Outline::new(width, px(2.0), color));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_wrap_through_every_visible_candidate() {
        assert!(menu_accepts_navigation(GameState::ModeSelection));
        assert_eq!(next_focus_index(Some(0), 4, false), Some(1));
        assert_eq!(next_focus_index(Some(3), 4, false), Some(0));
        assert_eq!(next_focus_index(Some(0), 4, true), Some(3));
        assert_eq!(next_focus_index(None, 4, false), Some(0));
        assert_eq!(next_focus_index(None, 4, true), Some(3));
        assert_eq!(next_focus_index(None, 0, false), None);
    }

    #[test]
    fn reading_order_is_top_to_bottom_then_left_to_right() {
        let entities = [
            Entity::from_raw_u32(1).unwrap(),
            Entity::from_raw_u32(2).unwrap(),
            Entity::from_raw_u32(3).unwrap(),
        ];
        let mut candidates = [
            Candidate {
                entity: entities[0],
                position: Vec2::new(40.0, 80.0),
            },
            Candidate {
                entity: entities[1],
                position: Vec2::new(90.0, 20.0),
            },
            Candidate {
                entity: entities[2],
                position: Vec2::new(10.0, 20.0),
            },
        ];
        sort_reading_order(&mut candidates);
        assert_eq!(
            candidates.map(|candidate| candidate.entity),
            [entities[2], entities[1], entities[0]]
        );
    }

    #[test]
    fn arrow_and_enter_drive_the_normal_button_interaction_path() {
        let mut app = App::new();
        app.insert_resource(State::new(GameState::MainMenu))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<RebindState>()
            .init_resource::<crate::audio::AudioSettings>()
            .init_resource::<KeyboardMenuNavigation>()
            .add_systems(Update, keyboard_menu_navigation);
        let first = app
            .world_mut()
            .spawn((
                Button,
                ComputedNode {
                    size: Vec2::splat(20.0),
                    ..default()
                },
                UiGlobalTransform::from_xy(10.0, 10.0),
                InheritedVisibility::VISIBLE,
            ))
            .id();
        let second = app
            .world_mut()
            .spawn((
                Button,
                ComputedNode {
                    size: Vec2::splat(20.0),
                    ..default()
                },
                UiGlobalTransform::from_xy(10.0, 40.0),
                InheritedVisibility::VISIBLE,
            ))
            .id();
        let hidden = app
            .world_mut()
            .spawn((
                Button,
                ComputedNode {
                    size: Vec2::splat(20.0),
                    ..default()
                },
                UiGlobalTransform::from_xy(10.0, 25.0),
                InheritedVisibility::HIDDEN,
            ))
            .id();

        app.update();
        assert_eq!(
            app.world().resource::<KeyboardMenuNavigation>().focused,
            Some(first)
        );
        assert_eq!(
            *app.world().get::<Interaction>(first).unwrap(),
            Interaction::None,
            "keyboard focus must not impersonate mouse hover"
        );

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowDown);
        app.update();
        assert_eq!(
            app.world().resource::<KeyboardMenuNavigation>().focused,
            Some(second)
        );
        assert_eq!(
            *app.world().get::<Interaction>(second).unwrap(),
            Interaction::None
        );
        assert_eq!(
            *app.world().get::<Interaction>(hidden).unwrap(),
            Interaction::None
        );

        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.clear();
            keys.press(KeyCode::Enter);
        }
        app.update();
        assert_eq!(
            *app.world().get::<Interaction>(second).unwrap(),
            Interaction::Pressed
        );
    }

    #[test]
    fn keyboard_focus_and_mouse_hover_receive_independent_outlines() {
        let mut app = App::new();
        let focused = app
            .world_mut()
            .spawn((Button, Interaction::None, InheritedVisibility::VISIBLE))
            .id();
        let hovered = app
            .world_mut()
            .spawn((Button, Interaction::Hovered, InheritedVisibility::VISIBLE))
            .id();
        app.insert_resource(KeyboardMenuNavigation {
            focused: Some(focused),
            keyboard_pressed: None,
        })
        .add_systems(Update, sync_menu_highlights);
        app.update();
        let focused_outline = app.world().get::<Outline>(focused).expect("focus outline");
        let hovered_outline = app.world().get::<Outline>(hovered).expect("hover outline");
        assert_eq!(focused_outline.width, px(2.0));
        assert_eq!(hovered_outline.width, px(1.0));
        assert_ne!(focused_outline.color, Color::NONE);
        assert_ne!(hovered_outline.color, Color::NONE);
        assert_ne!(focused_outline.color, hovered_outline.color);
    }

    #[test]
    fn enter_reaches_the_actual_menu_action_handler() {
        let mut app = App::new();
        app.insert_resource(State::new(GameState::MainMenu))
            .init_resource::<NextState<GameState>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<RebindState>()
            .init_resource::<crate::audio::AudioSettings>()
            .init_resource::<KeyboardMenuNavigation>()
            .init_resource::<super::super::menu::PauseMenuPage>()
            .add_message::<AppExit>()
            .add_systems(
                Update,
                (
                    keyboard_menu_navigation,
                    super::super::menu::handle_menu_buttons,
                )
                    .chain(),
            );
        app.world_mut().spawn((
            Button,
            super::super::menu::MenuAction::OpenGraphics,
            ComputedNode {
                size: Vec2::splat(20.0),
                ..default()
            },
            UiGlobalTransform::from_xy(10.0, 10.0),
            InheritedVisibility::VISIBLE,
        ));
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert_eq!(
            *app.world().resource::<super::super::menu::PauseMenuPage>(),
            super::super::menu::PauseMenuPage::Graphics
        );
    }

    #[test]
    fn keyboard_adjusts_a_focused_volume_slider_without_a_mouse_cursor() {
        let mut app = App::new();
        app.insert_resource(State::new(GameState::MainMenu))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<RebindState>()
            .init_resource::<crate::audio::AudioSettings>()
            .init_resource::<KeyboardMenuNavigation>()
            .add_systems(Update, keyboard_menu_navigation);
        app.world_mut().spawn((
            Button,
            SettingSlider(SettingKind::EngineVolume),
            ComputedNode {
                size: Vec2::splat(20.0),
                ..default()
            },
            UiGlobalTransform::from_xy(10.0, 10.0),
            InheritedVisibility::VISIBLE,
        ));
        app.world_mut()
            .resource_mut::<crate::audio::AudioSettings>()
            .engine_volume = 0.50;
        let initial = app
            .world()
            .resource::<crate::audio::AudioSettings>()
            .engine_volume;
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowRight);
        app.update();
        let changed = app
            .world()
            .resource::<crate::audio::AudioSettings>()
            .engine_volume;
        assert_eq!(changed, ((initial + 0.05).min(1.0) * 100.0).round() / 100.0);
    }
}
