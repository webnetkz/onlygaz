//! Performance measurement.
//!
//! The brief asks for a fixed set of numbers to be re-checked after every change
//! to the physics. They are measured here at runtime and printed to the log, and
//! the same quantities are asserted in `tests/physics_validation.rs` so a
//! regression fails the build rather than merely looking wrong on screen.

use bevy::prelude::*;

use super::physics::{Vehicle, VehicleTag};

/// Speed thresholds that are timed from a standstill [km/h].
pub const ACCELERATION_MARKS: [f32; 3] = [100.0, 200.0, 300.0];
/// Speeds whose braking distance to a standstill is measured [km/h].
pub const BRAKING_MARKS: [f32; 2] = [100.0, 200.0];
/// Below this the car counts as stopped [km/h].
const STOPPED_KMH: f32 = 1.0;
/// Brake pedal position that starts a braking measurement.
const BRAKING_PEDAL: f32 = 0.6;

/// One completed or in-progress measurement.
#[derive(Debug, Clone, Copy, Default)]
pub struct Measurement {
    pub value: Option<f32>,
    pub best: Option<f32>,
}

impl Measurement {
    fn record(&mut self, value: f32, lower_is_better: bool) {
        self.value = Some(value);
        self.best = Some(match self.best {
            Some(best) if lower_is_better => best.min(value),
            Some(best) => best.max(value),
            None => value,
        });
    }
}

/// Live performance figures for the player's car.
#[derive(Debug, Clone, Resource)]
pub struct PerformanceLog {
    /// Times from rest to each of [`ACCELERATION_MARKS`] [s].
    pub acceleration: [Measurement; ACCELERATION_MARKS.len()],
    /// Distances from each of [`BRAKING_MARKS`] to rest [m].
    pub braking: [Measurement; BRAKING_MARKS.len()],
    /// Highest speed reached [km/h].
    pub top_speed: f32,
    /// Highest engine speed seen in each gear [rpm].
    pub gear_max_rpm: [f32; 8],
    /// Highest road speed reached in each gear [km/h].
    pub gear_max_speed: [f32; 8],
    /// Lateral acceleration magnitude, low pass filtered [g].
    pub lateral_g: f32,
    /// Longitudinal acceleration, low pass filtered [g].
    pub longitudinal_g: f32,

    run_time: f32,
    launched: bool,
    reached: [bool; ACCELERATION_MARKS.len()],
    braking_marks: [Option<f32>; BRAKING_MARKS.len()],
    braking_active: bool,
    previous_speed: f32,
    previous_velocity: Vec3,
}

impl Default for PerformanceLog {
    fn default() -> Self {
        Self {
            acceleration: Default::default(),
            braking: Default::default(),
            top_speed: 0.0,
            gear_max_rpm: [0.0; 8],
            gear_max_speed: [0.0; 8],
            lateral_g: 0.0,
            longitudinal_g: 0.0,
            run_time: 0.0,
            launched: false,
            reached: [false; ACCELERATION_MARKS.len()],
            braking_marks: [None; BRAKING_MARKS.len()],
            braking_active: false,
            previous_speed: 0.0,
            previous_velocity: Vec3::ZERO,
        }
    }
}

impl PerformanceLog {
    /// Forget the in-progress run, keeping the records.
    pub fn restart_run(&mut self) {
        self.run_time = 0.0;
        self.launched = false;
        self.reached = [false; ACCELERATION_MARKS.len()];
        self.braking_marks = [None; BRAKING_MARKS.len()];
        self.braking_active = false;
    }

    /// A short summary for the debug HUD.
    pub fn summary(&self) -> String {
        let mut lines = Vec::new();
        for (index, mark) in ACCELERATION_MARKS.iter().enumerate() {
            let text = self.acceleration[index]
                .best
                .map(|v| format!("{v:.2} s"))
                .unwrap_or_else(|| "-".to_string());
            lines.push(format!("0-{mark:.0}: {text}"));
        }
        for (index, mark) in BRAKING_MARKS.iter().enumerate() {
            let text = self.braking[index]
                .best
                .map(|v| format!("{v:.1} m"))
                .unwrap_or_else(|| "-".to_string());
            lines.push(format!("{mark:.0}-0: {text}"));
        }
        lines.push(format!("Vmax: {:.1} km/h", self.top_speed));
        lines.join("   ")
    }
}

/// Update the performance log from the car.
pub fn record_performance(
    time: Res<Time>,
    mut log: ResMut<PerformanceLog>,
    vehicles: Query<(&Transform, &Vehicle), With<VehicleTag>>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let Ok((transform, vehicle)) = vehicles.single() else {
        return;
    };

    let speed = vehicle.speed_kmh();
    let position = transform.translation;

    // --- top speed and per gear records ------------------------------------
    if speed > log.top_speed {
        log.top_speed = speed;
    }
    let gear = vehicle.dynamics.transmission.gear;
    if (1..=7).contains(&gear) {
        let index = gear as usize;
        let rpm = vehicle.dynamics.rpm();
        if rpm > log.gear_max_rpm[index] {
            log.gear_max_rpm[index] = rpm;
        }
        if speed > log.gear_max_speed[index] {
            log.gear_max_speed[index] = speed;
        }
    }

    // --- g meters ----------------------------------------------------------
    let velocity = transform.rotation * Vec3::NEG_Z * (speed / 3.6);
    let acceleration = (velocity - log.previous_velocity) / dt;
    log.previous_velocity = velocity;
    let forward = transform.rotation * Vec3::NEG_Z;
    let right = transform.rotation * Vec3::X;
    let longitudinal = acceleration.dot(forward) / 9.81;
    let lateral = acceleration.dot(right) / 9.81;
    log.longitudinal_g += (longitudinal - log.longitudinal_g) * (dt * 6.0).min(1.0);
    log.lateral_g += (lateral - log.lateral_g) * (dt * 6.0).min(1.0);

    // --- standing start ----------------------------------------------------
    if speed < STOPPED_KMH {
        log.restart_run();
    } else if !log.launched && speed > STOPPED_KMH {
        log.launched = true;
        log.run_time = 0.0;
    }

    if log.launched {
        log.run_time += dt;
        for (index, mark) in ACCELERATION_MARKS.iter().enumerate() {
            if !log.reached[index] && speed >= *mark {
                log.reached[index] = true;
                let elapsed = log.run_time;
                log.acceleration[index].record(elapsed, true);
                info!("0-{mark:.0} km/h in {elapsed:.2} s");
            }
        }
    }

    // --- braking -----------------------------------------------------------
    let braking = vehicle.input.brake > BRAKING_PEDAL;
    if braking && !log.braking_active {
        log.braking_active = true;
        log.braking_marks = [None; BRAKING_MARKS.len()];
    } else if !braking && speed > STOPPED_KMH {
        log.braking_active = false;
    }

    if log.braking_active {
        for (index, mark) in BRAKING_MARKS.iter().enumerate() {
            // The instant the car drops through the mark, remember where it was.
            if log.braking_marks[index].is_none() && log.previous_speed >= *mark && speed < *mark {
                log.braking_marks[index] = Some(position.z);
            }
        }
        if speed < STOPPED_KMH {
            for (index, mark) in BRAKING_MARKS.iter().enumerate() {
                if let Some(start) = log.braking_marks[index] {
                    let distance = (position.z - start).abs();
                    log.braking[index].record(distance, true);
                    info!("{mark:.0}-0 km/h in {distance:.1} m");
                }
            }
            log.braking_marks = [None; BRAKING_MARKS.len()];
            log.braking_active = false;
        }
    }

    log.previous_speed = speed;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measurement_keeps_the_best_value() {
        let mut m = Measurement::default();
        m.record(3.0, true);
        m.record(2.5, true);
        m.record(4.0, true);
        assert_eq!(m.best, Some(2.5));
        assert_eq!(m.value, Some(4.0));
    }

    #[test]
    fn summary_lists_every_metric() {
        let log = PerformanceLog::default();
        let text = log.summary();
        for mark in ACCELERATION_MARKS {
            assert!(text.contains(&format!("0-{mark:.0}")), "{text}");
        }
        assert!(text.contains("Vmax"));
    }
}
