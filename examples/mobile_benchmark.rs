//! Frame-time benchmark of the phone build.
//!
//! ```sh
//! BENCH_SIZE=2400x1080 BENCH_SECONDS=20 \
//!     cargo run --release --features mobile --example mobile_benchmark -- out_dir
//! ```
//!
//! Starts a race through the phone menus with synthetic touches (like
//! `store_screenshots`), holds GAS, waits `BENCH_WARMUP` seconds and then
//! records the real duration of every frame for `BENCH_SECONDS`. The summary
//! (average, median, 90th/99th percentile, FPS) and what the frame contained
//! (entities, visible meshes, triangles, lights) is printed to the log and
//! written to `out_dir/benchmark.txt`. The graphics settings are the phone
//! build's first-launch defaults, so the run measures what a new player gets.
//!
//! Optional environment:
//! - `BENCH_SIZE`: window size in physical pixels, default `2400x1080`.
//! - `BENCH_NIGHT=1`: drive at night (headlights and street lights on).
//! - `BENCH_TRAFFIC=off|low|medium|high`: traffic density override.
//! - `BENCH_SHOT=1`: also save a screenshot at the end of the measurement.
//! - `BENCH_PRESET=<name>`: graphics preset instead of the first-launch
//!   default, by its menu label (`lite`, `performance`, `balanced`, ...).
//! - `BENCH_RESOLUTION=480p|540p|720p|native`: 3D render resolution, applied
//!   after the preset.
//! - `BENCH_WARMUP`: seconds of driving before the measurement, default 8.
//!
//! Works with a software Vulkan driver (Mesa lavapipe under Xvfb). Absolute
//! numbers then say little about a phone GPU, but the ratio between two runs
//! at the same size tracks how much rendering work was removed.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Duration;

use bevy::camera::visibility::ViewVisibility;
use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::light::{DirectionalLight, PointLight, SpotLight};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::{PrimaryWindow, WindowResolution};

use veyron_sim::mobile::{SteeringMode, TouchControlKind, TouchControlSettings};
use veyron_sim::world::lighting::{LightingQuality, LightingSettings, RenderResolution, TimeOfDay};
use veyron_sim::world::traffic::{TrafficDensity, TrafficSettings};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Step {
    Settle,
    TouchPlay,
    LiftPlay,
    TouchDrive,
    LiftDrive,
    WaitForControls,
    Warmup,
    Measure,
    Done,
}

#[derive(Resource)]
struct Bench {
    out: PathBuf,
    step: Step,
    resume_at: f64,
    held: Vec<(u64, Vec2)>,
    warmup: f64,
    seconds: f64,
    frames: Vec<f64>,
    measure_until: f64,
    night: bool,
    traffic: Option<TrafficDensity>,
    preset: Option<LightingQuality>,
    resolution: Option<RenderResolution>,
    shot: bool,
    started: f64,
}

fn env_f64(name: &str, default: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(default)
}

fn main() {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "mobile_benchmark".to_string()),
    );
    std::fs::create_dir_all(&out).expect("output directory");
    let _ = std::fs::remove_file(out.join("settings.ron"));
    // SAFETY: single-threaded at this point, before Bevy starts any thread.
    unsafe {
        // Do not touch the player's real settings file.
        std::env::set_var("VEYRON_SETTINGS_PATH", out.join("settings.ron"));
    }
    let size = std::env::var("BENCH_SIZE")
        .ok()
        .and_then(|value| {
            let (w, h) = value.trim().split_once('x')?;
            Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))
        })
        .unwrap_or((2400, 1080));
    let traffic = std::env::var("BENCH_TRAFFIC")
        .ok()
        .and_then(|value| match value.trim() {
            "off" => Some(TrafficDensity::Off),
            "low" => Some(TrafficDensity::Low),
            "medium" => Some(TrafficDensity::Medium),
            "high" => Some(TrafficDensity::High),
            _ => None,
        });

    let preset = std::env::var("BENCH_PRESET").ok().map(|value| {
        LightingQuality::ALL
            .into_iter()
            .find(|quality| quality.label().eq_ignore_ascii_case(value.trim()))
            .unwrap_or_else(|| panic!("unknown BENCH_PRESET {value}"))
    });

    let resolution = std::env::var("BENCH_RESOLUTION").ok().map(|value| {
        RenderResolution::ALL
            .into_iter()
            .find(|resolution| resolution.label().eq_ignore_ascii_case(value.trim()))
            .unwrap_or_else(|| panic!("unknown BENCH_RESOLUTION {value}"))
    });

    let mut app = veyron_sim::mobile::build_mobile_app();
    {
        let world = app.world_mut();
        let mut windows = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
        for mut window in windows.iter_mut(world) {
            window.resolution =
                WindowResolution::new(size.0, size.1).with_scale_factor_override(2.4);
        }
    }
    // Software rendering is slow: let game time keep pace with real time.
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .set_max_delta(Duration::from_secs(4));
    app.insert_resource(Bench {
        out,
        step: Step::Settle,
        resume_at: 6.0,
        held: Vec::new(),
        warmup: env_f64("BENCH_WARMUP", 8.0),
        seconds: env_f64("BENCH_SECONDS", 20.0),
        frames: Vec::new(),
        measure_until: 0.0,
        night: std::env::var("BENCH_NIGHT").is_ok_and(|value| value.trim() == "1"),
        traffic,
        preset,
        resolution,
        shot: std::env::var("BENCH_SHOT").is_ok_and(|value| value.trim() == "1"),
        started: 0.0,
    })
    .add_systems(Update, run_bench);
    app.run();
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn run_bench(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut bench: ResMut<Bench>,
    mut touch_settings: ResMut<TouchControlSettings>,
    mut lighting: ResMut<LightingSettings>,
    mut traffic: ResMut<TrafficSettings>,
    mut touches: MessageWriter<TouchInput>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    controls: Query<(&TouchControlKind, &UiGlobalTransform)>,
    texts: Query<(&Text, &UiGlobalTransform, &InheritedVisibility)>,
    (meshes, mesh_assets, entities, parents, names): (
        Query<(
            Entity,
            &Mesh3d,
            &ViewVisibility,
            &GlobalTransform,
            Option<&bevy::camera::primitives::Aabb>,
        )>,
        Res<Assets<Mesh>>,
        Query<Entity>,
        Query<&ChildOf>,
        Query<&Name>,
    ),
    (point_lights, spot_lights, suns, game_cameras): (
        Query<&ViewVisibility, With<PointLight>>,
        Query<&ViewVisibility, With<SpotLight>>,
        Query<&DirectionalLight>,
        Query<&GlobalTransform, With<veyron_sim::camera::controller::GameCamera>>,
    ),
    mut exit: MessageWriter<AppExit>,
) {
    let now = time.elapsed_secs_f64();
    let Ok((window, window_info)) = windows.single() else {
        return;
    };
    let scale = window_info.scale_factor();
    let held = bench.held.clone();
    let mut send = |phase, id, position| {
        touches.write(TouchInput {
            phase,
            position,
            window,
            force: None,
            id,
        });
    };
    for (id, position) in held {
        send(TouchPhase::Moved, id, position);
    }

    if bench.step == Step::Measure {
        bench.frames.push(time.delta_secs_f64());
        if now >= bench.measure_until {
            let report = summarise(
                &bench,
                window_info,
                &meshes,
                &mesh_assets,
                &parents,
                &names,
                entities.iter().count(),
                &point_lights,
                &spot_lights,
                &suns,
                game_cameras.iter().next().map(GlobalTransform::translation),
                &lighting,
            );
            info!("\n{report}");
            let _ = std::fs::write(bench.out.join("benchmark.txt"), &report);
            if bench.shot {
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(bench.out.join("benchmark.png")));
            }
            bench.step = Step::Done;
            bench.resume_at = now + if bench.shot { 8.0 } else { 0.5 };
        }
        return;
    }
    if now < bench.resume_at {
        return;
    }

    let find_text = |label: &str| {
        texts
            .iter()
            .find(|(text, _, visibility)| visibility.get() && text.as_str() == label)
            .map(|(_, transform, _)| transform.translation / scale)
    };

    match bench.step {
        Step::Settle => {
            if let Some(density) = bench.traffic {
                traffic.density = density;
            }
            if let Some(quality) = bench.preset {
                lighting.apply_preset(quality);
            }
            if let Some(resolution) = bench.resolution {
                lighting.render_resolution = resolution;
            }
            if bench.night {
                lighting.time_of_day = TimeOfDay::Night;
            }
            bench.step = Step::TouchPlay;
            // Let a changed preset or resolution settle before touching.
            bench.resume_at = now + 1.5;
        }
        Step::TouchPlay => {
            if let Some(position) = find_text("PLAY") {
                send(TouchPhase::Started, 10, position);
                bench.held.push((10, position));
                bench.step = Step::LiftPlay;
            }
            bench.resume_at = now + 0.3;
        }
        Step::LiftPlay => {
            lift(&mut bench, &mut send, 10);
            bench.step = Step::TouchDrive;
            bench.resume_at = now + 2.0;
        }
        Step::TouchDrive => {
            if let Some(position) = find_text("DRIVE WITHOUT DAMAGE") {
                send(TouchPhase::Started, 11, position);
                bench.held.push((11, position));
                bench.step = Step::LiftDrive;
            }
            bench.resume_at = now + 0.3;
        }
        Step::LiftDrive => {
            lift(&mut bench, &mut send, 11);
            touch_settings.steering = SteeringMode::Buttons;
            bench.step = Step::WaitForControls;
            bench.resume_at = now + 4.0;
        }
        Step::WaitForControls => {
            let gas = controls
                .iter()
                .find(|(kind, _)| **kind == TouchControlKind::Gas)
                .map(|(_, transform)| transform.translation / scale)
                .filter(|position| *position != Vec2::ZERO);
            if let Some(position) = gas {
                send(TouchPhase::Started, 1, position);
                bench.held.push((1, position));
                bench.step = Step::Warmup;
                bench.resume_at = now + bench.warmup;
            } else {
                bench.resume_at = now + 0.25;
            }
        }
        Step::Warmup => {
            bench.step = Step::Measure;
            bench.started = now;
            bench.measure_until = now + bench.seconds;
            bench.frames.clear();
        }
        Step::Measure => {}
        Step::Done => {
            exit.write(AppExit::Success);
        }
    }
}

fn lift(bench: &mut Bench, send: &mut impl FnMut(TouchPhase, u64, Vec2), id: u64) {
    if let Some(index) = bench.held.iter().position(|(held, _)| *held == id) {
        let (_, position) = bench.held.remove(index);
        send(TouchPhase::Ended, id, position);
    }
}

#[allow(clippy::too_many_arguments)]
fn summarise(
    bench: &Bench,
    window: &Window,
    meshes: &Query<(
        Entity,
        &Mesh3d,
        &ViewVisibility,
        &GlobalTransform,
        Option<&bevy::camera::primitives::Aabb>,
    )>,
    mesh_assets: &Assets<Mesh>,
    parents: &Query<&ChildOf>,
    names: &Query<&Name>,
    entity_count: usize,
    point_lights: &Query<&ViewVisibility, With<PointLight>>,
    spot_lights: &Query<&ViewVisibility, With<SpotLight>>,
    suns: &Query<&DirectionalLight>,
    camera: Option<Vec3>,
    lighting: &LightingSettings,
) -> String {
    let mut frames: Vec<f64> = bench
        .frames
        .iter()
        .map(|seconds| seconds * 1000.0)
        .collect();
    frames.sort_by(|a, b| a.total_cmp(b));
    let count = frames.len().max(1);
    let mean = frames.iter().sum::<f64>() / count as f64;
    let percentile = |p: f64| -> f64 {
        if frames.is_empty() {
            return 0.0;
        }
        let index = ((frames.len() - 1) as f64 * p).round() as usize;
        frames[index]
    };
    let (mut mesh_entities, mut visible_meshes, mut triangles) = (0usize, 0usize, 0usize);
    let mut by_root: std::collections::HashMap<String, (usize, usize, f32)> = Default::default();
    let mut without_aabb = 0usize;
    let mut blobs = String::new();
    for (entity, mesh, visibility, transform, aabb) in meshes.iter() {
        mesh_entities += 1;
        if names
            .get(entity)
            .is_ok_and(|name| name.as_str() == "BlobShadow")
        {
            let _ = writeln!(
                blobs,
                "  blob shadow at {:.2} scale {:.2} visible {}",
                transform.translation(),
                transform.scale(),
                visibility.get()
            );
        }
        if aabb.is_none() {
            without_aabb += 1;
        }
        if visibility.get() {
            let center = aabb.map_or(transform.translation(), |aabb| {
                transform.transform_point(Vec3::from(aabb.center))
            });
            let distance = camera.map_or(0.0, |camera| camera.distance(center));
            visible_meshes += 1;
            let tris = mesh_assets.get(&mesh.0).map_or(0, |mesh| {
                mesh.indices()
                    .map_or(mesh.count_vertices(), |indices| indices.len())
                    / 3
            });
            triangles += tris;
            // Attribute the mesh to its top-most named ancestor.
            let mut current = entity;
            let mut label = names.get(entity).map(|n| n.as_str().to_owned()).ok();
            while let Ok(child_of) = parents.get(current) {
                current = child_of.parent();
                if let Ok(name) = names.get(current) {
                    label = Some(name.as_str().to_owned());
                }
            }
            // Group numbered copies ("RoadLamp012", "Traffic 4") together.
            let label = label.unwrap_or_else(|| "(unnamed)".to_owned());
            let family = label
                .trim_end_matches(|c: char| c.is_ascii_digit() || c == ' ' || c == '_' || c == '#')
                .to_owned();
            let entry = by_root
                .entry(if family.is_empty() { label } else { family })
                .or_default();
            entry.0 += 1;
            entry.1 += tris;
            entry.2 = entry.2.max(distance);
        }
    }
    let mut by_root: Vec<_> = by_root.into_iter().collect();
    by_root.sort_by_key(|(_, (count, _, _))| std::cmp::Reverse(*count));
    let visible = |query: &mut dyn Iterator<Item = bool>| -> (usize, usize) {
        let mut total = 0;
        let mut shown = 0;
        for visible in query {
            total += 1;
            shown += usize::from(visible);
        }
        (total, shown)
    };
    let (points, points_visible) = visible(&mut point_lights.iter().map(|v| v.get()));
    let (spots, spots_visible) = visible(&mut spot_lights.iter().map(|v| v.get()));
    let shadow_suns = suns.iter().filter(|sun| sun.shadow_maps_enabled).count();

    let mut report = String::new();
    let _ = writeln!(
        report,
        "mobile_benchmark: {}x{} px, {:.1} s, {} frames, night={}",
        window.physical_width(),
        window.physical_height(),
        bench.seconds,
        frames.len(),
        bench.night
    );
    let _ = writeln!(
        report,
        "frame ms: mean {:.1}  median {:.1}  p90 {:.1}  p99 {:.1}  => {:.2} FPS",
        mean,
        percentile(0.5),
        percentile(0.9),
        percentile(0.99),
        1000.0 / mean.max(0.001)
    );
    let _ = writeln!(
        report,
        "entities {entity_count}, mesh entities {mesh_entities} ({without_aabb} without bounds), visible meshes {visible_meshes}, visible triangles {triangles}"
    );
    let _ = writeln!(
        report,
        "point lights {points} ({points_visible} visible), spot lights {spots} ({spots_visible} visible), shadowed suns {shadow_suns}"
    );
    let _ = writeln!(report, "visible meshes by object family:");
    for (label, (count, tris, farthest)) in by_root.iter().take(24) {
        let _ = writeln!(
            report,
            "  {count:>5} meshes {tris:>9} tris  farthest {farthest:>6.0} m  {label}"
        );
    }
    report.push_str(&blobs);
    if let Some(camera) = camera {
        let _ = writeln!(report, "  camera at {camera:.2}");
    }
    let _ = writeln!(report, "graphics: {lighting:?}");
    report
}
