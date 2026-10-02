//! Lighting.
//!
//! Three quality tiers, chosen at start-up with the `VEYRON_QUALITY`
//! environment variable and cycled at run time with `F5`:
//!
//! | tier | what it is |
//! |---|---|
//! | `performance` | one directional sun, authored skybox, flat ambient, distance fog, 2x MSAA and two shadow cascades. This is the default. |
//! | `balanced` | authored skybox, soft (PCSS) shadows, SSAO, contact shadows, bloom, ACES tonemapping and TAA. |
//! | `cinematic` | as balanced, plus screen space reflections through the deferred path, ultra SSAO, a bigger shadow map and subtle motion blur. |
//!
//! Day and night use separate authored cubemaps. Directional and ambient
//! lighting remain synchronised with each
//! image, while SSAO and contact shadows put back the short-range occlusion a
//! directional light alone cannot express.
//!
//! Genuine ray-traced global illumination is available too - Bevy ships it as
//! `bevy_solari` - behind the optional `raytracing` cargo feature. It needs
//! hardware ray query support, which today means Vulkan or DirectX 12; Metal
//! does not expose it, so it is off by default and will not help on a Mac.

use bevy::anti_alias::contrast_adaptive_sharpening::ContrastAdaptiveSharpening;
use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::camera::{Exposure, Hdr};
use bevy::core_pipeline::prepass::{DeferredPrepass, DepthPrepass};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::ecs::system::EntityCommands;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::atmosphere::ScatteringMedium;
use bevy::light::{
    Atmosphere, AtmosphereEnvironmentMapLight, CascadeShadowConfig, CascadeShadowConfigBuilder,
    DirectionalLightShadowMap, ShadowFilteringMethod, Skybox, light_consts::lux,
};
use bevy::pbr::{
    AtmosphereMode, AtmosphereSettings, ContactShadows, ScreenSpaceAmbientOcclusion,
    ScreenSpaceAmbientOcclusionQualityLevel, ScreenSpaceReflections,
};
use bevy::post_process::bloom::Bloom;
use bevy::post_process::motion_blur::MotionBlur;
use bevy::prelude::*;
use bevy::render::render_resource::{TextureViewDescriptor, TextureViewDimension};
use serde::{Deserialize, Serialize};

/// Colour of the sky, used for the clear colour and the fog on the
/// [`LightingQuality::Performance`] tier, where there is no simulated sky.
pub const SKY_COLOR: Color = Color::srgb(0.48, 0.66, 0.88);

/// Set by the phone build. Vulkan guarantees only 1x and 4x multisampling on
/// every GPU and many mobile GPUs reject 2x, so a 2x request (the Performance
/// preset) renders without MSAA there; 4x and TAA remain selectable.
pub static MSAA_2X_UNAVAILABLE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Environment variable that selects the tier at start-up.
pub const QUALITY_ENV: &str = "VEYRON_QUALITY";
/// Environment variable that overrides the camera exposure, in EV100. Lower is
/// brighter; each whole number is one stop.
pub const EXPOSURE_ENV: &str = "VEYRON_EXPOSURE";

/// How much of the lighting stack to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect, Serialize, Deserialize)]
pub enum LightingQuality {
    /// Directional sun, flat ambient, distance fog. No cinematic lighting passes.
    #[default]
    Performance,
    /// Simulated sky, sky-driven image based lighting, soft shadows, SSAO,
    /// contact shadows, bloom, ACES, TAA.
    Balanced,
    /// Everything, including screen space reflections.
    Cinematic,
}

/// Coarse time-of-day presets.  They intentionally describe the look of the
/// world rather than a clock: this keeps the setting predictable and cheap to
/// change from the pause menu while still driving the sun, sky and headlights.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect, Serialize)]
pub enum TimeOfDay {
    #[default]
    Day,
    Night,
}

// Keep old settings files readable after removing the sunset option. A saved
// `Sunset` is migrated to night while every other graphics/audio/control value
// in the same file remains intact.
#[derive(Deserialize)]
enum StoredTimeOfDay {
    Day,
    Sunset,
    Night,
}

impl<'de> Deserialize<'de> for TimeOfDay {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(match StoredTimeOfDay::deserialize(deserializer)? {
            StoredTimeOfDay::Day => Self::Day,
            StoredTimeOfDay::Sunset | StoredTimeOfDay::Night => Self::Night,
        })
    }
}

impl TimeOfDay {
    pub const ALL: [Self; 2] = [Self::Day, Self::Night];

    pub fn next(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Day => "DAY",
            Self::Night => "NIGHT",
        }
    }

    #[inline]
    const fn skybox_index(self) -> usize {
        match self {
            Self::Day => 0,
            Self::Night => 1,
        }
    }

    /// Approximate sky luminance before camera exposure is applied [cd/m²].
    /// Darker presets use much lower values because their exposure offset makes
    /// the virtual camera correspondingly more light-sensitive.
    pub const fn skybox_brightness(self) -> f32 {
        match self {
            Self::Day => 3_000.0,
            Self::Night => 4.0,
        }
    }

    #[inline]
    pub fn headlights_enabled(self) -> bool {
        self == Self::Night
    }

    /// Relative road-illumination power. The night low beam is deliberately
    /// strong enough to keep the road and lane markings readable at speed.
    #[inline]
    pub fn headlight_intensity_multiplier(self) -> f32 {
        match self {
            Self::Day => 0.0,
            Self::Night => 5.0,
        }
    }

    /// Eye/camera adaptation for each broad lighting condition, in stops.
    /// Without this a physically dim night scene is correctly exposed for
    /// noon and therefore appears almost completely black.
    pub fn exposure_offset(self) -> f32 {
        match self {
            Self::Day => 0.0,
            Self::Night => -6.0,
        }
    }
}

/// Five-level selector shared by the effects that have meaningful quality tiers.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Reflect, Serialize, Deserialize,
)]
pub enum GraphicsLevel {
    Off,
    Low,
    #[default]
    Medium,
    High,
    Ultra,
}

impl GraphicsLevel {
    pub const ALL: [Self; 5] = [Self::Off, Self::Low, Self::Medium, Self::High, Self::Ultra];

    pub fn next(self) -> Self {
        Self::ALL[(self as usize + 1) % Self::ALL.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
            Self::Ultra => "ULTRA",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect, Serialize, Deserialize)]
pub enum AntiAliasingQuality {
    Off,
    Msaa2,
    Msaa4,
    #[default]
    Taa,
}

impl AntiAliasingQuality {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Msaa2,
            Self::Msaa2 => Self::Msaa4,
            Self::Msaa4 => Self::Taa,
            Self::Taa => Self::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::Msaa2 => "MSAA 2X",
            Self::Msaa4 => "MSAA 4X",
            Self::Taa => "TAA",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect, Serialize, Deserialize)]
pub enum ViewDistance {
    Near,
    Medium,
    #[default]
    Far,
    Ultra,
}

impl ViewDistance {
    pub fn next(self) -> Self {
        match self {
            Self::Near => Self::Medium,
            Self::Medium => Self::Far,
            Self::Far => Self::Ultra,
            Self::Ultra => Self::Near,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Near => "NEAR",
            Self::Medium => "MEDIUM",
            Self::Far => "FAR",
            Self::Ultra => "ULTRA",
        }
    }

    pub fn meters(self) -> f32 {
        match self {
            Self::Near => 1_200.0,
            Self::Medium => 2_000.0,
            Self::Far => 4_000.0,
            Self::Ultra => 8_000.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect, Serialize, Deserialize)]
pub enum ExposureAdjustment {
    Darker,
    #[default]
    Normal,
    Brighter,
}

impl ExposureAdjustment {
    pub fn next(self) -> Self {
        match self {
            Self::Darker => Self::Normal,
            Self::Normal => Self::Brighter,
            Self::Brighter => Self::Darker,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Darker => "DARKER",
            Self::Normal => "NORMAL",
            Self::Brighter => "BRIGHTER",
        }
    }

    fn ev_offset(self) -> f32 {
        match self {
            Self::Darker => 0.75,
            Self::Normal => 0.0,
            Self::Brighter => -0.75,
        }
    }
}

impl LightingQuality {
    pub const ALL: [LightingQuality; 3] = [
        LightingQuality::Performance,
        LightingQuality::Balanced,
        LightingQuality::Cinematic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LightingQuality::Performance => "PERFORMANCE",
            LightingQuality::Balanced => "BALANCED",
            LightingQuality::Cinematic => "CINEMATIC",
        }
    }

    pub fn next(self) -> Self {
        match self {
            LightingQuality::Performance => LightingQuality::Balanced,
            LightingQuality::Balanced => LightingQuality::Cinematic,
            LightingQuality::Cinematic => LightingQuality::Performance,
        }
    }

    /// Read the tier from [`QUALITY_ENV`], falling back to the default.
    pub fn from_env() -> Self {
        match std::env::var(QUALITY_ENV) {
            Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
                "performance" | "low" | "fast" => LightingQuality::Performance,
                "balanced" | "medium" | "high" => LightingQuality::Balanced,
                "cinematic" | "ultra" | "lumen" => LightingQuality::Cinematic,
                other => {
                    warn!("unknown {QUALITY_ENV}={other}, using the default tier");
                    LightingQuality::default()
                }
            },
            Err(_) => LightingQuality::default(),
        }
    }
}

/// Everything the look of the scene is tuned with.
#[derive(Debug, Clone, Resource, Reflect, Serialize, Deserialize)]
#[reflect(Resource)]
#[serde(default)]
pub struct LightingSettings {
    pub quality: LightingQuality,
    pub time_of_day: TimeOfDay,
    pub shadow_quality: GraphicsLevel,
    pub ambient_occlusion: GraphicsLevel,
    pub reflection_quality: GraphicsLevel,
    pub atmosphere_quality: GraphicsLevel,
    pub speed_effect: GraphicsLevel,
    pub anti_aliasing: AntiAliasingQuality,
    pub view_distance: ViewDistance,
    pub exposure_adjustment: ExposureAdjustment,
    pub contact_shadows: bool,
    pub bloom: bool,
    pub motion_blur: bool,
    /// Strong temporary blur after a physical impact.
    pub collision_effect: bool,
    pub rearview_mirrors: bool,
    /// Wheel-contact tyre tracks and particulate smoke can be controlled
    /// independently after any preset is selected.
    pub tire_marks: bool,
    pub tire_smoke: bool,
    pub sharpening: bool,
    /// Height of the sun above the horizon [rad].
    pub sun_elevation: f32,
    /// Compass direction of the sun, measured from the road's +Z axis [rad].
    pub sun_azimuth: f32,
    /// Angular diameter of the sun used for penumbra width. The real sun is
    /// about half a degree across; a slightly larger value reads better on a
    /// low resolution shadow map.
    pub sun_angular_size: f32,
    /// Camera exposure. 15 is the classic "sunny 16" value.
    pub exposure_ev100: f32,
    pub bloom_intensity: f32,
    /// Furthest cascade [m]. The road runs to the horizon, so this trades
    /// shadow resolution near the car against how far shadows reach.
    pub shadow_distance: f32,
}

impl Default for LightingSettings {
    fn default() -> Self {
        let quality = LightingQuality::from_env();
        let mut settings = Self {
            quality,
            time_of_day: TimeOfDay::Day,
            shadow_quality: GraphicsLevel::Medium,
            ambient_occlusion: GraphicsLevel::Off,
            reflection_quality: GraphicsLevel::Off,
            atmosphere_quality: GraphicsLevel::Off,
            speed_effect: GraphicsLevel::Ultra,
            anti_aliasing: AntiAliasingQuality::Msaa2,
            view_distance: ViewDistance::Medium,
            exposure_adjustment: ExposureAdjustment::Normal,
            contact_shadows: false,
            bloom: false,
            motion_blur: false,
            collision_effect: true,
            rearview_mirrors: true,
            tire_marks: true,
            tire_smoke: true,
            sharpening: true,
            sun_elevation: 42.0_f32.to_radians(),
            sun_azimuth: 145.0_f32.to_radians(),
            sun_angular_size: 1.2,
            // Physically "sunny 16" is EV100 15, but Bevy's exposure carries its
            // own calibration constant and the scene's albedos are the real,
            // low ones - asphalt reflects 8 % of what hits it. Measured off a
            // render, 11.5 is where the road lands at the 0.22 sRGB a
            // photograph of dry tarmac actually has, with the sky bright but
            // not clipped. Override with `VEYRON_EXPOSURE`.
            exposure_ev100: exposure_from_env(11.5),
            bloom_intensity: 0.09,
            shadow_distance: 340.0,
        };
        settings.apply_preset(quality);
        settings
    }
}

/// Read [`EXPOSURE_ENV`], falling back to `default`.
fn exposure_from_env(default: f32) -> f32 {
    match std::env::var(EXPOSURE_ENV) {
        Ok(value) => value.trim().parse().unwrap_or_else(|_| {
            warn!("{EXPOSURE_ENV}={value} is not a number, using {default}");
            default
        }),
        Err(_) => default,
    }
}

impl LightingSettings {
    /// Apply a complete preset. Every field remains independently editable
    /// afterwards through the graphics menu.
    pub fn apply_preset(&mut self, quality: LightingQuality) {
        self.quality = quality;
        self.speed_effect = if quality == LightingQuality::Performance {
            GraphicsLevel::Off
        } else {
            GraphicsLevel::Ultra
        };
        self.exposure_adjustment = ExposureAdjustment::Normal;
        self.collision_effect = quality != LightingQuality::Performance;
        self.rearview_mirrors = quality != LightingQuality::Performance;
        self.tire_marks = quality != LightingQuality::Performance;
        self.tire_smoke = quality != LightingQuality::Performance;
        match quality {
            LightingQuality::Performance => {
                self.shadow_quality = GraphicsLevel::Low;
                self.ambient_occlusion = GraphicsLevel::Off;
                self.reflection_quality = GraphicsLevel::Off;
                self.atmosphere_quality = GraphicsLevel::Off;
                self.anti_aliasing =
                    if MSAA_2X_UNAVAILABLE.load(std::sync::atomic::Ordering::Relaxed) {
                        AntiAliasingQuality::Off
                    } else {
                        AntiAliasingQuality::Msaa2
                    };
                self.view_distance = ViewDistance::Medium;
                self.contact_shadows = false;
                self.bloom = false;
                self.motion_blur = false;
                self.sharpening = true;
            }
            LightingQuality::Balanced => {
                self.shadow_quality = GraphicsLevel::High;
                self.ambient_occlusion = GraphicsLevel::High;
                self.reflection_quality = GraphicsLevel::Off;
                self.atmosphere_quality = GraphicsLevel::High;
                self.anti_aliasing = AntiAliasingQuality::Taa;
                self.view_distance = ViewDistance::Far;
                self.contact_shadows = true;
                self.bloom = true;
                self.motion_blur = false;
                self.sharpening = true;
            }
            LightingQuality::Cinematic => {
                self.shadow_quality = GraphicsLevel::Ultra;
                self.ambient_occlusion = GraphicsLevel::Ultra;
                self.reflection_quality = GraphicsLevel::Ultra;
                self.atmosphere_quality = GraphicsLevel::Ultra;
                self.anti_aliasing = AntiAliasingQuality::Taa;
                self.view_distance = ViewDistance::Ultra;
                self.contact_shadows = true;
                self.bloom = true;
                self.motion_blur = true;
                self.sharpening = true;
            }
        }
    }

    /// The selected preset remains a convenient starting point; once any
    /// preset-controlled value changes, the menu can identify the result as a
    /// custom configuration without losing the user's base tier.
    pub fn matches_selected_preset(&self) -> bool {
        let mut preset = self.clone();
        preset.apply_preset(self.quality);
        self.shadow_quality == preset.shadow_quality
            && self.ambient_occlusion == preset.ambient_occlusion
            && self.reflection_quality == preset.reflection_quality
            && self.atmosphere_quality == preset.atmosphere_quality
            && self.speed_effect == preset.speed_effect
            && self.anti_aliasing == preset.anti_aliasing
            && self.view_distance == preset.view_distance
            && self.exposure_adjustment == preset.exposure_adjustment
            && self.contact_shadows == preset.contact_shadows
            && self.bloom == preset.bloom
            && self.motion_blur == preset.motion_blur
            && self.collision_effect == preset.collision_effect
            && self.rearview_mirrors == preset.rearview_mirrors
            && self.tire_marks == preset.tire_marks
            && self.tire_smoke == preset.tire_smoke
            && self.sharpening == preset.sharpening
    }

    pub fn uses_atmosphere(&self) -> bool {
        // A rendered atmosphere draws a full-screen sky after Bevy's cubemap
        // pass and would hide the authored day/night skyboxes. Keep the
        // atmosphere entity available for future weather effects, but use the
        // authored sky plus distance fog for every current preset.
        false
    }

    pub fn uses_reflections(&self) -> bool {
        self.reflection_quality != GraphicsLevel::Off
    }

    pub fn speed_effect_strength(&self) -> f32 {
        match self.speed_effect {
            GraphicsLevel::Off => 0.0,
            GraphicsLevel::Low => 1.0,
            GraphicsLevel::Medium => 1.5,
            GraphicsLevel::High => 2.0,
            GraphicsLevel::Ultra => 2.5,
        }
    }

    /// Extra exposure needed to compensate for the additional occlusion and
    /// contact-shadow passes on the upper presets. Lower EV100 is brighter.
    /// This keeps the same materials readable instead of making a quality
    /// upgrade look like a darker time of day.
    pub fn quality_exposure_offset(&self) -> f32 {
        let ambient_occlusion = match self.ambient_occlusion {
            GraphicsLevel::Off => 0.0,
            GraphicsLevel::Low => -0.10,
            GraphicsLevel::Medium => -0.20,
            GraphicsLevel::High => -0.35,
            GraphicsLevel::Ultra => -0.50,
        };
        let contact_shadows = if self.contact_shadows { -0.15 } else { 0.0 };
        let reflections = if self.uses_reflections() { -0.05 } else { 0.0 };
        ambient_occlusion + contact_shadows + reflections
    }

    pub fn camera_ev100(&self) -> f32 {
        self.exposure_ev100
            + self.exposure_adjustment.ev_offset()
            + self.time_of_day.exposure_offset()
            + self.quality_exposure_offset()
    }

    /// The authored skyboxes and physically dark material albedos need a
    /// curve that preserves shadow separation. ACES crushed those midtones on
    /// Balanced/Cinematic while Performance already used Tony successfully.
    pub fn tonemapping(&self) -> Tonemapping {
        if self.uses_atmosphere() {
            Tonemapping::AcesFitted
        } else {
            Tonemapping::TonyMcMapface
        }
    }

    pub fn tire_marks_enabled(&self) -> bool {
        self.tire_marks
    }

    pub fn tire_smoke_enabled(&self) -> bool {
        self.tire_smoke
    }

    pub fn mirrors_enabled(&self) -> bool {
        self.rearview_mirrors
    }

    pub fn collision_effect_enabled(&self) -> bool {
        self.collision_effect
    }

    /// Direction the sunlight travels in, world space.
    pub fn sun_direction(&self) -> Vec3 {
        let elevation = match self.time_of_day {
            TimeOfDay::Day => self.sun_elevation,
            // Below the horizon: the tiny night key light is only a cool fill.
            TimeOfDay::Night => -12.0_f32.to_radians(),
        };
        let (sin_elevation, cos_elevation) = elevation.sin_cos();
        let (sin_azimuth, cos_azimuth) = self.sun_azimuth.sin_cos();
        // Pointing *down* from the sun towards the ground.
        -Vec3::new(
            cos_elevation * sin_azimuth,
            sin_elevation,
            cos_elevation * cos_azimuth,
        )
        .normalize()
    }

    /// Directional-light intensity for the selected time of day.
    pub fn sun_illuminance(&self) -> f32 {
        let daylight = if self.uses_atmosphere() {
            lux::RAW_SUNLIGHT
        } else {
            lux::FULL_DAYLIGHT * 1.6
        };
        match self.time_of_day {
            TimeOfDay::Day => daylight,
            TimeOfDay::Night => daylight * 0.008,
        }
    }

    pub fn sun_color(&self) -> Color {
        match self.time_of_day {
            TimeOfDay::Day => Color::srgb(1.0, 0.98, 0.95),
            TimeOfDay::Night => Color::srgb(0.32, 0.42, 0.72),
        }
    }

    pub fn sky_color(&self) -> Color {
        match self.time_of_day {
            TimeOfDay::Day => SKY_COLOR,
            TimeOfDay::Night => Color::srgb(0.015, 0.025, 0.07),
        }
    }

    /// Shadow map resolution for the current tier.
    pub fn shadow_map_size(&self) -> usize {
        match self.shadow_quality {
            GraphicsLevel::Off | GraphicsLevel::Low => 1024,
            GraphicsLevel::Medium => 2048,
            GraphicsLevel::High => 4096,
            GraphicsLevel::Ultra => 4096,
        }
    }

    /// Directional-light cascade layout for the current tier.
    pub fn cascade_shadow_config(&self) -> CascadeShadowConfig {
        CascadeShadowConfigBuilder {
            num_cascades: match self.shadow_quality {
                GraphicsLevel::Off | GraphicsLevel::Low => 2,
                GraphicsLevel::Medium => 3,
                GraphicsLevel::High | GraphicsLevel::Ultra => 4,
            },
            minimum_distance: 0.3,
            maximum_distance: match self.shadow_quality {
                GraphicsLevel::Off | GraphicsLevel::Low => self.shadow_distance.min(160.0),
                GraphicsLevel::Medium => self.shadow_distance.min(240.0),
                GraphicsLevel::High => self.shadow_distance,
                GraphicsLevel::Ultra => self.shadow_distance.max(500.0),
            },
            first_cascade_far_bound: 14.0,
            overlap_proportion: 0.2,
        }
        .build()
    }

    fn ssao_quality(&self) -> Option<ScreenSpaceAmbientOcclusionQualityLevel> {
        match self.ambient_occlusion {
            GraphicsLevel::Off => None,
            GraphicsLevel::Low => Some(ScreenSpaceAmbientOcclusionQualityLevel::Low),
            GraphicsLevel::Medium => Some(ScreenSpaceAmbientOcclusionQualityLevel::Medium),
            GraphicsLevel::High => Some(ScreenSpaceAmbientOcclusionQualityLevel::High),
            GraphicsLevel::Ultra => Some(ScreenSpaceAmbientOcclusionQualityLevel::Ultra),
        }
    }
}

/// Marks the sun so it can be re-aimed at run time.
#[derive(Component)]
pub struct Sun;

/// Marks the entity carrying the simulated atmosphere.
#[derive(Component)]
pub struct SkyAtmosphere;

/// Marks a camera that the lighting tier applies to.
#[derive(Component)]
pub struct LitCamera;

const SKYBOX_PATHS: [&str; 2] = [
    "environment/skyboxes/day.png",
    "environment/skyboxes/night.png",
];

/// The two photographed cubemaps. PNG has no cubemap metadata, so each image is
/// loaded as a 1×6 vertical strip and converted to a cube texture once.
#[derive(Resource)]
pub struct TimeOfDaySkyboxes {
    handles: [Handle<Image>; 2],
    prepared: [bool; 2],
}

impl TimeOfDaySkyboxes {
    fn handle(&self, time: TimeOfDay) -> &Handle<Image> {
        &self.handles[time.skybox_index()]
    }
}

pub fn load_skybox_assets(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.insert_resource(TimeOfDaySkyboxes {
        handles: SKYBOX_PATHS.map(|path| asset_server.load(path)),
        prepared: [false; 2],
    });
}

/// Convert loaded strips to cubemaps and assign the image belonging to the
/// currently selected in-game time of day.
pub fn prepare_skyboxes(
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut assets: ResMut<TimeOfDaySkyboxes>,
    settings: Res<LightingSettings>,
    mut cameras: Query<&mut Skybox, With<LitCamera>>,
) {
    for index in 0..assets.handles.len() {
        if assets.prepared[index] {
            continue;
        }
        let handle = assets.handles[index].clone();
        if !asset_server.load_state(&handle).is_loaded() {
            continue;
        }
        let Some(mut image) = images.get_mut(&handle) else {
            continue;
        };
        if image.texture_descriptor.array_layer_count() == 1 {
            if image.height() != image.width() * 6 {
                error!(
                    "skybox {} must be a 1x6 strip, got {}x{}",
                    SKYBOX_PATHS[index],
                    image.width(),
                    image.height()
                );
                assets.prepared[index] = true;
                continue;
            }
            if let Err(error) = image.reinterpret_stacked_2d_as_array(6) {
                error!("could not prepare skybox {}: {error}", SKYBOX_PATHS[index]);
                assets.prepared[index] = true;
                continue;
            }
            image.texture_view_descriptor = Some(TextureViewDescriptor {
                dimension: Some(TextureViewDimension::Cube),
                ..default()
            });
        }
        // A cubemap sampler filters across matching face edges. Explicitly
        // override the global image sampler so pixel-art/nearest defaults can
        // never introduce a visible line while looking along the motorway.
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::ClampToEdge,
            address_mode_v: ImageAddressMode::ClampToEdge,
            address_mode_w: ImageAddressMode::ClampToEdge,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            ..default()
        });
        assets.prepared[index] = true;
        info!("skybox ready: {}", SKYBOX_PATHS[index]);
    }

    let index = settings.time_of_day.skybox_index();
    if !assets.prepared[index] {
        return;
    }
    let selected = assets.handle(settings.time_of_day).clone();
    let brightness = settings.time_of_day.skybox_brightness();
    for mut skybox in &mut cameras {
        if skybox.image.as_ref() != Some(&selected) {
            skybox.image = Some(selected.clone());
        }
        skybox.brightness = brightness;
    }
}

/// Spawn the sun and, on the upper tiers, the atmosphere.
pub fn spawn_lighting(
    commands: &mut Commands,
    settings: &LightingSettings,
    mediums: &mut Assets<ScatteringMedium>,
) {
    // With a simulated atmosphere the sun must be given its *raw*, unfiltered
    // illuminance, because the scattering model is what turns it into the light
    // that actually reaches the ground. Without one, the usual post-scattering
    // daylight figure is right.
    commands.spawn((
        Name::new("Sun"),
        Sun,
        DirectionalLight {
            color: settings.sun_color(),
            illuminance: settings.sun_illuminance(),
            shadow_maps_enabled: settings.shadow_quality != GraphicsLevel::Off,
            // Screen space contact shadows fill in the short range occlusion a
            // cascade cannot resolve - under the car, around the wheels.
            contact_shadows_enabled: settings.contact_shadows,
            // Percentage-closer soft shadows: the penumbra widens with distance
            // from the occluder, as a real sun's does.
            soft_shadow_size: (settings.shadow_quality >= GraphicsLevel::High)
                .then_some(settings.sun_angular_size),
            ..default()
        },
        settings.cascade_shadow_config(),
        Transform::from_translation(-settings.sun_direction() * 200.0)
            .looking_to(settings.sun_direction(), Vec3::Y),
    ));

    // Keep the atmosphere entity ready for possible future weather effects.
    // The current authored day/night cubemaps remain visible in every preset.
    let medium = mediums.add(ScatteringMedium::earth(256, 256));
    commands.spawn((
        Name::new("Atmosphere"),
        SkyAtmosphere,
        Atmosphere::earth(medium),
    ));
}

/// Ambient fill. Only used where there is no sky to light the scene.
pub fn ambient(uses_atmosphere: bool) -> GlobalAmbientLight {
    ambient_for(uses_atmosphere, TimeOfDay::Day)
}

/// Ambient fill, including the cool night tint for the
/// performance renderer (the atmosphere renderer gets its fill from the sky).
pub fn ambient_for(uses_atmosphere: bool, time_of_day: TimeOfDay) -> GlobalAmbientLight {
    if uses_atmosphere {
        // In daylight the generated environment map is the complete ambient
        // term. At dusk and night it approaches zero, so retain a very small
        // sky/moon fill: without it unlit parts of the road and cockpit collapse
        // to mathematical black instead of retaining real low-light detail.
        match time_of_day {
            TimeOfDay::Day => GlobalAmbientLight::NONE,
            TimeOfDay::Night => GlobalAmbientLight {
                color: Color::srgb(0.08, 0.12, 0.28),
                brightness: 55.0,
                ..default()
            },
        }
    } else {
        let (color, brightness) = match time_of_day {
            TimeOfDay::Day => (Color::srgb(0.62, 0.72, 0.9), 900.0),
            TimeOfDay::Night => (Color::srgb(0.08, 0.12, 0.28), 55.0),
        };
        GlobalAmbientLight {
            color,
            brightness,
            ..default()
        }
    }
}

pub fn shadow_map_settings(settings: &LightingSettings) -> DirectionalLightShadowMap {
    DirectionalLightShadowMap {
        size: settings.shadow_map_size(),
    }
}

/// Default distance haze used behind the authored cubemaps.
pub fn distance_fog() -> bevy::pbr::DistanceFog {
    distance_fog_for(TimeOfDay::Day)
}

pub fn distance_fog_for(time_of_day: TimeOfDay) -> bevy::pbr::DistanceFog {
    let (color, directional) = match time_of_day {
        TimeOfDay::Day => (SKY_COLOR, Color::srgb(1.0, 0.97, 0.88)),
        TimeOfDay::Night => (
            Color::srgb(0.015, 0.025, 0.07),
            Color::srgb(0.20, 0.28, 0.55),
        ),
    };
    bevy::pbr::DistanceFog {
        color,
        directional_light_color: directional,
        directional_light_exponent: 24.0,
        falloff: bevy::pbr::FogFalloff::Linear {
            start: 420.0,
            end: 1600.0,
        },
    }
}

/// The graphics selector controls atmospheric distance haze without covering
/// the user-selected day/night skybox with a generated atmosphere. `Off`
/// removes the pass entirely, which keeps the Performance preset lean.
pub fn atmospheric_haze_for(
    time_of_day: TimeOfDay,
    quality: GraphicsLevel,
) -> Option<bevy::pbr::DistanceFog> {
    let (start, end) = match quality {
        GraphicsLevel::Off => return None,
        GraphicsLevel::Low => (750.0, 2_300.0),
        GraphicsLevel::Medium => (550.0, 1_900.0),
        GraphicsLevel::High => (420.0, 1_600.0),
        GraphicsLevel::Ultra => (300.0, 1_450.0),
    };
    let mut fog = distance_fog_for(time_of_day);
    fog.falloff = bevy::pbr::FogFalloff::Linear { start, end };
    Some(fog)
}

/// Apply a tier's worth of rendering components to a freshly spawned camera.
///
/// This only ever *inserts*. Removing a component that the render world syncs
/// panics in an app built without a renderer - which is exactly how the
/// head-less tests build it - so tier switching is a separate function that is
/// only ever called at run time, where a render world certainly exists.
pub fn apply_to_camera(entity: &mut EntityCommands, settings: &LightingSettings) {
    // Keep the render target format stable while live presets change. Cached
    // post-processing pipelines must never render into an SDR attachment.
    entity.insert(Hdr);
    entity.insert(settings.tonemapping());
    entity.insert(if settings.shadow_quality >= GraphicsLevel::High {
        ShadowFilteringMethod::Temporal
    } else {
        ShadowFilteringMethod::Hardware2x2
    });

    match settings.anti_aliasing {
        AntiAliasingQuality::Off => {
            entity.insert(Msaa::Off);
        }
        AntiAliasingQuality::Msaa2 => {
            entity.insert(
                if MSAA_2X_UNAVAILABLE.load(std::sync::atomic::Ordering::Relaxed) {
                    Msaa::Off
                } else {
                    Msaa::Sample2
                },
            );
        }
        AntiAliasingQuality::Msaa4 => {
            entity.insert(Msaa::Sample4);
        }
        AntiAliasingQuality::Taa => {
            entity.insert((Msaa::Off, TemporalAntiAliasing::default()));
        }
    }

    entity.insert((
        Hdr,
        Exposure {
            ev100: settings.camera_ev100(),
        },
    ));

    if settings.uses_atmosphere() {
        entity.insert((
            AtmosphereSettings {
                rendering_method: if settings.atmosphere_quality == GraphicsLevel::Ultra {
                    AtmosphereMode::Raymarched
                } else {
                    AtmosphereMode::LookupTexture
                },
                // The road is 33 km long, so aerial perspective has to stay
                // meaningful well past the default range.
                aerial_view_lut_max_distance: 4.0e4,
                ..default()
            },
            AtmosphereEnvironmentMapLight {
                intensity: 1.0,
                size: UVec2::splat(match settings.atmosphere_quality {
                    GraphicsLevel::Low => 128,
                    GraphicsLevel::Medium | GraphicsLevel::High => 256,
                    GraphicsLevel::Ultra => 512,
                    GraphicsLevel::Off => 128,
                }),
                ..default()
            },
        ));
    } else if let Some(haze) =
        atmospheric_haze_for(settings.time_of_day, settings.atmosphere_quality)
    {
        entity.insert(haze);
    }

    if let Some(quality_level) = settings.ssao_quality() {
        entity.insert(ScreenSpaceAmbientOcclusion {
            quality_level,
            // Keep tight contact detail without turning broad creases,
            // interiors and dark bodywork into solid black shapes.
            constant_object_thickness: 0.12,
        });
    }

    if settings.contact_shadows && settings.shadow_quality != GraphicsLevel::Off {
        entity.insert(ContactShadows {
            linear_steps: match settings.shadow_quality {
                GraphicsLevel::Ultra => 12,
                GraphicsLevel::High => 8,
                _ => 4,
            },
            thickness: 0.12,
            length: 0.35,
        });
    }

    if settings.sharpening {
        entity.insert(ContrastAdaptiveSharpening {
            enabled: true,
            sharpening_strength: 0.35,
            denoise: true,
        });
    }

    if settings.bloom {
        entity.insert(Bloom {
            intensity: settings.bloom_intensity,
            ..Bloom::NATURAL
        });
    }

    if settings.uses_reflections() {
        // Screen space reflections only exist on the deferred path, which needs
        // both prepasses.
        entity.insert((
            DepthPrepass,
            DeferredPrepass,
            ScreenSpaceReflections {
                // Asphalt is rougher than the default cut-off, and a motorway
                // that reflects nothing at all looks like a photograph of a
                // road rather than a road. Extending the fade-out is what puts
                // the sky and the car back into the tarmac.
                max_perceptual_roughness: 0.70..0.82,
                linear_steps: match settings.reflection_quality {
                    GraphicsLevel::Low => 6,
                    GraphicsLevel::Medium => 10,
                    GraphicsLevel::High => 14,
                    GraphicsLevel::Ultra => 20,
                    GraphicsLevel::Off => 6,
                },
                bisection_steps: if settings.reflection_quality >= GraphicsLevel::High {
                    6
                } else {
                    3
                },
                ..default()
            },
        ));
    }

    if settings.motion_blur {
        entity.insert((
            DepthPrepass,
            MotionBlur {
                shutter_angle: 0.4,
                samples: 2,
            },
        ));
    }
}

/// Move an existing camera to another tier.
///
/// Unlike [`apply_to_camera`] this removes the components the new tier does not
/// want, so it must only be used at run time.
pub fn switch_camera_quality(entity: &mut EntityCommands, settings: &LightingSettings) {
    entity.remove::<(
        Exposure,
        Bloom,
        TemporalAntiAliasing,
        ScreenSpaceAmbientOcclusion,
        ContactShadows,
        ContrastAdaptiveSharpening,
    )>();
    entity.remove::<(
        AtmosphereSettings,
        AtmosphereEnvironmentMapLight,
        ScreenSpaceReflections,
        DepthPrepass,
        DeferredPrepass,
        MotionBlur,
        bevy::pbr::DistanceFog,
    )>();
    apply_to_camera(entity, settings);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_tiers_cycle() {
        let mut quality = LightingQuality::Performance;
        for _ in 0..3 {
            quality = quality.next();
        }
        assert_eq!(quality, LightingQuality::Performance);
        for tier in LightingQuality::ALL {
            assert!(!tier.label().is_empty());
        }
    }

    #[test]
    fn performance_is_the_safe_default() {
        assert_eq!(LightingQuality::default(), LightingQuality::Performance);
    }

    #[test]
    fn performance_disables_all_optional_vehicle_and_screen_effects() {
        let mut settings = LightingSettings::default();
        settings.apply_preset(LightingQuality::Cinematic);
        assert!(settings.tire_marks_enabled());
        assert!(settings.tire_smoke_enabled());
        assert!(settings.mirrors_enabled());
        assert!(settings.speed_effect_strength() > 0.0);
        settings.apply_preset(LightingQuality::Performance);
        assert!(settings.matches_selected_preset());
        assert!(!settings.tire_marks_enabled());
        assert!(!settings.tire_smoke_enabled());
        assert!(!settings.mirrors_enabled());
        assert!(!settings.collision_effect_enabled());
        assert_eq!(settings.speed_effect_strength(), 0.0);
        assert!(!settings.bloom);
        assert!(!settings.motion_blur);

        // Performance is only a starting point: any individual feature can be
        // re-enabled and must then really work at run time.
        settings.tire_marks = true;
        settings.tire_smoke = true;
        settings.rearview_mirrors = true;
        settings.speed_effect = GraphicsLevel::Ultra;
        settings.reflection_quality = GraphicsLevel::Ultra;
        settings.collision_effect = true;
        assert!(!settings.matches_selected_preset());
        assert!(settings.tire_marks_enabled());
        assert!(settings.tire_smoke_enabled());
        assert!(settings.mirrors_enabled());
        assert!(settings.collision_effect_enabled());
        assert_eq!(settings.speed_effect_strength(), 2.5);
        assert!(settings.uses_reflections());
        settings.apply_preset(LightingQuality::Performance);
        assert!(settings.matches_selected_preset());
    }

    #[test]
    fn preset_reflections_follow_the_selected_level_not_the_tier() {
        let mut performance = LightingSettings::default();
        performance.apply_preset(LightingQuality::Performance);
        assert!(!performance.uses_reflections());
        performance.reflection_quality = GraphicsLevel::Low;
        assert!(performance.uses_reflections());

        let mut balanced = LightingSettings::default();
        balanced.apply_preset(LightingQuality::Balanced);
        assert!(!balanced.uses_reflections());

        let mut cinematic = LightingSettings::default();
        cinematic.apply_preset(LightingQuality::Cinematic);
        assert!(cinematic.uses_reflections());

        let mut night = LightingSettings {
            time_of_day: TimeOfDay::Night,
            ..default()
        };
        night.apply_preset(LightingQuality::Cinematic);
        assert!(!night.uses_atmosphere());
    }

    #[test]
    fn custom_atmospheric_haze_can_be_enabled_on_performance() {
        let mut settings = LightingSettings::default();
        settings.apply_preset(LightingQuality::Performance);
        assert!(atmospheric_haze_for(settings.time_of_day, settings.atmosphere_quality).is_none());
        settings.atmosphere_quality = GraphicsLevel::High;
        assert!(atmospheric_haze_for(settings.time_of_day, settings.atmosphere_quality).is_some());
        assert!(!settings.matches_selected_preset());
    }

    #[test]
    fn the_sun_shines_downwards_from_the_configured_bearing() {
        let settings = LightingSettings::default();
        let direction = settings.sun_direction();
        assert!(direction.y < 0.0, "sunlight must travel downwards");
        assert!((direction.length() - 1.0).abs() < 1.0e-5);
        // A 42 degree elevation means the vertical component is sin(42).
        assert!((direction.y + 42.0_f32.to_radians().sin()).abs() < 1.0e-4);
    }

    #[test]
    fn shadow_maps_grow_with_quality() {
        let mut low_settings = LightingSettings::default();
        low_settings.apply_preset(LightingQuality::Performance);
        let low = low_settings.shadow_map_size();
        let mut high_settings = LightingSettings::default();
        high_settings.apply_preset(LightingQuality::Cinematic);
        let high = high_settings.shadow_map_size();
        assert!(high > low, "{high} !> {low}");
    }

    #[test]
    fn upper_presets_compensate_for_extra_occlusion_without_crushing_shadows() {
        let mut performance = LightingSettings::default();
        performance.apply_preset(LightingQuality::Performance);
        let mut balanced = performance.clone();
        balanced.apply_preset(LightingQuality::Balanced);
        let mut cinematic = performance.clone();
        cinematic.apply_preset(LightingQuality::Cinematic);

        assert_eq!(performance.quality_exposure_offset(), 0.0);
        assert!((balanced.quality_exposure_offset() + 0.5).abs() < f32::EPSILON);
        assert!((cinematic.quality_exposure_offset() + 0.7).abs() < f32::EPSILON);
        assert!(balanced.camera_ev100() < performance.camera_ev100());
        assert!(cinematic.camera_ev100() < balanced.camera_ev100());
        assert_eq!(performance.tonemapping(), Tonemapping::TonyMcMapface);
        assert_eq!(balanced.tonemapping(), Tonemapping::TonyMcMapface);
        assert_eq!(cinematic.tonemapping(), Tonemapping::TonyMcMapface);

        // Compensation follows the actual custom settings, not just the name
        // of the preset selected at the top of the menu.
        cinematic.ambient_occlusion = GraphicsLevel::Off;
        cinematic.contact_shadows = false;
        cinematic.reflection_quality = GraphicsLevel::Off;
        assert_eq!(cinematic.quality_exposure_offset(), 0.0);
    }

    #[test]
    fn time_of_day_cycles_and_controls_headlights() {
        let mut time = TimeOfDay::Day;
        assert!(!time.headlights_enabled());
        time = time.next();
        assert_eq!(time, TimeOfDay::Night);
        assert!(time.headlights_enabled());
        time = time.next();
        assert_eq!(time, TimeOfDay::Day);
        assert_eq!(TimeOfDay::Day.headlight_intensity_multiplier(), 0.0);
        assert_eq!(TimeOfDay::Night.headlight_intensity_multiplier(), 5.0);
        assert!(TimeOfDay::Day.skybox_brightness() > TimeOfDay::Night.skybox_brightness());
        assert_eq!(SKYBOX_PATHS.len(), TimeOfDay::ALL.len());
        assert!(SKYBOX_PATHS.iter().all(|path| path.ends_with(".png")));
        assert!(
            LightingSettings::default().sun_illuminance() > {
                let night = LightingSettings {
                    time_of_day: TimeOfDay::Night,
                    ..default()
                };
                night.sun_illuminance()
            }
        );
    }

    #[test]
    fn camera_adapts_to_darker_times_of_day() {
        assert_eq!(TimeOfDay::Day.exposure_offset(), 0.0);
        assert!(TimeOfDay::Night.exposure_offset() < TimeOfDay::Day.exposure_offset());
    }

    #[test]
    fn old_sunset_setting_migrates_to_night() {
        assert_eq!(
            ron::from_str::<TimeOfDay>("Sunset").unwrap(),
            TimeOfDay::Night
        );
    }
}
