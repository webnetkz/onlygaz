//! Touch semantics for the menus.
//!
//! Bevy's UI presses a button the moment a finger lands on it, which is right
//! for a mouse but makes a scrolling list impossible to use on a phone: every
//! drag would first toggle the setting under the finger. On touch screens the
//! menus therefore behave like native mobile controls:
//!
//! * a tap activates the button when the finger is lifted over it;
//! * moving the finger more than [`DRAG_THRESHOLD`] cancels the tap and, inside
//!   a [`TouchScroll`] container, scrolls it instead;
//! * sliders keep Bevy's direct press-and-drag behaviour.
//!
//! All existing menu handlers keep reading `Interaction::Pressed`; this module
//! only changes *when* a touch produces it.

use bevy::prelude::*;
use bevy::ui::UiSystems;
use bevy::window::PrimaryWindow;

use crate::game::GameState;
use crate::ui::menu::{SettingSlider, TouchScroll};

/// Finger travel that turns a tap into a drag [logical px].
pub const DRAG_THRESHOLD: f32 = 12.0;

#[derive(Resource, Default, Debug)]
struct MenuTouch {
    touch: Option<u64>,
    start: Vec2,
    last: Vec2,
    /// Button that will be activated if the finger is lifted over it.
    target: Option<Entity>,
    /// Scroll container under the finger, if any.
    scroll: Option<Entity>,
    dragging: bool,
}

pub struct MenuGesturePlugin;

impl Plugin for MenuGesturePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuTouch>()
            .add_systems(PreUpdate, menu_touch_gestures.after(UiSystems::Focus));
    }
}

fn menu_is_open(state: GameState) -> bool {
    matches!(
        state,
        GameState::MainMenu | GameState::ModeSelection | GameState::Paused
    )
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn menu_touch_gestures(
    touches: Res<Touches>,
    state: Res<State<GameState>>,
    ui_scale: Res<UiScale>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut gesture: ResMut<MenuTouch>,
    mut buttons: Query<
        (
            Entity,
            &mut Interaction,
            &ComputedNode,
            &UiGlobalTransform,
            &InheritedVisibility,
        ),
        (With<Button>, Without<SettingSlider>),
    >,
    mut scrolls: Query<
        (
            Entity,
            &mut ScrollPosition,
            &ComputedNode,
            &UiGlobalTransform,
            &InheritedVisibility,
        ),
        With<TouchScroll>,
    >,
    sliders: Query<(&ComputedNode, &UiGlobalTransform, &InheritedVisibility), With<SettingSlider>>,
) {
    if !menu_is_open(*state.get()) {
        *gesture = MenuTouch::default();
        return;
    }
    let scale_factor = windows.single().map_or(1.0, |window| window.scale_factor());
    let to_physical = |logical: Vec2| logical * scale_factor;

    // A new finger: remember it and hold back the press Bevy just produced.
    if gesture.touch.is_none()
        && let Some(touch) = touches.iter_just_pressed().next()
    {
        let position = touch.position();
        let physical = to_physical(position);
        // A very short tap can begin and end within one frame. Bevy's focus
        // system sees no pressed finger then, so activate the button directly.
        if touches.just_released(touch.id()) {
            if let Some((_, mut interaction, ..)) =
                buttons
                    .iter_mut()
                    .find(|(_, _, node, transform, visibility)| {
                        visibility.get() && node.contains_point(**transform, physical)
                    })
            {
                *interaction = Interaction::Pressed;
            }
            *gesture = MenuTouch::default();
            return;
        }
        let mut target = None;
        for (entity, mut interaction, node, transform, visibility) in &mut buttons {
            if *interaction == Interaction::Pressed
                && visibility.get()
                && node.contains_point(*transform, physical)
            {
                *interaction = Interaction::Hovered;
                target = Some(entity);
            }
        }
        // A finger on a volume slider drags the slider, not the page.
        let on_slider = sliders.iter().any(|(node, transform, visibility)| {
            visibility.get() && node.contains_point(*transform, physical)
        });
        let scroll = scrolls
            .iter()
            .filter(|_| !on_slider)
            .filter(|(_, _, node, transform, visibility)| {
                visibility.get() && node.contains_point(**transform, physical)
            })
            // The innermost (smallest) container wins.
            .min_by(|a, b| {
                let area_a = a.2.size().x * a.2.size().y;
                let area_b = b.2.size().x * b.2.size().y;
                area_a.total_cmp(&area_b)
            })
            .map(|(entity, ..)| entity);
        debug!(
            "menu touch {} starts at {position} on {target:?}, scroll {scroll:?}",
            touch.id()
        );
        *gesture = MenuTouch {
            touch: Some(touch.id()),
            start: position,
            last: position,
            target,
            scroll,
            dragging: false,
        };
        return;
    }

    let Some(id) = gesture.touch else {
        return;
    };

    if let Some(touch) = touches.get_pressed(id) {
        // Bevy presses again whenever another finger lands; keep holding the
        // activation back until this finger is lifted.
        for (entity, mut interaction, ..) in &mut buttons {
            if *interaction == Interaction::Pressed {
                *interaction = if Some(entity) == gesture.target && !gesture.dragging {
                    Interaction::Hovered
                } else {
                    Interaction::None
                };
            }
        }
        let position = touch.position();
        let travelled = position - gesture.start;
        if !gesture.dragging && travelled.length() > DRAG_THRESHOLD {
            gesture.dragging = true;
            if let Some(target) = gesture.target.take()
                && let Ok((_, mut interaction, ..)) = buttons.get_mut(target)
            {
                *interaction = Interaction::None;
            }
        }
        if gesture.dragging
            && let Some(scroll) = gesture.scroll
            && let Ok((_, mut position_component, node, ..)) = scrolls.get_mut(scroll)
        {
            // Finger delta in UI pixels: logical pixels divided by UiScale.
            let delta = (position.y - gesture.last.y) / ui_scale.0.max(0.01);
            let max =
                ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
            position_component.y = (position_component.y - delta).clamp(0.0, max);
        }
        gesture.last = position;
        return;
    }

    // The finger was lifted (or the touch cancelled): activate on release.
    debug!(
        "menu touch {id} ends: target {:?}, dragging {}",
        gesture.target, gesture.dragging
    );
    let released_at = touches
        .iter_just_released()
        .find(|touch| touch.id() == id)
        .map(|touch| touch.position());
    if let (Some(position), Some(target), false) = (released_at, gesture.target, gesture.dragging)
        && let Ok((_, mut interaction, node, transform, visibility)) = buttons.get_mut(target)
        && visibility.get()
        && node.contains_point(*transform, to_physical(position))
    {
        // Bevy's focus system clears this again on the next frame, exactly
        // like a mouse click that was pressed and released in one frame.
        *interaction = Interaction::Pressed;
        debug!("menu tap activates {target:?}");
    }
    *gesture = MenuTouch::default();
}
