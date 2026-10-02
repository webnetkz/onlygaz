//! On-screen driving controls: gas, brake, nitro, steering, pause and camera.
//!
//! Every control is an icon (assets/ui/touch, drawn by
//! tools/make_touch_icons.py) tinted with the control's colour: pedals for
//! gas and brake, a flame for nitro.
//!
//! The controls are plain UI nodes without Bevy's `Interaction`, because that
//! only tracks a single pointer. Here every finger is hit-tested against
//! every control each frame, so the player can hold GAS, steer and fire NITRO
//! at the same time, and can slide a thumb from one pedal to the other.

use bevy::prelude::*;
use bevy::ui::UiSystems;
use bevy::window::PrimaryWindow;

use crate::camera::controller::GameCamera;
use crate::game::GameState;
use crate::mobile::{SteeringMode, TouchControlSettings};
use crate::vehicle::controller::{DriverInput, collect_keyboard_input, smooth_driver_input};

use super::layout::SafeArea;
use super::tilt::TiltInput;

/// One on-screen control.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TouchControlKind {
    SteerLeft,
    SteerRight,
    Gas,
    Brake,
    Nitro,
    Menu,
    Camera,
}

impl TouchControlKind {
    pub const ALL: [Self; 7] = [
        Self::SteerLeft,
        Self::SteerRight,
        Self::Gas,
        Self::Brake,
        Self::Nitro,
        Self::Menu,
        Self::Camera,
    ];

    const fn bit(self) -> u8 {
        1 << self as u8
    }

    /// Pedals and steering act while held; the others fire once per tap.
    const fn is_held_control(self) -> bool {
        !matches!(self, Self::Menu | Self::Camera)
    }

    fn accent(self) -> Color {
        match self {
            Self::Gas => Color::srgb(0.36, 0.92, 0.56),
            Self::Brake => Color::srgb(1.0, 0.38, 0.38),
            Self::Nitro => Color::srgb(0.32, 0.78, 1.0),
            Self::SteerLeft | Self::SteerRight | Self::Menu | Self::Camera => {
                Color::srgb(0.86, 0.92, 1.0)
            }
        }
    }
}

/// Which controls are held this frame, and which were tapped.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TouchControlState {
    held: u8,
    tapped: u8,
}

impl TouchControlState {
    pub fn held(&self, kind: TouchControlKind) -> bool {
        self.held & kind.bit() != 0
    }

    pub fn tapped(&self, kind: TouchControlKind) -> bool {
        self.tapped & kind.bit() != 0
    }

    /// Steering demand from the arrow buttons: -1 left, +1 right.
    pub fn button_steer(&self) -> f32 {
        f32::from(self.held(TouchControlKind::SteerRight) as u8)
            - f32::from(self.held(TouchControlKind::SteerLeft) as u8)
    }

    #[cfg(test)]
    fn with(held: &[TouchControlKind], tapped: &[TouchControlKind]) -> Self {
        Self {
            held: held.iter().fold(0, |bits, kind| bits | kind.bit()),
            tapped: tapped.iter().fold(0, |bits, kind| bits | kind.bit()),
        }
    }
}

#[derive(Component)]
struct TouchOverlayRoot;

/// The icon inside a control; turns white while the control is held.
#[derive(Component)]
struct TouchIcon(TouchControlKind);

/// Fingers within this fraction of a control's size still count as on it.
const HIT_SLOP: f32 = 0.12;
// Sizes in UI pixels of the landscape canvas (see `layout::DESIGN_WIDTH`):
// the left thumb steers, the right thumb works the pedals.
const EDGE: f32 = 16.0;
const GAP: f32 = 12.0;
const STEER_SIZE: Vec2 = Vec2::new(112.0, 112.0);
const GAS_SIZE: Vec2 = Vec2::new(112.0, 176.0);
const BRAKE_SIZE: Vec2 = Vec2::new(98.0, 124.0);
const TILT_BRAKE_SIZE: Vec2 = Vec2::new(128.0, 150.0);
const NITRO_SIZE: Vec2 = Vec2::new(112.0, 82.0);
const SMALL_SIZE: Vec2 = Vec2::new(60.0, 60.0);

const FILL: Color = Color::srgba(0.02, 0.05, 0.09, 0.40);
const FILL_HELD: Color = Color::srgba(0.07, 0.30, 0.48, 0.78);

/// Where a control sits for the current steering mode. `None` hides it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Placement {
    left: Option<f32>,
    right: Option<f32>,
    top: Option<f32>,
    bottom: Option<f32>,
    size: Vec2,
}

fn placement(kind: TouchControlKind, show_arrows: bool, safe: SafeArea) -> Option<Placement> {
    let bottom = EDGE + safe.bottom;
    let top = EDGE + safe.top;
    let left = EDGE + safe.left;
    let right = EDGE + safe.right;
    let at = |left_x: Option<f32>,
              right_x: Option<f32>,
              top_y: Option<f32>,
              bottom_y: Option<f32>,
              size| {
        Some(Placement {
            left: left_x,
            right: right_x,
            top: top_y,
            bottom: bottom_y,
            size,
        })
    };
    match kind {
        TouchControlKind::SteerLeft if show_arrows => {
            at(Some(left), None, None, Some(bottom), STEER_SIZE)
        }
        TouchControlKind::SteerRight if show_arrows => at(
            Some(left + STEER_SIZE.x + GAP),
            None,
            None,
            Some(bottom),
            STEER_SIZE,
        ),
        TouchControlKind::SteerLeft | TouchControlKind::SteerRight => None,
        TouchControlKind::Gas => at(None, Some(right), None, Some(bottom), GAS_SIZE),
        TouchControlKind::Nitro => at(
            None,
            Some(right),
            None,
            Some(bottom + GAS_SIZE.y + GAP),
            NITRO_SIZE,
        ),
        TouchControlKind::Brake if show_arrows => at(
            None,
            Some(right + GAS_SIZE.x + GAP),
            None,
            Some(bottom),
            BRAKE_SIZE,
        ),
        TouchControlKind::Brake => at(Some(left), None, None, Some(bottom), TILT_BRAKE_SIZE),
        TouchControlKind::Menu => at(None, Some(right), Some(top), None, SMALL_SIZE),
        TouchControlKind::Camera => at(
            None,
            Some(right + SMALL_SIZE.x + GAP),
            Some(top),
            None,
            SMALL_SIZE,
        ),
    }
}

/// Icon image of a control.
fn icon_for(kind: TouchControlKind) -> &'static str {
    match kind {
        TouchControlKind::SteerLeft => "ui/touch/arrow_left.png",
        TouchControlKind::SteerRight => "ui/touch/arrow_right.png",
        TouchControlKind::Gas => "ui/touch/gas.png",
        TouchControlKind::Brake => "ui/touch/brake.png",
        TouchControlKind::Nitro => "ui/touch/nitro.png",
        TouchControlKind::Menu => "ui/touch/pause.png",
        TouchControlKind::Camera => "ui/touch/camera.png",
    }
}

/// Size of the icon inside a button of `button` size [UI px]. Keeps each
/// image's aspect and grows with the button (the brake is larger in tilt
/// mode).
fn icon_size(kind: TouchControlKind, button: Vec2) -> Vec2 {
    let side = button.x.min(button.y);
    match kind {
        TouchControlKind::Gas => {
            // gas.png is 128 x 208: a tall pedal for the tall button.
            let width = (button.x * 0.55).round();
            Vec2::new(width, (width * 208.0 / 128.0).round())
        }
        TouchControlKind::SteerLeft | TouchControlKind::SteerRight => {
            Vec2::splat((side * 0.48).round())
        }
        TouchControlKind::Brake => Vec2::splat((side * 0.68).round()),
        TouchControlKind::Nitro => Vec2::splat((side * 0.64).round()),
        TouchControlKind::Menu => Vec2::splat((side * 0.47).round()),
        TouchControlKind::Camera => Vec2::splat((side * 0.57).round()),
    }
}

fn corner_radius(kind: TouchControlKind) -> f32 {
    match kind {
        TouchControlKind::SteerLeft | TouchControlKind::SteerRight => STEER_SIZE.x * 0.5,
        TouchControlKind::Menu | TouchControlKind::Camera => 14.0,
        _ => 18.0,
    }
}

pub struct TouchControlsPlugin;

impl Plugin for TouchControlsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TouchControlState>()
            .add_systems(Startup, spawn_touch_controls)
            .add_systems(PreUpdate, read_touch_controls.after(UiSystems::Focus))
            .add_systems(
                Update,
                (
                    apply_touch_driving_input
                        .after(collect_keyboard_input)
                        .before(smooth_driver_input)
                        .run_if(in_state(GameState::Driving)),
                    handle_touch_taps.run_if(in_state(GameState::Driving)),
                    arrange_touch_controls,
                    style_touch_controls,
                ),
            );
    }
}

fn spawn_touch_controls(mut commands: Commands, assets: Res<AssetServer>) {
    commands
        .spawn((
            Name::new("Touch controls"),
            TouchOverlayRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.0),
                height: percent(100.0),
                ..default()
            },
            Visibility::Hidden,
            // Above the HUD, below every menu.
            GlobalZIndex(40),
        ))
        .with_children(|root| {
            for kind in TouchControlKind::ALL {
                let radius = corner_radius(kind);
                let mut control = root.spawn((
                    Name::new(format!("Touch {kind:?}")),
                    kind,
                    Node {
                        position_type: PositionType::Absolute,
                        width: px(SMALL_SIZE.x),
                        height: px(SMALL_SIZE.y),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(px(2)),
                        border_radius: BorderRadius::all(px(radius)),
                        ..default()
                    },
                    BackgroundColor(FILL),
                    BorderColor::all(kind.accent().with_alpha(0.55)),
                ));
                // Sized for real by `arrange_touch_controls`.
                let size = icon_size(kind, SMALL_SIZE);
                control.with_children(|content| {
                    content.spawn((
                        TouchIcon(kind),
                        ImageNode::new(assets.load(icon_for(kind))).with_color(kind.accent()),
                        Node {
                            width: px(size.x),
                            height: px(size.y),
                            ..default()
                        },
                    ));
                });
            }
        });
}

/// Hit-test every finger (and, on a desktop preview, the mouse) against the
/// visible controls.
fn read_touch_controls(
    touches: Res<Touches>,
    mouse: Res<ButtonInput<MouseButton>>,
    state: Res<State<GameState>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut controls: ResMut<TouchControlState>,
    nodes: Query<(
        &TouchControlKind,
        &ComputedNode,
        &UiGlobalTransform,
        &InheritedVisibility,
        &Node,
    )>,
) {
    let mut held = 0u8;
    let mut tapped = 0u8;
    if *state.get() == GameState::Driving
        && let Ok(window) = windows.single()
    {
        let scale = window.scale_factor();
        let mut pointers: Vec<(Vec2, bool)> = touches
            .iter()
            .map(|touch| (touch.position() * scale, touches.just_pressed(touch.id())))
            .collect();
        if mouse.pressed(MouseButton::Left)
            && let Some(cursor) = window.physical_cursor_position()
        {
            pointers.push((cursor, mouse.just_pressed(MouseButton::Left)));
        }
        for (kind, node, transform, visibility, style) in &nodes {
            if !visibility.get() || style.display == Display::None {
                continue;
            }
            let Some(inverse) = transform.try_inverse() else {
                continue;
            };
            let half = node.size() * (0.5 + HIT_SLOP * 0.5);
            for (pointer, just_pressed) in &pointers {
                let local = inverse.transform_point2(*pointer);
                if local.x.abs() <= half.x && local.y.abs() <= half.y {
                    if kind.is_held_control() {
                        held |= kind.bit();
                    } else if *just_pressed {
                        tapped |= kind.bit();
                    }
                }
            }
        }
    }
    let next = TouchControlState { held, tapped };
    if *controls != next {
        *controls = next;
    }
}

/// Merge the touch pedals and steering into the driver's raw demand.
fn apply_touch_driving_input(
    controls: Res<TouchControlState>,
    settings: Res<TouchControlSettings>,
    tilt: Res<TiltInput>,
    mut inputs: Query<&mut DriverInput>,
) {
    let steer = touch_steer(&controls, &settings, &tilt);
    for mut input in &mut inputs {
        merge_touch_input(&mut input, &controls, steer);
    }
}

fn touch_steer(
    controls: &TouchControlState,
    settings: &TouchControlSettings,
    tilt: &TiltInput,
) -> f32 {
    let buttons = controls.button_steer();
    if settings.steering == SteeringMode::Tilt && tilt.available && buttons == 0.0 {
        tilt.steer
    } else {
        buttons
    }
}

fn merge_touch_input(input: &mut DriverInput, controls: &TouchControlState, steer: f32) {
    if controls.held(TouchControlKind::Gas) {
        input.throttle = 1.0;
    }
    if controls.held(TouchControlKind::Brake) {
        input.brake = 1.0;
    }
    if controls.held(TouchControlKind::Nitro) {
        // Nitro already implies full forward demand in the solver.
        input.nitro = 1.0;
    }
    if steer != 0.0 {
        input.steer = steer.clamp(-1.0, 1.0);
    }
}

fn handle_touch_taps(
    controls: Res<TouchControlState>,
    mut next: ResMut<NextState<GameState>>,
    mut cameras: Query<&mut GameCamera>,
) {
    if controls.tapped(TouchControlKind::Menu) {
        next.set(GameState::Paused);
    }
    if controls.tapped(TouchControlKind::Camera) {
        for mut camera in &mut cameras {
            camera.cycle();
            info!("camera: {}", camera.mode.label());
        }
    }
}

/// Show the overlay only while driving and lay it out for the steering mode.
#[allow(clippy::too_many_arguments)]
fn arrange_touch_controls(
    state: Res<State<GameState>>,
    settings: Res<TouchControlSettings>,
    tilt: Res<TiltInput>,
    safe_area: Res<SafeArea>,
    ui_scale: Res<UiScale>,
    mut roots: Query<&mut Visibility, With<TouchOverlayRoot>>,
    mut controls: Query<(&TouchControlKind, &mut Node), Without<TouchIcon>>,
    mut icons: Query<(&TouchIcon, &mut Node), Without<TouchControlKind>>,
    mut last: Local<Option<(bool, SafeArea)>>,
) {
    let driving = *state.get() == GameState::Driving;
    for mut visibility in &mut roots {
        let wanted = if driving {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }

    // Arrows stay available whenever tilt cannot steer (no sensor).
    let show_arrows = settings.steering == SteeringMode::Buttons || !tilt.available;
    let safe = safe_area.in_ui_pixels(ui_scale.0);
    if *last == Some((show_arrows, safe)) {
        return;
    }
    *last = Some((show_arrows, safe));
    for (kind, mut node) in &mut controls {
        match placement(*kind, show_arrows, safe) {
            Some(place) => {
                node.display = Display::Flex;
                node.width = px(place.size.x);
                node.height = px(place.size.y);
                node.left = place.left.map_or(Val::Auto, px);
                node.right = place.right.map_or(Val::Auto, px);
                node.top = place.top.map_or(Val::Auto, px);
                node.bottom = place.bottom.map_or(Val::Auto, px);
            }
            None => node.display = Display::None,
        }
    }
    for (icon, mut node) in &mut icons {
        if let Some(place) = placement(icon.0, show_arrows, safe) {
            let size = icon_size(icon.0, place.size);
            node.width = px(size.x);
            node.height = px(size.y);
        }
    }
}

fn style_touch_controls(
    controls: Res<TouchControlState>,
    mut nodes: Query<(&TouchControlKind, &mut BackgroundColor, &mut BorderColor)>,
    mut icons: Query<(&TouchIcon, &mut ImageNode)>,
) {
    if !controls.is_changed() {
        return;
    }
    for (kind, mut background, mut border) in &mut nodes {
        let active = controls.held(*kind);
        let fill = if active { FILL_HELD } else { FILL };
        if background.0 != fill {
            background.0 = fill;
        }
        let edge = BorderColor::all(kind.accent().with_alpha(if active { 1.0 } else { 0.55 }));
        if *border != edge {
            *border = edge;
        }
    }
    for (icon, mut image) in &mut icons {
        let tint = icon_tint(icon.0, controls.held(icon.0));
        if image.color != tint {
            image.color = tint;
        }
    }
}

/// Icons show the control's colour, and turn white while it is held.
fn icon_tint(kind: TouchControlKind, held: bool) -> Color {
    if held { Color::WHITE } else { kind.accent() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pedals_and_nitro_reach_the_driver_input() {
        let controls =
            TouchControlState::with(&[TouchControlKind::Gas, TouchControlKind::Nitro], &[]);
        let mut input = DriverInput::default();
        merge_touch_input(&mut input, &controls, 0.0);
        assert_eq!(input.throttle, 1.0);
        assert_eq!(input.nitro, 1.0);
        assert_eq!(input.brake, 0.0);
    }

    #[test]
    fn arrow_buttons_steer_and_cancel_each_other() {
        let settings = TouchControlSettings::default();
        let tilt = TiltInput::default();
        let left = TouchControlState::with(&[TouchControlKind::SteerLeft], &[]);
        assert_eq!(touch_steer(&left, &settings, &tilt), -1.0);
        let both = TouchControlState::with(
            &[TouchControlKind::SteerLeft, TouchControlKind::SteerRight],
            &[],
        );
        assert_eq!(touch_steer(&both, &settings, &tilt), 0.0);
    }

    #[test]
    fn tilt_mode_uses_the_sensor_only_when_available() {
        let settings = TouchControlSettings {
            steering: SteeringMode::Tilt,
            ..default()
        };
        let none = TouchControlState::default();
        let mut tilt = TiltInput {
            available: true,
            roll_degrees: 10.0,
            steer: 0.4,
        };
        assert_eq!(touch_steer(&none, &settings, &tilt), 0.4);
        tilt.available = false;
        assert_eq!(touch_steer(&none, &settings, &tilt), 0.0);
    }

    #[test]
    fn keyboard_steering_survives_when_no_touch_steers() {
        let mut input = DriverInput {
            steer: -1.0,
            ..default()
        };
        merge_touch_input(&mut input, &TouchControlState::default(), 0.0);
        assert_eq!(input.steer, -1.0);
    }

    #[test]
    fn layouts_keep_controls_apart() {
        let safe = SafeArea::default();
        let gas = placement(TouchControlKind::Gas, true, safe).unwrap();
        let brake = placement(TouchControlKind::Brake, true, safe).unwrap();
        // Brake sits left of gas with a gap in button mode.
        assert!(brake.right.unwrap() >= gas.right.unwrap() + gas.size.x);
        // Tilt mode moves the brake to the left edge and hides the arrows.
        let tilt_brake = placement(TouchControlKind::Brake, false, safe).unwrap();
        assert!(tilt_brake.left.is_some());
        assert!(placement(TouchControlKind::SteerLeft, false, safe).is_none());
        // Both thumb clusters leave the middle of the smallest canvas free
        // for the car, and the pedal stack stays below the top buttons.
        let right_cluster = brake.right.unwrap() + brake.size.x;
        let steer = placement(TouchControlKind::SteerRight, true, safe).unwrap();
        let left_cluster = steer.left.unwrap() + steer.size.x;
        assert!(left_cluster + right_cluster < super::super::layout::DESIGN_WIDTH * 0.6);
        let nitro = placement(TouchControlKind::Nitro, true, safe).unwrap();
        let menu = placement(TouchControlKind::Menu, true, safe).unwrap();
        let stack_top = nitro.bottom.unwrap() + nitro.size.y;
        let menu_bottom = menu.top.unwrap() + menu.size.y;
        assert!(stack_top + menu_bottom + GAP < super::super::layout::DESIGN_HEIGHT);
    }

    #[test]
    fn every_control_has_an_icon_that_fits_its_button() {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        for kind in TouchControlKind::ALL {
            let icon = icon_for(kind);
            assert!(assets.join(icon).is_file(), "missing {icon}");
            for show_arrows in [true, false] {
                if let Some(place) = placement(kind, show_arrows, SafeArea::default()) {
                    let icon = icon_size(kind, place.size);
                    assert!(
                        icon.x + 8.0 <= place.size.x && icon.y + 8.0 <= place.size.y,
                        "{kind:?}: {icon} does not fit {}",
                        place.size
                    );
                }
            }
        }
        assert_eq!(icon_tint(TouchControlKind::Brake, true), Color::WHITE);
        assert_eq!(
            icon_tint(TouchControlKind::Brake, false),
            TouchControlKind::Brake.accent()
        );
    }

    #[test]
    fn taps_and_holds_are_tracked_separately() {
        let state = TouchControlState::with(&[TouchControlKind::Gas], &[TouchControlKind::Menu]);
        assert!(state.held(TouchControlKind::Gas));
        assert!(!state.held(TouchControlKind::Menu));
        assert!(state.tapped(TouchControlKind::Menu));
    }
}
