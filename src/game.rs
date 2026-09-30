//! Application assembly: plugin wiring, global state and the keys that are not
//! part of driving.

use std::collections::HashMap;

use avian3d::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::light::{CascadeShadowConfig, NotShadowCaster};
use bevy::pbr::DefaultOpaqueRendererMethod;
use bevy::prelude::*;
use bevy::render::renderer::RenderDevice;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::audio::EngineAudioPlugin;
use crate::camera::CameraPlugin;
use crate::camera::controller::{GameCamera, SideView};
use crate::camera::speed_effect::{CollisionEffectSettings, SpeedEffectSettings};
use crate::ui::UiPlugin;
use crate::vehicle::VehiclePlugin;
use crate::vehicle::controller::{ControlAction, KeyBindings};
use crate::vehicle::physics::{self, Vehicle};
use crate::vehicle::telemetry::PerformanceLog;
use crate::world::lighting::{self, LightingSettings, LitCamera};
use crate::world::road::RoadSpec;
use crate::world::{PerformanceShadowExcluded, WorldPlugin};

/// Physics rate. 120 Hz gives roughly one metre of travel per step at 415 km/h,
/// which the ray-cast suspension and the tyre relaxation model handle
/// comfortably; the tyre ODE is sub-stepped further inside the solver.
pub const PHYSICS_HZ: f64 = 120.0;

/// The car must remain clearly roof-down for this long before automatic
/// recovery. A short grace period lets a normal roll finish naturally.
const AUTO_RECOVERY_DELAY_SECONDS: f32 = 1.5;
const UPSIDE_DOWN_UP_DOT: f32 = -0.35;

#[derive(Resource, Default)]
struct AutoRecoveryState {
    upside_down_for: HashMap<Entity, f32>,
}

/// Coarse application state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, States)]
pub enum GameState {
    #[default]
    Driving,
    MainMenu,
    ModeSelection,
    Paused,
}

/// Everything except the user interface: world, car, cameras, physics and the
/// global key bindings.
///
/// Kept separate from [`GamePlugin`] so the simulation can be built and stepped
/// without a renderer - see `tests/app_smoke.rs`.
pub struct SimulationPlugin;

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        let lighting = app
            .world()
            .get_resource::<LightingSettings>()
            .cloned()
            .unwrap_or_default();
        info!("lighting quality: {}", lighting.quality.label());

        // Screen space reflections only exist on the deferred path, and that is
        // an app-wide decision rather than a per-camera one.
        if lighting.uses_reflections() {
            app.insert_resource(bevy::pbr::DefaultOpaqueRendererMethod::deferred());
        }

        let ambient = lighting::ambient_for(lighting.uses_atmosphere(), lighting.time_of_day);
        let collision_effect = CollisionEffectSettings {
            enabled: lighting.collision_effect_enabled(),
            ..default()
        };

        app.insert_resource(ClearColor(lighting.sky_color()))
            .insert_resource(ambient)
            .insert_resource(lighting::shadow_map_settings(&lighting))
            .insert_resource(lighting)
            .insert_resource(collision_effect)
            .insert_resource(Time::<Fixed>::from_hz(PHYSICS_HZ))
            .insert_resource(Gravity(Vec3::NEG_Y * 9.81))
            .init_state::<GameState>()
            .init_resource::<KeyBindings>()
            .init_resource::<AutoRecoveryState>()
            .add_plugins(PhysicsPlugins::default())
            .add_plugins((WorldPlugin, VehiclePlugin, CameraPlugin, EngineAudioPlugin))
            .register_type::<LightingSettings>()
            .add_systems(
                Update,
                (
                    cycle_camera,
                    hold_rear_view,
                    reset_vehicle,
                    auto_recover_vehicle,
                    toggle_abs,
                    toggle_triple_power,
                    handle_pause,
                    cycle_lighting_quality,
                    sync_cursor_with_game_state,
                ),
            )
            .add_systems(Update, apply_lighting_quality.after(cycle_lighting_quality))
            .add_systems(
                Update,
                sync_collision_effect_setting.after(cycle_lighting_quality),
            )
            .add_systems(OnEnter(GameState::Paused), pause_physics)
            .add_systems(OnExit(GameState::Paused), resume_physics)
            .add_systems(OnEnter(GameState::ModeSelection), pause_physics)
            .add_systems(
                OnEnter(GameState::ModeSelection),
                crate::vehicle::prepare_new_race.after(pause_physics),
            )
            .add_systems(OnExit(GameState::ModeSelection), resume_physics)
            .add_systems(OnEnter(GameState::MainMenu), pause_physics)
            .add_systems(OnExit(GameState::MainMenu), resume_physics);
    }
}

/// The full game: simulation plus interface.
pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((SimulationPlugin, UiPlugin))
            // Headless simulation tests keep the default Driving state, while
            // the full game opens in the main menu before choosing a mode.
            .insert_state(GameState::MainMenu);
    }
}

fn cycle_camera(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<KeyBindings>,
    state: Res<State<GameState>>,
    mut cameras: Query<&mut GameCamera>,
) {
    if *state.get() != GameState::Driving
        || !bindings.just_pressed(&keys, ControlAction::CycleCamera)
    {
        return;
    }
    for mut camera in &mut cameras {
        camera.cycle();
        info!("camera: {}", camera.mode.label());
    }
}

fn hold_rear_view(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<KeyBindings>,
    state: Res<State<GameState>>,
    rebind: Option<Res<crate::ui::menu::RebindState>>,
    mut cameras: Query<&mut GameCamera>,
) {
    let accepts_input = *state.get() == GameState::Driving
        && !rebind.as_ref().is_some_and(|state| state.pending.is_some());
    let side_view = if accepts_input && bindings.pressed(&keys, ControlAction::ViewLeft) {
        SideView::Left
    } else if accepts_input && bindings.pressed(&keys, ControlAction::ViewRight) {
        SideView::Right
    } else {
        SideView::None
    };
    let rear_view = accepts_input
        && side_view == SideView::None
        && bindings.pressed(&keys, ControlAction::ToggleRearView);
    for mut camera in &mut cameras {
        let changed = camera.set_rear_view(rear_view) | camera.set_side_view(side_view);
        if changed {
            info!("camera: {}", camera.display_label());
        }
    }
}

fn is_upside_down(rotation: Quat) -> bool {
    (rotation * Vec3::Y).dot(Vec3::Y) < UPSIDE_DOWN_UP_DOT
}

/// Recover a local car after it has rested on its roof long enough to be
/// unrecoverable through normal steering. This uses the exact same clean
/// respawn path as the player's `R` action.
fn auto_recover_vehicle(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<State<GameState>>,
    spec: Res<RoadSpec>,
    mut recovery: ResMut<AutoRecoveryState>,
    mut performance: ResMut<PerformanceLog>,
    mut vehicles: Query<
        (Entity, &Position, &Rotation, &mut Vehicle),
        With<crate::vehicle::physics::VehicleTag>,
    >,
) {
    if *state.get() != GameState::Driving {
        recovery.upside_down_for.clear();
        return;
    }
    recovery
        .upside_down_for
        .retain(|entity, _| vehicles.contains(*entity));

    for (entity, position, rotation, mut vehicle) in &mut vehicles {
        if !is_upside_down(rotation.0) {
            recovery.upside_down_for.remove(&entity);
            continue;
        }
        let elapsed = recovery.upside_down_for.entry(entity).or_default();
        *elapsed += time.delta_secs();
        if *elapsed < AUTO_RECOVERY_DELAY_SECONDS {
            continue;
        }

        let pose = spec.respawn_transform(position.0);
        physics::respawn(&mut commands, entity, &mut vehicle, pose);
        recovery.upside_down_for.remove(&entity);
        performance.restart_run();
        info!("vehicle automatically recovered from its roof");
    }
}

fn reset_vehicle(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<KeyBindings>,
    rebind: Option<Res<crate::ui::menu::RebindState>>,
    spec: Res<RoadSpec>,
    mut performance: ResMut<PerformanceLog>,
    mut vehicles: Query<
        (Entity, &Transform, &mut Vehicle),
        With<crate::vehicle::physics::VehicleTag>,
    >,
) {
    if rebind.as_ref().is_some_and(|state| state.pending.is_some()) {
        return;
    }
    if !bindings.just_pressed(&keys, ControlAction::Reset) {
        return;
    }
    for (entity, transform, mut vehicle) in &mut vehicles {
        let pose = spec.respawn_transform(transform.translation);
        physics::respawn(&mut commands, entity, &mut vehicle, pose);
        performance.restart_run();
        info!(
            "vehicle reset to lane x={:.2} at {:.0} m",
            pose.translation.x, pose.translation.z
        );
    }
}

fn toggle_abs(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<KeyBindings>,
    rebind: Option<Res<crate::ui::menu::RebindState>>,
    mut vehicles: Query<&mut Vehicle, With<crate::vehicle::physics::VehicleTag>>,
) {
    if rebind.as_ref().is_some_and(|state| state.pending.is_some()) {
        return;
    }
    if !bindings.just_pressed(&keys, ControlAction::ToggleAbs) {
        return;
    }
    for mut vehicle in &mut vehicles {
        vehicle.dynamics.abs_enabled = !vehicle.dynamics.abs_enabled;
        info!(
            "ABS {}",
            if vehicle.dynamics.abs_enabled {
                "on"
            } else {
                "off"
            }
        );
    }
}

fn toggle_triple_power(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<KeyBindings>,
    rebind: Option<Res<crate::ui::menu::RebindState>>,
    mut vehicles: Query<&mut Vehicle, With<crate::vehicle::physics::VehicleTag>>,
) {
    if rebind.as_ref().is_some_and(|state| state.pending.is_some()) {
        return;
    }
    if !bindings.just_pressed(&keys, ControlAction::ToggleTriplePower) {
        return;
    }
    for mut vehicle in &mut vehicles {
        let enabled = vehicle.dynamics.toggle_triple_power();
        info!("POWER x3 {}", if enabled { "on" } else { "off" });
    }
}

fn handle_pause(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<KeyBindings>,
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
    mut exit: MessageWriter<AppExit>,
    mut menu_page: Option<ResMut<crate::ui::menu::PauseMenuPage>>,
    rebind: Option<ResMut<crate::ui::menu::RebindState>>,
) {
    if rebind.as_ref().is_some_and(|state| state.pending.is_some()) {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) || bindings.just_pressed(&keys, ControlAction::Menu) {
        match state.get() {
            GameState::Driving => next.set(GameState::Paused),
            GameState::Paused => {
                if let Some(page) = &mut menu_page
                    && **page != crate::ui::menu::PauseMenuPage::Main
                {
                    **page = crate::ui::menu::PauseMenuPage::Main;
                } else {
                    next.set(GameState::Driving);
                }
            }
            // Start-screen systems own these states and perform the correct
            // nested back action (name entry -> mode -> main menu).
            GameState::MainMenu | GameState::ModeSelection => {}
        }
    }
    if matches!(*state.get(), GameState::Paused | GameState::MainMenu)
        && bindings.just_pressed(&keys, ControlAction::Quit)
    {
        exit.write(AppExit::Success);
    }
}

/// Gameplay never needs the system pointer, while every menu is mouse-driven.
/// Applying this continuously also restores the requested policy after the
/// window regains focus or is recreated by a display-mode change.
fn sync_cursor_with_game_state(
    state: Res<State<GameState>>,
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let driving = *state.get() == GameState::Driving;
    for mut cursor in &mut cursors {
        cursor.visible = !driving;
        cursor.grab_mode = if driving {
            CursorGrabMode::Locked
        } else {
            CursorGrabMode::None
        };
    }
}

/// The global lighting state the quality switch has to keep in step.
#[derive(SystemParam)]
struct WorldLighting<'w, 's> {
    ambient: ResMut<'w, GlobalAmbientLight>,
    clear_color: ResMut<'w, ClearColor>,
    shadow_map: ResMut<'w, bevy::light::DirectionalLightShadowMap>,
    renderer: ResMut<'w, DefaultOpaqueRendererMethod>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    speed_effect: ResMut<'w, SpeedEffectSettings>,
    sun: Query<
        'w,
        's,
        (
            &'static mut DirectionalLight,
            &'static mut CascadeShadowConfig,
            &'static mut Transform,
        ),
        With<lighting::Sun>,
    >,
    performance_shadow_excluded: Query<'w, 's, Entity, With<PerformanceShadowExcluded>>,
}

/// `F5` steps through the lighting tiers.
///
fn cycle_lighting_quality(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<KeyBindings>,
    mut settings: ResMut<LightingSettings>,
    rebind: Option<Res<crate::ui::menu::RebindState>>,
) {
    if rebind.as_ref().is_some_and(|state| state.pending.is_some()) {
        return;
    }
    if !bindings.just_pressed(&keys, ControlAction::CycleQuality) {
        return;
    }
    let quality = settings.quality.next();
    settings.apply_preset(quality);
    info!("lighting quality: {}", quality.label());
}

/// Apply the complete graphics preset after F5 or a pause-menu selection.
///
/// This includes the opaque renderer method and dirties materials so switching
/// to Cinematic at runtime produces the same deferred/SSR image as starting in
/// Cinematic from the environment.
fn apply_lighting_quality(
    mut commands: Commands,
    settings: Res<LightingSettings>,
    render_device: Option<Res<RenderDevice>>,
    mut world_lighting: WorldLighting,
    mut cameras: Query<(Entity, &mut Projection), With<LitCamera>>,
) {
    // Simulation smoke tests intentionally construct no render world. There is
    // nothing to apply there, and render-synced components must not be queued.
    if render_device.is_none() || !settings.is_changed() {
        return;
    }

    if settings.uses_reflections() {
        world_lighting.renderer.set_to_deferred();
    } else {
        world_lighting.renderer.set_to_forward();
    }
    // Renderer method is resolved while preparing StandardMaterial. Touching
    // every material makes Bevy rebuild those pipelines for the new preset.
    for _ in world_lighting.materials.iter_mut() {}

    *world_lighting.ambient =
        lighting::ambient_for(settings.uses_atmosphere(), settings.time_of_day);
    world_lighting.clear_color.0 = settings.sky_color();
    *world_lighting.shadow_map = lighting::shadow_map_settings(&settings);
    world_lighting.speed_effect.full_strength = settings.speed_effect_strength();
    for (mut light, mut cascades, mut transform) in &mut world_lighting.sun {
        light.illuminance = settings.sun_illuminance();
        light.color = settings.sun_color();
        light.shadow_maps_enabled = settings.shadow_quality != lighting::GraphicsLevel::Off;
        light.contact_shadows_enabled = settings.contact_shadows;
        light.soft_shadow_size = (settings.shadow_quality >= lighting::GraphicsLevel::High)
            .then_some(settings.sun_angular_size);
        *cascades = settings.cascade_shadow_config();
        let direction = settings.sun_direction();
        *transform = Transform::from_translation(-direction * 200.0).looking_to(direction, Vec3::Y);
    }

    for (camera, mut projection) in &mut cameras {
        if let Projection::Perspective(perspective) = &mut *projection {
            perspective.far = settings.view_distance.meters();
        }
        let mut camera_commands = commands.entity(camera);
        lighting::switch_camera_quality(&mut camera_commands, &settings);
    }

    for entity in &world_lighting.performance_shadow_excluded {
        if settings.shadow_quality <= lighting::GraphicsLevel::Low {
            commands.entity(entity).insert(NotShadowCaster);
        } else {
            commands.entity(entity).remove::<NotShadowCaster>();
        }
    }
}

fn sync_collision_effect_setting(
    settings: Res<LightingSettings>,
    mut collision_effect: ResMut<CollisionEffectSettings>,
) {
    if settings.is_changed() {
        collision_effect.enabled = settings.collision_effect_enabled();
    }
}

fn pause_physics(mut time: ResMut<Time<Physics>>) {
    time.pause();
}

fn resume_physics(mut time: ResMut<Time<Physics>>) {
    time.unpause();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor_after_sync(state: GameState) -> CursorOptions {
        let mut app = App::new();
        app.insert_resource(State::new(state))
            .add_systems(Update, sync_cursor_with_game_state)
            .world_mut()
            .spawn((PrimaryWindow, CursorOptions::default()));
        app.update();
        let world = app.world_mut();
        let mut cursor = world.query_filtered::<&CursorOptions, With<PrimaryWindow>>();
        cursor.single(world).expect("primary-window cursor").clone()
    }

    #[test]
    fn cursor_is_hidden_and_locked_only_during_the_race() {
        let driving = cursor_after_sync(GameState::Driving);
        assert!(!driving.visible);
        assert_eq!(driving.grab_mode, CursorGrabMode::Locked);

        for state in [
            GameState::MainMenu,
            GameState::ModeSelection,
            GameState::Paused,
        ] {
            let menu = cursor_after_sync(state);
            assert!(menu.visible);
            assert_eq!(menu.grab_mode, CursorGrabMode::None);
        }
    }

    #[test]
    fn graphics_toggle_reaches_the_runtime_collision_effect() {
        let mut graphics = LightingSettings::default();
        graphics.collision_effect = false;
        let mut app = App::new();
        app.insert_resource(graphics)
            .insert_resource(CollisionEffectSettings::default())
            .add_systems(Update, sync_collision_effect_setting);
        app.update();
        assert!(!app.world().resource::<CollisionEffectSettings>().enabled);
    }

    #[test]
    fn rear_view_is_active_only_while_the_bound_key_is_held() {
        let target = Entity::from_bits(51);
        let mut app = App::new();
        app.insert_resource(State::new(GameState::Driving))
            .insert_resource(KeyBindings::default())
            .insert_resource(ButtonInput::<KeyCode>::default())
            .add_systems(Update, hold_rear_view)
            .world_mut()
            .spawn(GameCamera::new(target));

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyB);
        app.update();
        assert!(
            app.world_mut()
                .query::<&GameCamera>()
                .single(app.world())
                .unwrap()
                .rear_view
        );

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyB);
        app.update();
        assert!(
            !app.world_mut()
                .query::<&GameCamera>()
                .single(app.world())
                .unwrap()
                .rear_view
        );
    }

    #[test]
    fn angle_bracket_views_override_rear_view_and_release_cleanly() {
        let target = Entity::from_bits(52);
        let mut app = App::new();
        app.insert_resource(State::new(GameState::Driving))
            .insert_resource(KeyBindings::default())
            .insert_resource(ButtonInput::<KeyCode>::default())
            .add_systems(Update, hold_rear_view)
            .world_mut()
            .spawn(GameCamera::new(target));

        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::KeyB);
            keys.press(KeyCode::Comma);
        }
        app.update();
        let camera = app
            .world_mut()
            .query::<&GameCamera>()
            .single(app.world())
            .unwrap();
        assert_eq!(camera.side_view, SideView::Left);
        assert!(!camera.rear_view);

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Comma);
        app.update();
        let camera = app
            .world_mut()
            .query::<&GameCamera>()
            .single(app.world())
            .unwrap();
        assert_eq!(camera.side_view, SideView::None);
        assert!(camera.rear_view);
    }

    #[test]
    fn roof_orientation_is_detected_but_normal_cornering_is_not() {
        assert!(is_upside_down(Quat::from_rotation_z(core::f32::consts::PI)));
        assert!(!is_upside_down(Quat::IDENTITY));
        assert!(!is_upside_down(Quat::from_rotation_z(1.2)));
    }
}
