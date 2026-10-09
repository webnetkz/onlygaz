//! Frame rate in the log while driving, for testing phones.
//!
//! Every [`REPORT_SECONDS`] of a race the phone build writes one line such as
//!
//! ```text
//! OnlyGAZ frame rate: 58.4 fps average, 41.2 fps 1% low (LITE, 3D 1200x540)
//! ```
//!
//! On Android it appears in Android Studio's Logcat (search for "frame rate").
//! Menus and pauses are not measured.

use bevy::prelude::*;

use crate::camera::render_scale::SceneImage;
use crate::game::GameState;
use crate::world::lighting::LightingSettings;

/// Length of one measurement [s].
pub const REPORT_SECONDS: f64 = 5.0;

#[derive(Default)]
pub(super) struct FrameWindow {
    started: Option<f64>,
    frame_times: Vec<f64>,
}

/// Average frame rate and the 1 % low (the rate of the frame at the 99th
/// percentile of frame times) of `frame_times` [s].
pub fn frame_rate_summary(frame_times: &[f64]) -> Option<(f64, f64)> {
    let total: f64 = frame_times.iter().sum();
    if frame_times.is_empty() || total <= 0.0 {
        return None;
    }
    let mut sorted = frame_times.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = ((sorted.len() as f64 * 0.99).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1);
    let one_percent_low = 1.0 / sorted[index].max(f64::EPSILON);
    Some((sorted.len() as f64 / total, one_percent_low))
}

pub(super) fn log_frame_rate(
    time: Res<Time<Real>>,
    state: Res<State<GameState>>,
    settings: Res<LightingSettings>,
    scene: Option<Res<SceneImage>>,
    mut window: Local<FrameWindow>,
) {
    let now = time.elapsed_secs_f64();
    if *state.get() != GameState::Driving {
        *window = FrameWindow::default();
        return;
    }
    let started = *window.started.get_or_insert(now);
    let delta = time.delta_secs_f64();
    if delta.is_finite() && delta > 0.0 {
        window.frame_times.push(delta);
    }
    if now - started < REPORT_SECONDS {
        return;
    }
    if let Some((average, low)) = frame_rate_summary(&window.frame_times) {
        let resolution = scene.and_then(|scene| scene.size()).map_or_else(
            || "native".to_owned(),
            |size| format!("{}x{}", size.x, size.y),
        );
        info!(
            "OnlyGAZ frame rate: {average:.1} fps average, {low:.1} fps 1% low ({}, 3D {resolution})",
            settings.quality.label(),
        );
    }
    *window = FrameWindow {
        started: Some(now),
        frame_times: Vec::new(),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_reports_the_average_and_the_slowest_percent() {
        assert_eq!(frame_rate_summary(&[]), None);
        let mut frames = vec![1.0 / 60.0; 99];
        frames.push(1.0 / 20.0);
        let (average, low) = frame_rate_summary(&frames).unwrap();
        assert!((average - 100.0 / (99.0 / 60.0 + 1.0 / 20.0)).abs() < 1.0e-9);
        assert!((low - 60.0).abs() < 1.0e-6, "{low}");
        let (_, worst) = frame_rate_summary(&[1.0 / 60.0, 1.0 / 20.0]).unwrap();
        assert!((worst - 20.0).abs() < 1.0e-6);
    }
}
