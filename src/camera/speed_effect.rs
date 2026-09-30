//! Speed-driven peripheral blur and tunnel vision.
//!
//! On every quality tier the effect is a fullscreen GPU pass attached to the
//! game camera. It runs before tonemapping, after the 3D scene has been rendered,
//! so the HUD stays perfectly sharp. Below 120 km/h it is an exact one-sample
//! pass-through. Above that the strength follows a smooth curve up to the car's
//! maximum-speed region.

use bevy::core_pipeline::fullscreen_material::{FullscreenMaterial, FullscreenMaterialPlugin};
use bevy::prelude::*;
use bevy::render::{extract_component::ExtractComponent, render_resource::ShaderType};
use bevy::shader::ShaderRef;

use super::controller::{GameCamera, smoothing_factor};
use crate::game::GameState;
use crate::vehicle::physics::{Vehicle, VehicleTag};
use avian3d::prelude::{CollisionStart, ContactGraph};

/// Nitro adds this much effect on top of the speed curve. At 400 km/h the
/// combined value reaches 7.0, nearly three times the already-strengthened
/// normal maximum.
const NITRO_EFFECT_BONUS: f32 = 4.5;
const NITRO_EFFECT_ATTACK_RATE: f32 = 15.0;

/// Tuning for the temporary loss of visual focus after an impact.
#[derive(Debug, Clone, Resource, Reflect)]
#[reflect(Resource)]
pub struct CollisionEffectSettings {
    pub enabled: bool,
    /// Impact-induced velocity change below which tiny contacts are ignored [m/s].
    pub minimum_delta_speed: f32,
    /// Impact-induced velocity change that produces the maximum five-second effect [m/s].
    pub full_delta_speed: f32,
    pub minimum_duration: f32,
    pub maximum_duration: f32,
}

impl Default for CollisionEffectSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            minimum_delta_speed: 1.5,
            full_delta_speed: 25.0,
            minimum_duration: 1.0,
            maximum_duration: 5.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollisionResponse {
    pub intensity: f32,
    pub duration: f32,
}

/// Maps the solved contact impulse per kilogram (a velocity change) to a strong
/// blur and a recovery duration in the requested one-to-five-second range.
pub fn collision_response(
    delta_speed: f32,
    settings: &CollisionEffectSettings,
) -> Option<CollisionResponse> {
    if !settings.enabled || delta_speed < settings.minimum_delta_speed {
        return None;
    }
    let range = (settings.full_delta_speed - settings.minimum_delta_speed).max(f32::EPSILON);
    let linear = ((delta_speed - settings.minimum_delta_speed) / range).clamp(0.0, 1.0);
    let severity = linear * linear * (3.0 - 2.0 * linear);
    Some(CollisionResponse {
        // Even the smallest accepted collision is deliberately conspicuous.
        intensity: 0.68 + severity * 0.32,
        duration: settings.minimum_duration
            + severity * (settings.maximum_duration - settings.minimum_duration),
    })
}

/// Runtime envelope shared by the single player camera.
#[derive(Debug, Resource)]
pub struct CollisionBlurState {
    remaining: f32,
    duration: f32,
    peak: f32,
    direction: Vec2,
}

impl Default for CollisionBlurState {
    fn default() -> Self {
        Self {
            remaining: 0.0,
            duration: 0.0,
            peak: 0.0,
            direction: Vec2::new(0.86, 0.51),
        }
    }
}

impl CollisionBlurState {
    fn trigger(&mut self, response: CollisionResponse, direction: Vec2) {
        let live = self.intensity();
        self.duration = self.remaining.max(response.duration);
        self.remaining = self.duration;
        self.peak = live.max(response.intensity);
        if direction.length_squared() > 1.0e-6 {
            self.direction = direction.normalize();
        }
    }

    fn intensity(&self) -> f32 {
        if self.duration <= 0.0 || self.remaining <= 0.0 {
            return 0.0;
        }
        let x = (self.remaining / self.duration).clamp(0.0, 1.0);
        self.peak * x * x * (3.0 - 2.0 * x)
    }

    fn tick(&mut self, dt: f32) -> f32 {
        let intensity = self.intensity();
        self.remaining = (self.remaining - dt.max(0.0)).max(0.0);
        if self.remaining == 0.0 {
            self.duration = 0.0;
            self.peak = 0.0;
        }
        intensity
    }

    fn clear(&mut self) {
        self.remaining = 0.0;
        self.duration = 0.0;
        self.peak = 0.0;
    }
}

/// Tuning for the relationship between road speed and the visual effect.
#[derive(Debug, Clone, Resource, Reflect)]
#[reflect(Resource)]
pub struct SpeedEffectSettings {
    /// The effect is completely absent at and below this speed [km/h].
    pub start_kmh: f32,
    /// The effect reaches full strength here [km/h].
    pub full_kmh: f32,
    /// Strength multiplier at [`Self::full_kmh`] relative to the original look.
    pub full_strength: f32,
    /// How quickly the effect builds while accelerating [1/s].
    pub attack_rate: f32,
    /// How quickly clear peripheral vision returns while slowing [1/s].
    pub release_rate: f32,
}

impl Default for SpeedEffectSettings {
    fn default() -> Self {
        Self {
            start_kmh: 120.0,
            full_kmh: 400.0,
            full_strength: 2.5,
            attack_rate: 5.0,
            release_rate: 7.0,
        }
    }
}

/// Per-camera values uploaded to the fullscreen shader.
///
/// Eight floats deliberately fill two 16-byte uniform slots, which also keeps
/// the component compatible with WebGL-style uniform alignment rules.
#[derive(Debug, Clone, Copy, Component, ExtractComponent, ShaderType)]
pub struct SpeedTunnelEffect {
    /// Smoothed live effect strength: up to 2.5 normally and 7.0 with nitro.
    pub intensity: f32,
    /// Maximum length of the radial sampling streak in UV-relative units.
    pub radial_blur: f32,
    /// Maximum peripheral darkening.
    pub vignette: f32,
    /// Maximum red/blue separation in UV units.
    pub chromatic_aberration: f32,
    /// Live nitro amount; tightens the sharp centre independently of speed.
    pub nitro: f32,
    /// Full-frame loss of focus caused by a collision, normalized to `0..=1`.
    pub impact_blur: f32,
    /// Stable smear direction gives the collision a double-vision character.
    pub impact_direction: Vec2,
}

impl Default for SpeedTunnelEffect {
    fn default() -> Self {
        Self {
            intensity: 0.0,
            radial_blur: 0.085,
            vignette: 0.52,
            chromatic_aberration: 0.0035,
            nitro: 0.0,
            impact_blur: 0.0,
            impact_direction: Vec2::new(0.86, 0.51),
        }
    }
}

impl FullscreenMaterial for SpeedTunnelEffect {
    fn fragment_shader() -> ShaderRef {
        "shaders/speed_tunnel.wgsl".into()
    }
}

pub struct SpeedEffectPlugin;

impl Plugin for SpeedEffectPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FullscreenMaterialPlugin::<SpeedTunnelEffect>::default())
            .init_resource::<SpeedEffectSettings>()
            .init_resource::<CollisionEffectSettings>()
            .init_resource::<CollisionBlurState>()
            .register_type::<SpeedEffectSettings>()
            .register_type::<CollisionEffectSettings>()
            .add_systems(
                PostUpdate,
                (capture_collision_effect, update_speed_effect).chain(),
            );
    }
}

/// Convert road speed to an effect target using a smoothstep curve.
///
/// Smoothstep makes crossing 120 km/h imperceptible as a threshold while still
/// leaving a pronounced progression through 200, 300 and 400 km/h.
pub fn intensity_for_speed(speed_kmh: f32, settings: &SpeedEffectSettings) -> f32 {
    let range = (settings.full_kmh - settings.start_kmh).max(f32::EPSILON);
    let x = ((speed_kmh.abs() - settings.start_kmh) / range).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x) * settings.full_strength.max(0.0)
}

fn capture_collision_effect(
    mut collisions: MessageReader<CollisionStart>,
    game_state: Res<State<GameState>>,
    settings: Res<CollisionEffectSettings>,
    contacts: Res<ContactGraph>,
    vehicles: Query<(Entity, &Vehicle), With<VehicleTag>>,
    mut blur: ResMut<CollisionBlurState>,
) {
    if !settings.enabled {
        blur.clear();
    }
    let active = settings.enabled && *game_state.get() == GameState::Driving;
    let vehicle = vehicles.single().ok();

    for collision in collisions.read() {
        let Some((vehicle_entity, vehicle)) = vehicle else {
            continue;
        };
        if !active
            || ![
                Some(collision.collider1),
                Some(collision.collider2),
                collision.body1,
                collision.body2,
            ]
            .contains(&Some(vehicle_entity))
        {
            continue;
        }
        let Some((_edge, pair)) = contacts.get(collision.collider1, collision.collider2) else {
            continue;
        };
        let delta_speed = pair.total_normal_impulse_magnitude() / vehicle.config.mass.max(1.0);
        let Some(response) = collision_response(delta_speed, &settings) else {
            continue;
        };

        let mut impulse = pair.max_normal_impulse();
        if pair.collider1 == vehicle_entity || pair.body1 == Some(vehicle_entity) {
            impulse = -impulse;
        }
        let direction = Vec2::new(impulse.x, -impulse.z);
        blur.trigger(response, direction);
    }
}

fn update_speed_effect(
    time: Res<Time>,
    settings: Res<SpeedEffectSettings>,
    collision_settings: Res<CollisionEffectSettings>,
    mut collision_blur: ResMut<CollisionBlurState>,
    vehicles: Query<&Vehicle>,
    mut cameras: Query<(&GameCamera, &mut SpeedTunnelEffect)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let impact = if collision_settings.enabled {
        collision_blur.tick(dt)
    } else {
        collision_blur.clear();
        0.0
    };
    let impact_direction = collision_blur.direction;

    for (camera, mut effect) in &mut cameras {
        if settings.full_strength <= 0.0 {
            effect.intensity = 0.0;
            effect.nitro = 0.0;
        }
        let Ok(vehicle) = vehicles.get(camera.target) else {
            continue;
        };
        // `full_strength == 0` is the player's explicit "effect off" setting,
        // which also disables nitro blur while leaving the physical boost on.
        let nitro = if settings.full_strength > 0.0 {
            vehicle.nitro_strength()
        } else {
            0.0
        };
        let target =
            intensity_for_speed(vehicle.speed_kmh(), &settings) + nitro * NITRO_EFFECT_BONUS;
        let rate = if target > effect.intensity {
            if nitro > 0.0 {
                NITRO_EFFECT_ATTACK_RATE
            } else {
                settings.attack_rate
            }
        } else {
            settings.release_rate
        };
        effect.intensity += (target - effect.intensity) * smoothing_factor(rate.max(0.0), dt);
        effect.nitro = nitro;
        effect.impact_blur = impact;
        effect.impact_direction = impact_direction;

        // Avoid leaving a microscopic non-zero value after the car has slowed;
        // at exactly zero the shader is a clean copy of the original frame.
        if target == 0.0 && effect.intensity < 1.0e-4 {
            effect.intensity = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::VehicleKind;
    use avian3d::prelude::{ContactEdge, ContactManifold, ContactPoint};

    #[test]
    fn effect_starts_only_above_120_kmh() {
        let settings = SpeedEffectSettings::default();
        assert_eq!(intensity_for_speed(0.0, &settings), 0.0);
        assert_eq!(intensity_for_speed(120.0, &settings), 0.0);
        assert!(intensity_for_speed(121.0, &settings) > 0.0);
    }

    #[test]
    fn effect_grows_monotonically_to_full_strength() {
        let settings = SpeedEffectSettings::default();
        let samples = [120.0, 160.0, 200.0, 250.0, 300.0, 350.0, 400.0];
        for speeds in samples.windows(2) {
            assert!(
                intensity_for_speed(speeds[1], &settings)
                    > intensity_for_speed(speeds[0], &settings)
            );
        }
        assert_eq!(intensity_for_speed(400.0, &settings), 2.5);
        assert_eq!(intensity_for_speed(500.0, &settings), 2.5);
    }

    #[test]
    fn reverse_speed_uses_the_same_visual_curve() {
        let settings = SpeedEffectSettings::default();
        assert_eq!(
            intensity_for_speed(-300.0, &settings),
            intensity_for_speed(300.0, &settings)
        );
    }

    #[test]
    fn nitro_is_far_stronger_than_the_normal_maximum() {
        let settings = SpeedEffectSettings::default();
        let ordinary = intensity_for_speed(400.0, &settings);
        assert_eq!(ordinary, 2.5);
        assert_eq!(ordinary + NITRO_EFFECT_BONUS, 7.0);
    }

    #[test]
    fn collision_duration_scales_from_one_to_five_seconds() {
        let settings = CollisionEffectSettings::default();
        assert!(collision_response(settings.minimum_delta_speed - 0.01, &settings).is_none());

        let minimum = collision_response(settings.minimum_delta_speed, &settings).unwrap();
        assert!((minimum.duration - 1.0).abs() < f32::EPSILON);
        assert!(minimum.intensity >= 0.68);

        let maximum = collision_response(settings.full_delta_speed, &settings).unwrap();
        assert!((maximum.duration - 5.0).abs() < f32::EPSILON);
        assert!((maximum.intensity - 1.0).abs() < f32::EPSILON);

        let beyond = collision_response(settings.full_delta_speed * 2.0, &settings).unwrap();
        assert_eq!(beyond, maximum);
    }

    #[test]
    fn collision_blur_returns_smoothly_to_clear_vision() {
        let mut blur = CollisionBlurState::default();
        blur.trigger(
            CollisionResponse {
                intensity: 1.0,
                duration: 5.0,
            },
            Vec2::X,
        );
        let mut samples = Vec::new();
        for _ in 0..6 {
            samples.push(blur.tick(1.0));
        }
        assert_eq!(samples[0], 1.0);
        assert_eq!(samples[5], 0.0);
        assert!(samples.windows(2).all(|pair| pair[1] <= pair[0]));
        assert!(samples[1..5].iter().all(|value| *value > 0.0));
    }

    #[test]
    fn disabled_collision_effect_never_produces_a_response() {
        let settings = CollisionEffectSettings {
            enabled: false,
            ..default()
        };
        assert!(collision_response(100.0, &settings).is_none());
    }

    #[test]
    fn solved_player_collision_triggers_the_blur_envelope() {
        let mut app = App::new();
        app.insert_resource(State::new(GameState::Driving))
            .insert_resource(CollisionEffectSettings::default())
            .insert_resource(CollisionBlurState::default())
            .add_message::<CollisionStart>()
            .add_systems(Update, capture_collision_effect);

        let config = VehicleKind::Veyron.config();
        let mass = config.mass;
        let vehicle = app
            .world_mut()
            .spawn((Vehicle::new(config, Transform::IDENTITY), VehicleTag))
            .id();
        let barrier = app.world_mut().spawn_empty().id();

        let mut point = ContactPoint::new(Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, 0.02);
        point.normal_impulse = mass * 25.0;
        let manifold = ContactManifold::new([point], Vec3::X);
        let mut contacts = ContactGraph::default();
        contacts.add_edge_with(ContactEdge::new(vehicle, barrier), |pair| {
            pair.body1 = Some(vehicle);
            pair.manifolds.push(manifold.clone());
        });
        app.insert_resource(contacts);
        app.world_mut().write_message(CollisionStart {
            collider1: vehicle,
            collider2: barrier,
            body1: Some(vehicle),
            body2: None,
        });

        app.update();
        let blur = app.world().resource::<CollisionBlurState>();
        assert_eq!(blur.intensity(), 1.0);
        assert_eq!(blur.remaining, 5.0);
        assert_eq!(blur.direction, Vec2::NEG_X);
    }

    #[test]
    fn fullscreen_collision_shader_is_valid_wgsl() {
        let source = include_str!("../../assets/shaders/speed_tunnel.wgsl").replacen(
            "#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput",
            "struct FullscreenVertexOutput {\n\
                 @builtin(position) position: vec4<f32>,\n\
                 @location(0) uv: vec2<f32>,\n\
             };",
            1,
        );
        let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|error| {
            panic!(
                "invalid speed/collision fullscreen shader:\n{}",
                error.emit_to_string(&source)
            )
        });
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("speed/collision fullscreen shader failed semantic validation");
    }
}
