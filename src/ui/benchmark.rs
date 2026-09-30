//! Optional repeatable FPS benchmark for release verification.
//!
//! Set `VEYRON_BENCHMARK_SECONDS` to a positive number. The game warms up for
//! three seconds, measures rendered frames for the requested interval, prints a
//! machine-readable result and exits cleanly.

use bevy::prelude::*;

use crate::world::lighting::LightingSettings;

const BENCHMARK_ENV: &str = "VEYRON_BENCHMARK_SECONDS";
const WARMUP_SECONDS: f64 = 3.0;

#[derive(Resource)]
pub struct FpsBenchmark {
    duration: Option<f64>,
    started_at: Option<f64>,
    frame_times: Vec<f64>,
    finished: bool,
}

impl Default for FpsBenchmark {
    fn default() -> Self {
        let duration = std::env::var(BENCHMARK_ENV)
            .ok()
            .and_then(|value| value.trim().parse::<f64>().ok())
            .filter(|seconds| seconds.is_finite() && *seconds > 0.0);

        Self {
            duration,
            started_at: None,
            frame_times: Vec::new(),
            finished: false,
        }
    }
}

pub fn measure_fps(
    time: Res<Time<Real>>,
    lighting: Res<LightingSettings>,
    mut benchmark: ResMut<FpsBenchmark>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(duration) = benchmark.duration else {
        return;
    };
    if benchmark.finished || time.elapsed_secs_f64() < WARMUP_SECONDS {
        return;
    }

    let started_at = *benchmark
        .started_at
        .get_or_insert_with(|| time.elapsed_secs_f64());
    let delta = time.delta_secs_f64();
    if delta.is_finite() && delta > 0.0 {
        benchmark.frame_times.push(delta);
    }

    if time.elapsed_secs_f64() - started_at < duration {
        return;
    }

    let elapsed: f64 = benchmark.frame_times.iter().sum();
    let average_fps = benchmark.frame_times.len() as f64 / elapsed.max(f64::EPSILON);
    benchmark.frame_times.sort_by(f64::total_cmp);
    let percentile_index = ((benchmark.frame_times.len() as f64 * 0.99).ceil() as usize)
        .saturating_sub(1)
        .min(benchmark.frame_times.len().saturating_sub(1));
    let one_percent_low = benchmark
        .frame_times
        .get(percentile_index)
        .map_or(0.0, |frame_time| 1.0 / frame_time);

    println!(
        "VEYRON_BENCHMARK quality={} frames={} seconds={elapsed:.3} average_fps={average_fps:.1} one_percent_low_fps={one_percent_low:.1}",
        lighting.quality.label(),
        benchmark.frame_times.len(),
    );
    benchmark.finished = true;
    exit.write(AppExit::Success);
}
