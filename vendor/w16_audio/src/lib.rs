//! # w16_audio — W16 hypercar sound pack integration for Bevy 0.19
//!
//! Drives the pack's seamless loops from a few per-frame car parameters and plays the
//! one-shots (shifts, blow-off, pops, impacts, crashes, UI) from messages.
//!
//! * Engine: 10 on-load + 10 off-load/idle RPM loops per view (exterior / interior), crossfaded
//!   with equal-power curves by RPM and throttle, pitch-matched with `set_speed`, plus a
//!   rev-limiter loop.
//! * Turbo whistle layer driven by an internal boost model, blow-off valve on lift-off,
//!   exhaust crackle / backfire after lift-off, DSG shift accents.
//! * Tyres: rolling noise by speed, gravel / dirt / grass / kerb, squeal by slip, locked-wheel skid.
//! * Wind by speed, gearbox whine, scraping against guardrail / concrete / cars / ground.
//! * Impacts & crashes from [`W16Impact`] / [`W16Crash`] messages.
//!
//! Uses only Bevy's built-in `bevy_audio` (rodio). Silent layers are paused, so only the
//! handful of loops that are audible actually mix.
//!
//! ```ignore
//! app.add_plugins(W16AudioPlugin::default());
//! commands.spawn((Transform::default(), CarAudio::player()));
//! // every frame:
//! car.rpm = 4200.0; car.throttle = 1.0; car.speed_kmh = 180.0; car.slip = 0.1;
//! ```

use bevy::audio::{
    AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, PlaybackSettings, SpatialAudioSink,
    Volume,
};
use bevy::ecs::message::{Message, MessageReader};
use bevy::prelude::*;
use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;

pub mod sounds;
pub use sounds::{SoundInfo, SOUNDS};

// ------------------------------------------------------------------------------------------------
// Public configuration & API types
// ------------------------------------------------------------------------------------------------

/// Which file type to use for the seamless loops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopFormat {
    /// 48 kHz / 16-bit WAV: sample-exact loops (requires the `wav` feature of `bevy`). Recommended.
    Wav,
    /// OGG Vorbis: works with Bevy's default features; loops decode sample-exact with rodio too,
    /// but lossy coding makes the seam very slightly less perfect than WAV.
    Ogg,
}

/// Global settings, inserted as a resource by the plugin.
#[derive(Resource, Clone, Debug)]
pub struct W16AudioConfig {
    /// Folder of the pack inside Bevy's `assets/` directory.
    pub base_path: String,
    pub loop_format: LoopFormat,
    /// Master volume for everything played by this plugin.
    pub master_volume: f32,
    /// Seconds for gain smoothing of loop layers (prevents zipper noise).
    pub smoothing: f32,
    /// Rev limiter RPM (the limiter loop fades in above `limiter_rpm - 60` at full throttle).
    pub limiter_rpm: f32,
}

impl Default for W16AudioConfig {
    fn default() -> Self {
        Self {
            base_path: "audio/w16".into(),
            loop_format: LoopFormat::Ogg,
            master_volume: 0.8,
            smoothing: 0.035,
            limiter_rpm: 6400.0,
        }
    }
}

/// Camera / listener position relative to the car.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CameraView {
    /// Chase / trackside camera.
    #[default]
    Exterior,
    /// Cockpit camera.
    Interior,
}

/// Ground the tyres are rolling on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Surface {
    #[default]
    Asphalt,
    Gravel,
    Dirt,
    Grass,
}

/// What the car body is grinding against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrapeMaterial {
    Guardrail,
    Concrete,
    Car,
    Underbody,
}

/// Which loop groups to create for a car (fewer groups = fewer audio sinks).
#[derive(Clone, Debug)]
pub struct LayerSet {
    pub interior: bool,
    pub turbo: bool,
    pub transmission: bool,
    pub wind: bool,
    pub tires: bool,
    pub surfaces: bool,
    pub scrapes: bool,
}

impl LayerSet {
    pub fn all() -> Self {
        Self {
            interior: true,
            turbo: true,
            transmission: true,
            wind: true,
            tires: true,
            surfaces: true,
            scrapes: true,
        }
    }
    /// Lightweight set for AI / opponent cars (exterior engine, tyres, turbo).
    pub fn opponent() -> Self {
        Self {
            interior: false,
            turbo: true,
            transmission: false,
            wind: false,
            tires: true,
            surfaces: false,
            scrapes: false,
        }
    }
}

/// Per-group volume trims.
#[derive(Clone, Debug)]
pub struct CarMix {
    pub engine: f32,
    pub turbo: f32,
    pub blowoff: f32,
    pub pops: f32,
    pub shifts: f32,
    pub tires: f32,
    pub skid: f32,
    pub wind: f32,
    pub transmission: f32,
    pub scrape: f32,
}

impl Default for CarMix {
    fn default() -> Self {
        Self {
            engine: 1.0,
            turbo: 0.55,
            blowoff: 0.7,
            pops: 0.6,
            shifts: 0.7,
            tires: 1.0,
            skid: 1.0,
            wind: 1.0,
            transmission: 0.6,
            scrape: 1.0,
        }
    }
}

/// Put this on a car entity and update the public fields every frame.
#[derive(Component, Clone, Debug)]
pub struct CarAudio {
    /// Engine speed (idle ~800, redline 6400).
    pub rpm: f32,
    /// Accelerator pedal 0..1.
    pub throttle: f32,
    /// Road speed in km/h.
    pub speed_kmh: f32,
    /// Tyre slip 0..1 (0.1 = light squeal, 0.5 = sliding, 0.9+ = heavy drift / wheelspin).
    pub slip: f32,
    /// Brake pedal 0..1.
    pub brake: f32,
    /// True while the wheels are locked (no ABS): plays the locked-wheel skid.
    pub wheels_locked: bool,
    pub surface: Surface,
    /// Driving over a kerb / rumble strip.
    pub on_kerb: bool,
    /// Body scraping against something, with intensity 0..1.
    pub scrape: Option<(ScrapeMaterial, f32)>,
    pub view: CameraView,
    /// Overall volume of this car.
    pub volume: f32,
    /// Engine running (use [`W16EngineStart`] / [`W16EngineStop`] for the start/stop sequences).
    pub engine_running: bool,
    /// Physical ECU state; do not infer fuel cut merely from the sample's top RPM.
    pub limiter_active: bool,
    /// Use Bevy spatial audio (needs a `SpatialListener` and a `GlobalTransform` on the car).
    pub spatial: bool,
    /// Probability (0..1) of crackles after lifting off the throttle at high RPM.
    pub crackle_chance: f32,
    pub layers: LayerSet,
    pub mix: CarMix,
    state: CarState,
}

impl Default for CarAudio {
    fn default() -> Self {
        Self {
            rpm: 800.0,
            throttle: 0.0,
            speed_kmh: 0.0,
            slip: 0.0,
            brake: 0.0,
            wheels_locked: false,
            surface: Surface::Asphalt,
            on_kerb: false,
            scrape: None,
            view: CameraView::Exterior,
            volume: 1.0,
            engine_running: true,
            limiter_active: false,
            spatial: false,
            crackle_chance: 0.8,
            layers: LayerSet::all(),
            mix: CarMix::default(),
            state: CarState::default(),
        }
    }
}

impl CarAudio {
    /// Player car: every layer, non-spatial.
    pub fn player() -> Self {
        Self::default()
    }
    /// Opponent car: exterior-only lightweight set, spatial audio.
    pub fn opponent() -> Self {
        Self {
            spatial: true,
            layers: LayerSet::opponent(),
            ..Self::default()
        }
    }
    /// Current turbo boost estimate 0..1 (useful for a HUD gauge).
    pub fn boost(&self) -> f32 {
        self.state.boost
    }
}

// ------------------------------------------------------------------------------------------------
// Messages
// ------------------------------------------------------------------------------------------------

/// Gear change on a car: plays the DSG accent (ignition-cut brap / rev-match crackle + clunk).
#[derive(Message, Clone, Debug)]
pub struct W16Shift {
    pub car: Entity,
    pub up: bool,
}

/// Start / stop sequences (the loops fade in/out around the one-shot automatically).
#[derive(Message, Clone, Debug)]
pub struct W16EngineStart {
    pub car: Entity,
}
#[derive(Message, Clone, Debug)]
pub struct W16EngineStop {
    pub car: Entity,
}

/// Material of an impact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImpactMaterial {
    Guardrail,
    Concrete,
    TireWall,
    Car,
    CarSideswipe,
    Cone,
    SignPost,
    SuspensionBump,
    Landing,
    BottomOut,
}

/// Collision: `intensity` 0..1 selects light / medium / heavy variants and extra glass / debris.
#[derive(Message, Clone, Debug)]
pub struct W16Impact {
    pub material: ImpactMaterial,
    pub intensity: f32,
    pub view: CameraView,
    /// World position for spatial playback (None = 2D).
    pub position: Option<Vec3>,
}

/// Pre-designed serious crash sequences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrashKind {
    HighspeedConcrete,
    HighspeedGuardrail,
    GuardrailScrapeLong,
    Rollover,
    HeadOnCar,
    /// Includes a 1.9 s tyre-screech lead-in before the impact.
    SpinIntoWall,
}

#[derive(Message, Clone, Debug)]
pub struct W16Crash {
    pub kind: CrashKind,
    pub view: CameraView,
    pub position: Option<Vec3>,
}

/// Play any sound of the pack by id (see `SOUNDS` / CATALOG.csv), e.g. `"race_countdown_beep"`.
#[derive(Message, Clone, Debug)]
pub struct W16Sfx {
    pub id: String,
    pub volume: f32,
    pub speed: f32,
    pub position: Option<Vec3>,
}

impl W16Sfx {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            volume: 1.0,
            speed: 1.0,
            position: None,
        }
    }
}

// ------------------------------------------------------------------------------------------------
// Plugin
// ------------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct W16AudioPlugin {
    pub config: W16AudioConfig,
}

impl Plugin for W16AudioPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.config.clone())
            .init_resource::<W16Library>()
            .add_message::<W16Shift>()
            .add_message::<W16EngineStart>()
            .add_message::<W16EngineStop>()
            .add_message::<W16Impact>()
            .add_message::<W16Crash>()
            .add_message::<W16Sfx>()
            .add_systems(Startup, load_library)
            .add_systems(
                Update,
                (
                    spawn_car_layers,
                    update_cars,
                    handle_messages,
                    cleanup_layers,
                )
                    .chain(),
            );
    }
}

/// All handles of the pack, keyed by sound id.
#[derive(Resource, Default)]
pub struct W16Library {
    handles: HashMap<&'static str, Handle<AudioSource>>,
}

impl W16Library {
    pub fn get(&self, id: &str) -> Option<Handle<AudioSource>> {
        self.handles.get(id).cloned()
    }
    pub fn len(&self) -> usize {
        self.handles.len()
    }
    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }
}

fn load_library(mut lib: ResMut<W16Library>, cfg: Res<W16AudioConfig>, assets: Res<AssetServer>) {
    for s in SOUNDS {
        let ext = if s.looped && cfg.loop_format == LoopFormat::Wav {
            "wav"
        } else {
            "ogg"
        };
        let h: Handle<AudioSource> = assets.load(format!("{}/{}.{}", cfg.base_path, s.path, ext));
        lib.handles.insert(s.id, h);
    }
}

// ------------------------------------------------------------------------------------------------
// Loop tables (must match the file names of the pack)
// ------------------------------------------------------------------------------------------------

/// On-load (full throttle) loop RPM points.
pub const ON_RPM: [f32; 10] = [
    800.0, 1100.0, 1500.0, 2000.0, 2600.0, 3200.0, 3900.0, 4600.0, 5400.0, 6400.0,
];
/// Off-load loop RPM points; index 0 is the idle loop.
pub const OFF_RPM: [f32; 10] = [
    800.0, 1100.0, 1500.0, 2000.0, 2600.0, 3200.0, 3900.0, 4600.0, 5400.0, 6400.0,
];
pub const WIND_KMH: [f32; 4] = [100.0, 200.0, 300.0, 400.0];
pub const ROLL_KMH: [f32; 5] = [40.0, 70.0, 120.0, 200.0, 320.0];
const SQUEAL_NAMES: [&str; 4] = ["light", "medium", "heavy", "drift"];
const SQUEAL_SLIP: [f32; 4] = [0.22, 0.5, 0.78, 1.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LayerKind {
    EngineOn(CameraView, usize),
    EngineOff(CameraView, usize),
    Limiter(CameraView),
    Turbo,
    Whine,
    Wind(CameraView, usize),
    Roll(CameraView, usize),
    Ground(CameraView, Surface),
    Kerb(CameraView),
    Squeal(CameraView, usize),
    Lock(CameraView),
    Scrape(CameraView, ScrapeMaterial),
}

fn v(view: CameraView) -> &'static str {
    match view {
        CameraView::Exterior => "ext",
        CameraView::Interior => "int",
    }
}

fn layer_id(kind: LayerKind) -> String {
    match kind {
        LayerKind::EngineOn(w, i) => format!("w16_{}_on_{:04}", v(w), ON_RPM[i] as u32),
        LayerKind::EngineOff(w, 0) => format!("w16_{}_idle", v(w)),
        LayerKind::EngineOff(w, i) => format!("w16_{}_off_{:04}", v(w), OFF_RPM[i] as u32),
        LayerKind::Limiter(w) => format!("w16_{}_limiter", v(w)),
        LayerKind::Turbo => "turbo_whistle_loop".into(),
        LayerKind::Whine => "gear_whine_loop".into(),
        LayerKind::Wind(w, i) => format!("wind_{}_{:03}kmh", v(w), WIND_KMH[i] as u32),
        LayerKind::Roll(w, i) => format!("tire_roll_asphalt_{}_{:03}kmh", v(w), ROLL_KMH[i] as u32),
        LayerKind::Ground(w, s) => {
            let n = match s {
                Surface::Gravel => "gravel",
                Surface::Dirt => "dirt",
                _ => "grass",
            };
            format!("tire_roll_{}_{}_loop", n, v(w))
        }
        LayerKind::Kerb(w) => format!("kerb_rumble_strip_{}_loop", v(w)),
        LayerKind::Squeal(CameraView::Exterior, i) => {
            format!("tire_squeal_{}_ext_loop", SQUEAL_NAMES[i])
        }
        // the cockpit set has medium & heavy only
        LayerKind::Squeal(CameraView::Interior, i) => {
            format!(
                "tire_squeal_{}_int_loop",
                if i <= 1 { "medium" } else { "heavy" }
            )
        }
        LayerKind::Lock(w) => format!("tire_skid_lock_{}_loop", v(w)),
        LayerKind::Scrape(w, m) => {
            let (n, has_int) = match m {
                ScrapeMaterial::Guardrail => ("guardrail", true),
                ScrapeMaterial::Concrete => ("concrete", true),
                ScrapeMaterial::Car => ("car", false),
                ScrapeMaterial::Underbody => ("underbody", false),
            };
            let vv = if w == CameraView::Interior && has_int {
                "int"
            } else {
                "ext"
            };
            format!("scrape_{}_{}_loop", n, vv)
        }
    }
}

fn car_layer_kinds(set: &LayerSet) -> Vec<LayerKind> {
    let mut out = Vec::new();
    let mut views = vec![CameraView::Exterior];
    if set.interior {
        views.push(CameraView::Interior);
    }
    for &w in &views {
        for i in 0..ON_RPM.len() {
            out.push(LayerKind::EngineOn(w, i));
        }
        for i in 0..OFF_RPM.len() {
            out.push(LayerKind::EngineOff(w, i));
        }
        out.push(LayerKind::Limiter(w));
        if set.wind {
            for i in 0..WIND_KMH.len() {
                out.push(LayerKind::Wind(w, i));
            }
        }
        if set.tires {
            for i in 0..ROLL_KMH.len() {
                out.push(LayerKind::Roll(w, i));
            }
            let squeals: &[usize] = if w == CameraView::Exterior {
                &[0, 1, 2, 3]
            } else {
                &[1, 2]
            };
            for &i in squeals {
                out.push(LayerKind::Squeal(w, i));
            }
            out.push(LayerKind::Lock(w));
        }
        if set.surfaces {
            for s in [Surface::Gravel, Surface::Dirt, Surface::Grass] {
                out.push(LayerKind::Ground(w, s));
            }
            out.push(LayerKind::Kerb(w));
        }
        if set.scrapes {
            for m in [
                ScrapeMaterial::Guardrail,
                ScrapeMaterial::Concrete,
                ScrapeMaterial::Car,
                ScrapeMaterial::Underbody,
            ] {
                out.push(LayerKind::Scrape(w, m));
            }
        }
    }
    if set.turbo {
        out.push(LayerKind::Turbo);
    }
    if set.transmission {
        out.push(LayerKind::Whine);
    }
    out
}

// ------------------------------------------------------------------------------------------------
// Internal state
// ------------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct CarState {
    spawned: bool,
    thr: f32,
    prev_thr: f32,
    view_ext: f32,
    boost: f32,
    lim: f32,
    run: f32,
    run_target: f32,
    start_timer: Option<f32>,
    bov_cooldown: f32,
    peak_boost: f32,
    pops: Vec<f32>,
    skid: f32,
    lock: f32,
    rng: u64,
}

impl Default for CarState {
    fn default() -> Self {
        Self {
            spawned: false,
            thr: 0.0,
            prev_thr: 0.0,
            view_ext: 1.0,
            boost: 0.0,
            lim: 0.0,
            run: 1.0,
            run_target: 1.0,
            start_timer: None,
            bov_cooldown: 0.0,
            peak_boost: 0.0,
            pops: Vec::new(),
            skid: 0.0,
            lock: 0.0,
            rng: 0x9E37_79B9_7F4A_7C15,
        }
    }
}

impl CarState {
    fn rand(&mut self) -> f32 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        ((self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / ((1u64 << 24) as f32)
    }
}

#[derive(Component)]
struct W16Layer {
    car: Entity,
    kind: LayerKind,
    gain: f32,
    paused: bool,
}

fn rnd(seed: &mut u64) -> f32 {
    *seed ^= *seed >> 12;
    *seed ^= *seed << 25;
    *seed ^= *seed >> 27;
    ((seed.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / ((1u64 << 24) as f32)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Equal-power crossfade weights of `x` over sorted `points` -> (index_a, w_a, index_b, w_b).
fn blend(points: &[f32], x: f32) -> (usize, f32, usize, f32) {
    let n = points.len();
    if x <= points[0] {
        return (0, 1.0, 0, 0.0);
    }
    if x >= points[n - 1] {
        return (n - 1, 1.0, n - 1, 0.0);
    }
    let mut i = 0;
    while i + 1 < n && points[i + 1] <= x {
        i += 1;
    }
    let f = (x - points[i]) / (points[i + 1] - points[i]);
    (i, (f * FRAC_PI_2).cos(), i + 1, (f * FRAC_PI_2).sin())
}

fn weight_of(points: &[f32], x: f32, idx: usize) -> f32 {
    let (a, wa, b, wb) = blend(points, x);
    let mut w = 0.0;
    if a == idx {
        w += wa;
    }
    if b == idx && b != a {
        w += wb;
    }
    w
}

fn approach(cur: f32, target: f32, dt: f32, tau: f32) -> f32 {
    if tau <= 0.0 {
        return target;
    }
    cur + (target - cur) * (1.0 - (-dt / tau).exp())
}

// ------------------------------------------------------------------------------------------------
// Systems
// ------------------------------------------------------------------------------------------------

fn spawn_car_layers(
    mut commands: Commands,
    mut cars: Query<(Entity, &mut CarAudio)>,
    lib: Res<W16Library>,
) {
    if lib.is_empty() {
        return;
    }
    for (car, mut ca) in &mut cars {
        if ca.state.spawned {
            continue;
        }
        ca.state.spawned = true;
        ca.state.rng ^= car.to_bits().wrapping_mul(0x9E37_79B9);
        ca.state.view_ext = if ca.view == CameraView::Exterior {
            1.0
        } else {
            0.0
        };
        ca.state.run = if ca.engine_running { 1.0 } else { 0.0 };
        ca.state.run_target = ca.state.run;
        for kind in car_layer_kinds(&ca.layers) {
            let id = layer_id(kind);
            let Some(h) = lib.get(&id) else {
                warn!("w16_audio: missing loop '{id}'");
                continue;
            };
            // Starting 80+ silent decoders at once causes a startup CPU spike.
            // Keep the initial sink state in sync with the component state.
            let settings = PlaybackSettings::LOOP
                .paused()
                .with_volume(Volume::SILENT)
                .with_spatial(ca.spatial);
            let mut e = commands.spawn((
                AudioPlayer::new(h),
                settings,
                W16Layer {
                    car,
                    kind,
                    gain: 0.0,
                    paused: true,
                },
            ));
            if ca.spatial {
                e.insert(Transform::default());
            }
        }
    }
}

struct Snapshot {
    rpm: f32,
    thr: f32,
    view_ext: f32,
    boost: f32,
    lim: f32,
    run: f32,
    speed: f32,
    skid: f32,
    slip: f32,
    lock: f32,
    surface: Surface,
    on_kerb: bool,
    scrape: Option<(ScrapeMaterial, f32)>,
    volume: f32,
    mix: CarMix,
    pos: Option<Vec3>,
}

type LayerQueryItem<'a> = (
    &'a mut W16Layer,
    Option<&'a mut AudioSink>,
    Option<&'a mut SpatialAudioSink>,
    Option<&'a mut Transform>,
);

#[allow(clippy::too_many_arguments)]
fn update_cars(
    mut commands: Commands,
    time: Res<Time>,
    cfg: Res<W16AudioConfig>,
    lib: Res<W16Library>,
    mut cars: Query<(Entity, &mut CarAudio, Option<&GlobalTransform>)>,
    mut layers: Query<LayerQueryItem>,
) {
    let dt = time.delta_secs().min(0.1);
    let mut snaps: HashMap<Entity, Snapshot> = HashMap::new();
    for (car, ca, gt) in &mut cars {
        let ca = ca.into_inner();
        let rpm = ca.rpm.max(0.0);
        let thr_in = ca.throttle.clamp(0.0, 1.0);
        let view_t = if ca.view == CameraView::Exterior {
            1.0
        } else {
            0.0
        };
        let limiter = cfg.limiter_rpm;
        let (slip_in, speed, locked, brake, crackle) = (
            ca.slip.clamp(0.0, 1.0),
            ca.speed_kmh.max(0.0),
            ca.wheels_locked,
            ca.brake,
            ca.crackle_chance,
        );
        let st = &mut ca.state;
        st.prev_thr = st.thr;
        let tau = if thr_in > st.thr { 0.06 } else { 0.09 };
        st.thr = approach(st.thr, thr_in, dt, tau);
        st.view_ext = approach(st.view_ext, view_t, dt, 0.12);
        // boost model (quad turbo, sequential second pair)
        let b_target = thr_in.powf(1.2) * smoothstep(1600.0, 3300.0, rpm);
        let tb = if b_target > st.boost { 0.45 } else { 0.12 };
        st.boost = approach(st.boost, b_target, dt, tb);
        st.peak_boost = (st.peak_boost - dt * 0.8).max(st.boost);
        let lim_t = if ca.limiter_active && thr_in > 0.85 && rpm >= limiter - 60.0 {
            1.0
        } else {
            0.0
        };
        st.lim = approach(st.lim, lim_t, dt, 0.06);
        // start / stop fades
        if st.start_timer.is_none() {
            st.run_target = if ca.engine_running { 1.0 } else { 0.0 };
        }
        if let Some(t) = st.start_timer.as_mut() {
            *t += dt;
            if *t >= 6.6 {
                st.run_target = 1.0;
                st.start_timer = None;
            }
        }
        let run_tau = if st.run_target > st.run { 0.5 } else { 0.18 };
        st.run = approach(st.run, st.run_target, dt, run_tau);
        // skid / lock envelopes
        st.skid = approach(
            st.skid,
            smoothstep(0.06, 0.28, slip_in) * (speed / 12.0 + slip_in * 0.6).min(1.0),
            dt,
            0.05,
        );
        let lock_t = if locked && speed > 4.0 {
            (brake.max(0.6) * (speed / 40.0).min(1.0)).min(1.0)
        } else {
            0.0
        };
        st.lock = approach(st.lock, lock_t, dt, 0.04);
        // lift-off events: blow-off valve + crackles
        st.bov_cooldown -= dt;
        let lifted = st.prev_thr >= 0.3 && st.thr < 0.3;
        let view_str = if view_t > 0.5 { "ext" } else { "int" };
        let pos = if ca.spatial {
            gt.map(|g| g.translation())
        } else {
            None
        };
        let vol = ca.volume * cfg.master_volume;
        if lifted && st.run > 0.5 {
            if st.peak_boost > 0.3 && st.bov_cooldown <= 0.0 {
                let k = 1 + (st.rand() * 4.0) as usize % 4;
                let g = vol * ca.mix.blowoff * (0.4 + 0.6 * st.peak_boost);
                let sp = 0.96 + 0.08 * st.rand();
                play(
                    &mut commands,
                    &lib,
                    &format!("turbo_blowoff_{k:02}"),
                    g,
                    sp,
                    pos,
                );
                st.bov_cooldown = 0.6;
            }
            if rpm > 2800.0 && st.rand() < crackle {
                let n = 2 + (st.rand() * 5.0) as usize;
                for _ in 0..n {
                    let t = 0.12 + st.rand() * 1.3;
                    st.pops.push(t);
                }
            }
        }
        // scheduled crackles
        let mut due = 0;
        st.pops.retain_mut(|t| {
            *t -= dt;
            if *t <= 0.0 {
                due += 1;
                false
            } else {
                true
            }
        });
        for _ in 0..due {
            if st.thr > 0.35 {
                continue; // back on throttle: no crackle
            }
            let id = if view_str == "ext" {
                if st.rand() < 0.04 {
                    format!("w16_ext_backfire_{:02}", 1 + (st.rand() * 3.0) as usize % 3)
                } else {
                    format!("w16_ext_pop_{:02}", 1 + (st.rand() * 8.0) as usize % 8)
                }
            } else {
                format!("w16_int_pops_{:02}", 1 + (st.rand() * 3.0) as usize % 3)
            };
            let g = vol * ca.mix.pops * (0.45 + 0.55 * st.rand()) * (rpm / 6400.0).clamp(0.4, 1.0);
            let sp = 0.94 + 0.12 * st.rand();
            play(&mut commands, &lib, &id, g, sp, pos);
        }
        let s = &ca.state;
        snaps.insert(
            car,
            Snapshot {
                rpm,
                thr: s.thr,
                view_ext: s.view_ext,
                boost: s.boost,
                lim: s.lim,
                run: s.run,
                speed,
                skid: s.skid,
                slip: slip_in,
                lock: s.lock,
                surface: ca.surface,
                on_kerb: ca.on_kerb,
                scrape: ca.scrape,
                volume: vol,
                mix: ca.mix.clone(),
                pos,
            },
        );
    }

    let tau = cfg.smoothing;
    for (mut layer, sink, spatial_sink, tf) in &mut layers {
        let Some(s) = snaps.get(&layer.car) else {
            continue;
        };
        let (target, speed) = layer_target(layer.kind, s);
        layer.gain = approach(layer.gain, target, dt, tau);
        if let (Some(mut tf), Some(p)) = (tf, s.pos) {
            tf.translation = p;
        }
        let g = layer.gain;
        let silent = g < 1e-4 && target < 1e-4;
        if let Some(mut sink) = sink {
            apply(&mut *sink, &mut layer.paused, g, speed, silent);
        } else if let Some(mut sink) = spatial_sink {
            apply(&mut *sink, &mut layer.paused, g, speed, silent);
        }
    }
}

fn apply<S: AudioSinkPlayback>(
    sink: &mut S,
    paused: &mut bool,
    gain: f32,
    speed: f32,
    silent: bool,
) {
    if silent {
        if !*paused {
            sink.set_volume(Volume::SILENT);
            sink.pause();
            *paused = true;
        }
        return;
    }
    if *paused {
        sink.play();
        *paused = false;
    }
    sink.set_volume(Volume::Linear(gain));
    sink.set_speed(speed.clamp(0.25, 4.0));
}

/// Target gain and playback speed for one loop layer.
fn layer_target(kind: LayerKind, s: &Snapshot) -> (f32, f32) {
    let vw = |w: CameraView| {
        if w == CameraView::Exterior {
            s.view_ext.sqrt()
        } else {
            (1.0 - s.view_ext).max(0.0).sqrt()
        }
    };
    let eng = s.volume * s.mix.engine * s.run;
    let w_on = s.thr.sqrt();
    let w_off = (1.0 - s.thr).max(0.0).sqrt();
    match kind {
        LayerKind::EngineOn(w, i) => {
            let g = weight_of(&ON_RPM, s.rpm, i) * w_on * (1.0 - s.lim).sqrt() * vw(w) * eng;
            (g, s.rpm.max(200.0) / ON_RPM[i])
        }
        LayerKind::EngineOff(w, i) => {
            let g = weight_of(&OFF_RPM, s.rpm, i) * w_off * vw(w) * eng;
            (g, s.rpm.max(200.0) / OFF_RPM[i])
        }
        LayerKind::Limiter(w) => (w_on * s.lim.sqrt() * vw(w) * eng, s.rpm / 6325.0),
        LayerKind::Turbo => {
            let g = s.boost.powf(2.3)
                * (0.45 + 0.55 * (1.0 - s.view_ext))
                * s.mix.turbo
                * s.volume
                * s.run;
            (g, (1700.0 + 6400.0 * s.boost.powf(0.9)) / 5000.0)
        }
        LayerKind::Whine => {
            let g = (s.speed / 150.0).clamp(0.0, 1.6).powf(0.8)
                * (0.25 + 0.75 * (1.0 - s.view_ext))
                * s.mix.transmission
                * s.volume;
            (g, (s.speed / 150.0).clamp(0.2, 3.0))
        }
        LayerKind::Wind(w, i) => {
            let low = (s.speed / 100.0).clamp(0.0, 1.0).powi(2);
            (
                weight_of(&WIND_KMH, s.speed, i) * low * vw(w) * s.mix.wind * s.volume,
                (s.speed.max(1.0) / WIND_KMH[i])
                    .powf(0.25)
                    .clamp(0.85, 1.15),
            )
        }
        LayerKind::Roll(w, i) => {
            let low = (s.speed / ROLL_KMH[0]).clamp(0.0, 1.0).powf(1.5);
            let on_asphalt = if s.surface == Surface::Asphalt {
                1.0
            } else {
                0.25
            };
            (
                weight_of(&ROLL_KMH, s.speed, i)
                    * low
                    * on_asphalt
                    * vw(w)
                    * s.mix.tires
                    * s.volume,
                (s.speed.max(1.0) / ROLL_KMH[i]).clamp(0.75, 1.3),
            )
        }
        LayerKind::Ground(w, surf) => {
            let on = if s.surface == surf { 1.0 } else { 0.0 };
            (
                on * (s.speed / 60.0).clamp(0.0, 1.3).powf(0.8) * vw(w) * s.mix.tires * s.volume,
                (s.speed.max(1.0) / 60.0).clamp(0.6, 1.6),
            )
        }
        LayerKind::Kerb(w) => {
            let on = if s.on_kerb { 1.0 } else { 0.0 };
            (
                on * (s.speed / 80.0).clamp(0.0, 1.0) * vw(w) * s.mix.tires * s.volume,
                (s.speed.max(1.0) / 100.0).clamp(0.4, 2.5),
            )
        }
        LayerKind::Squeal(w, i) => {
            // crossfade light -> medium -> heavy -> drift by slip; the cockpit set has medium/heavy only
            let wsl = if w == CameraView::Exterior {
                weight_of(&SQUEAL_SLIP, s.slip, i)
            } else if i == 1 {
                weight_of(&[0.35, 0.8], s.slip, 0)
            } else {
                weight_of(&[0.35, 0.8], s.slip, 1)
            };
            let g = wsl * s.skid * (1.0 - 0.7 * s.lock) * vw(w) * s.mix.skid * s.volume;
            (g, 0.92 + 0.16 * (s.speed / 200.0).clamp(0.0, 1.0))
        }
        LayerKind::Lock(w) => (
            s.lock * vw(w) * s.mix.skid * s.volume,
            0.85 + 0.2 * (s.speed / 150.0).clamp(0.0, 1.0),
        ),
        LayerKind::Scrape(w, m) => {
            let g = match s.scrape {
                Some((mm, k)) if mm == m => {
                    k.clamp(0.0, 1.0) * (s.speed / 40.0 + 0.2).clamp(0.0, 1.0)
                }
                _ => 0.0,
            };
            (
                g * vw(w) * s.mix.scrape * s.volume,
                0.85 + 0.3 * (s.speed / 200.0).clamp(0.0, 1.0),
            )
        }
    }
}

fn play(
    commands: &mut Commands,
    lib: &W16Library,
    id: &str,
    volume: f32,
    speed: f32,
    pos: Option<Vec3>,
) {
    let Some(h) = lib.get(id) else {
        warn!("w16_audio: unknown sound '{id}'");
        return;
    };
    let settings = PlaybackSettings::DESPAWN
        .with_volume(Volume::Linear(volume.max(0.0)))
        .with_speed(speed)
        .with_spatial(pos.is_some());
    match pos {
        Some(p) => {
            commands.spawn((
                AudioPlayer::new(h),
                settings,
                Transform::from_translation(p),
            ));
        }
        None => {
            commands.spawn((AudioPlayer::new(h), settings));
        }
    }
}

fn pick(seed: &mut u64, n: usize) -> usize {
    1 + ((rnd(seed) * n as f32) as usize).min(n - 1)
}

#[allow(clippy::too_many_arguments)]
fn handle_messages(
    mut commands: Commands,
    lib: Res<W16Library>,
    cfg: Res<W16AudioConfig>,
    mut cars: Query<&mut CarAudio>,
    mut shifts: MessageReader<W16Shift>,
    mut starts: MessageReader<W16EngineStart>,
    mut stops: MessageReader<W16EngineStop>,
    mut impacts: MessageReader<W16Impact>,
    mut crashes: MessageReader<W16Crash>,
    mut sfx: MessageReader<W16Sfx>,
    mut rng: Local<u64>,
) {
    if *rng == 0 {
        *rng = 0x1234_5678_9ABC_DEF1;
    }
    let rng: &mut u64 = &mut rng;
    let m = cfg.master_volume;
    for e in shifts.read() {
        let Ok(mut ca) = cars.get_mut(e.car) else {
            continue;
        };
        let ext = ca.view == CameraView::Exterior;
        let vol = ca.volume * m * ca.mix.shifts;
        let id = match (e.up, ext) {
            (true, true) => format!("w16_ext_upshift_{:02}", pick(rng, 4)),
            (true, false) => format!("w16_int_upshift_{:02}", pick(rng, 2)),
            (false, true) => format!("w16_ext_downshift_{:02}", pick(rng, 3)),
            (false, false) => format!("w16_int_downshift_{:02}", pick(rng, 2)),
        };
        let sp = 0.96 + 0.08 * rnd(rng);
        play(
            &mut commands,
            &lib,
            &id,
            vol * (0.6 + 0.4 * ca.throttle.max(0.3)),
            sp,
            None,
        );
        if !ext {
            play(
                &mut commands,
                &lib,
                &format!("dsg_clunk_{:02}", pick(rng, 3)),
                vol * 0.6,
                1.0,
                None,
            );
        }
        // a quick ignition-cut dip of the loops
        ca.state.thr *= 0.55;
    }
    for e in starts.read() {
        let Ok(mut ca) = cars.get_mut(e.car) else {
            continue;
        };
        let id = if ca.view == CameraView::Exterior {
            "w16_ext_startup"
        } else {
            "w16_int_startup"
        };
        play(&mut commands, &lib, id, ca.volume * m, 1.0, None);
        ca.engine_running = true;
        ca.state.run = 0.0;
        ca.state.run_target = 0.0;
        ca.state.start_timer = Some(0.0);
    }
    for e in stops.read() {
        let Ok(mut ca) = cars.get_mut(e.car) else {
            continue;
        };
        let id = if ca.view == CameraView::Exterior {
            "w16_ext_shutdown"
        } else {
            "w16_int_shutdown"
        };
        play(&mut commands, &lib, id, ca.volume * m, 1.0, None);
        ca.engine_running = false;
        ca.state.run_target = 0.0;
        ca.state.start_timer = None;
    }
    for e in impacts.read() {
        let k = e.intensity.clamp(0.0, 1.0);
        let tier = if k < 0.35 {
            "light"
        } else if k < 0.7 {
            "medium"
        } else {
            "heavy"
        };
        let inside = e.view == CameraView::Interior;
        let (id, trim) = match e.material {
            ImpactMaterial::Guardrail | ImpactMaterial::Concrete => {
                let mat = if e.material == ImpactMaterial::Guardrail {
                    "guardrail"
                } else {
                    "concrete"
                };
                if inside && tier == "heavy" {
                    (format!("impact_{mat}_heavy_int"), 1.0)
                } else {
                    let n = match (mat, tier) {
                        ("guardrail", _) => 3,
                        (_, "heavy") => 3,
                        _ => 2,
                    };
                    (
                        format!("impact_{mat}_{tier}_{:02}", pick(rng, n)),
                        if inside { 0.7 } else { 1.0 },
                    )
                }
            }
            ImpactMaterial::TireWall => (
                format!("impact_tirewall_{:02}", 1 + ((k * 2.99) as usize)),
                1.0,
            ),
            ImpactMaterial::Car => {
                if k < 0.6 {
                    (format!("impact_car_bump_{:02}", pick(rng, 3)), 1.0)
                } else {
                    (format!("impact_car_heavy_{:02}", pick(rng, 2)), 1.0)
                }
            }
            ImpactMaterial::CarSideswipe => {
                (format!("impact_car_sideswipe_{:02}", pick(rng, 2)), 1.0)
            }
            ImpactMaterial::Cone => (format!("impact_cone_{:02}", pick(rng, 2)), 1.0),
            ImpactMaterial::SignPost => ("impact_sign_post_01".to_string(), 1.0),
            ImpactMaterial::SuspensionBump => {
                if k < 0.5 {
                    (format!("suspension_bump_small_{:02}", pick(rng, 3)), 1.0)
                } else {
                    (format!("suspension_bump_big_{:02}", pick(rng, 2)), 1.0)
                }
            }
            ImpactMaterial::Landing => (format!("suspension_landing_{:02}", pick(rng, 2)), 1.0),
            ImpactMaterial::BottomOut => {
                (format!("suspension_bottom_out_{:02}", pick(rng, 2)), 1.0)
            }
        };
        let within = match tier {
            "light" => k / 0.35,
            "medium" => (k - 0.35) / 0.35,
            _ => (k - 0.7) / 0.3,
        };
        let vol = m * trim * (0.7 + 0.3 * within.clamp(0.0, 1.0));
        let sp = 0.95 + 0.1 * rnd(rng);
        play(&mut commands, &lib, &id, vol, sp, e.position);
        let hard = matches!(
            e.material,
            ImpactMaterial::Guardrail | ImpactMaterial::Concrete | ImpactMaterial::Car
        );
        if hard && k > 0.9 && rnd(rng) < 0.6 {
            play(
                &mut commands,
                &lib,
                &format!("glass_shatter_{:02}", pick(rng, 3)),
                m * 0.6,
                1.0,
                e.position,
            );
            play(
                &mut commands,
                &lib,
                &format!("debris_metal_{:02}", pick(rng, 3)),
                m * 0.6,
                1.0,
                e.position,
            );
        }
    }
    for e in crashes.read() {
        let id = match (e.kind, e.view) {
            (CrashKind::HighspeedConcrete, CameraView::Interior) => "crash_highspeed_concrete_int",
            (CrashKind::HighspeedConcrete, _) => "crash_highspeed_concrete_01",
            (CrashKind::HighspeedGuardrail, _) => "crash_highspeed_guardrail_01",
            (CrashKind::GuardrailScrapeLong, _) => "crash_guardrail_scrape_long_01",
            (CrashKind::Rollover, _) => "crash_rollover_01",
            (CrashKind::HeadOnCar, _) => "crash_headon_car_01",
            (CrashKind::SpinIntoWall, _) => "crash_spin_into_wall_01",
        };
        play(&mut commands, &lib, id, m, 1.0, e.position);
    }
    for e in sfx.read() {
        play(
            &mut commands,
            &lib,
            &e.id,
            m * e.volume,
            e.speed,
            e.position,
        );
    }
}

fn cleanup_layers(
    mut commands: Commands,
    layers: Query<(Entity, &W16Layer)>,
    cars: Query<(), With<CarAudio>>,
) {
    for (e, l) in &layers {
        if cars.get(l.car).is_err() {
            commands.entity(e).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_layer_and_oneshot_id_exists() {
        let ids: std::collections::HashSet<&str> = SOUNDS.iter().map(|s| s.id).collect();
        for kind in car_layer_kinds(&LayerSet::all()) {
            let id = layer_id(kind);
            assert!(ids.contains(id.as_str()), "missing loop {id}");
        }
        let fixed = [
            "w16_ext_startup",
            "w16_int_startup",
            "w16_ext_shutdown",
            "w16_int_shutdown",
            "impact_sign_post_01",
            "crash_highspeed_concrete_int",
            "crash_highspeed_concrete_01",
            "crash_highspeed_guardrail_01",
            "crash_guardrail_scrape_long_01",
            "crash_rollover_01",
            "crash_headon_car_01",
            "crash_spin_into_wall_01",
            "impact_guardrail_heavy_int",
            "impact_concrete_heavy_int",
        ];
        for id in fixed {
            assert!(ids.contains(id), "missing {id}");
        }
        let series: [(&str, usize); 26] = [
            ("turbo_blowoff_", 4),
            ("w16_ext_backfire_", 3),
            ("w16_ext_pop_", 8),
            ("w16_int_pops_", 3),
            ("w16_ext_upshift_", 4),
            ("w16_int_upshift_", 2),
            ("w16_ext_downshift_", 3),
            ("w16_int_downshift_", 2),
            ("dsg_clunk_", 3),
            ("impact_guardrail_light_", 3),
            ("impact_guardrail_medium_", 3),
            ("impact_guardrail_heavy_", 3),
            ("impact_concrete_light_", 2),
            ("impact_concrete_medium_", 2),
            ("impact_concrete_heavy_", 3),
            ("impact_tirewall_", 3),
            ("impact_car_bump_", 3),
            ("impact_car_heavy_", 2),
            ("impact_car_sideswipe_", 2),
            ("impact_cone_", 2),
            ("suspension_bump_small_", 3),
            ("suspension_bump_big_", 2),
            ("suspension_landing_", 2),
            ("suspension_bottom_out_", 2),
            ("glass_shatter_", 3),
            ("debris_metal_", 3),
        ];
        for (prefix, n) in series {
            for i in 1..=n {
                let id = format!("{prefix}{i:02}");
                assert!(ids.contains(id.as_str()), "missing {id}");
            }
        }
    }

    #[test]
    fn blend_is_equal_power() {
        for rpm in [800.0, 950.0, 1234.0, 3000.0, 6100.0, 6400.0, 7000.0] {
            let p: f32 = (0..ON_RPM.len())
                .map(|i| weight_of(&ON_RPM, rpm, i).powi(2))
                .sum();
            assert!((p - 1.0).abs() < 1e-4, "rpm {rpm}: power {p}");
        }
    }
}
