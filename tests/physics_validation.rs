//! Physics validation suite.
//!
//! Every figure the brief asks to be re-checked after a physics change is
//! measured here, headlessly and deterministically, by driving the real solver
//! through the small rigid-body integrator in `vehicle::testbed`. Run
//!
//! ```text
//! cargo test --test physics_validation -- --nocapture
//! ```
//!
//! to see the full table.

use bevy::math::Vec3;
use veyron_sim::vehicle::VehicleKind;
use veyron_sim::vehicle::presets::{
    bugatti_veyron_super_sport, ford_mustang_gt3, mercedes_benz_sls_amg, pagani_zonda_cinque,
};
use veyron_sim::vehicle::solver::{ControlInput, TRIPLE_POWER_MULTIPLIER};
use veyron_sim::vehicle::testbed::VehicleTestbed;

/// Same rate as the game's `FixedUpdate`; the solver sub-steps internally.
const DT: f32 = 1.0 / 120.0;

fn throttle(value: f32) -> ControlInput {
    ControlInput {
        throttle: value,
        ..Default::default()
    }
}

fn brake(value: f32) -> ControlInput {
    ControlInput {
        brake: value,
        ..Default::default()
    }
}

/// Result of one standing-start run.
struct AccelerationRun {
    /// Time from rest to each requested speed [s].
    times: Vec<(f32, Option<f32>)>,
    /// Peak speed reached during the run [km/h].
    peak_kmh: f32,
    /// Highest engine speed observed in each gear [rpm].
    gear_rpm: [f32; 9],
    /// Highest road speed observed in each gear [km/h].
    gear_speed: [f32; 9],
    distance: f32,
}

fn standing_start(marks: &[f32], duration: f32) -> (VehicleTestbed, AccelerationRun) {
    let mut testbed = VehicleTestbed::new(bugatti_veyron_super_sport());
    let mut times: Vec<(f32, Option<f32>)> = marks.iter().map(|m| (*m, None)).collect();
    let mut peak = 0.0f32;
    let mut gear_rpm = [0.0f32; 9];
    let mut gear_speed = [0.0f32; 9];

    let input = throttle(1.0);
    let steps = (duration / DT).ceil() as u32;
    let mut elapsed = 0.0f32;
    for _ in 0..steps {
        testbed.step(&input, DT);
        elapsed += DT;
        let speed = testbed.speed_kmh();
        peak = peak.max(speed);
        let gear = testbed.dynamics.transmission.gear;
        if (1..=8).contains(&gear) {
            let index = gear as usize;
            gear_rpm[index] = gear_rpm[index].max(testbed.dynamics.rpm());
            gear_speed[index] = gear_speed[index].max(speed);
        }
        for entry in times.iter_mut() {
            if entry.1.is_none() && speed >= entry.0 {
                entry.1 = Some(elapsed);
            }
        }
    }

    let distance = testbed.distance;
    (
        testbed,
        AccelerationRun {
            times,
            peak_kmh: peak,
            gear_rpm,
            gear_speed,
            distance,
        },
    )
}

fn power_mode_run(multiplier: f32, duration: f32) -> (f32, f32) {
    let mut testbed = VehicleTestbed::new(bugatti_veyron_super_sport());
    if multiplier > 1.0 {
        testbed.dynamics.toggle_triple_power();
    }
    let input = throttle(1.0);
    let mut speed_at_ten_seconds = 0.0;
    let mut peak_speed: f32 = 0.0;

    for step in 0..(duration / DT).ceil() as u32 {
        testbed.step(&input, DT);
        if step == (10.0 / DT) as u32 {
            speed_at_ten_seconds = testbed.speed_kmh();
        }
        peak_speed = peak_speed.max(testbed.speed_kmh());
    }

    (speed_at_ten_seconds, peak_speed)
}

/// Accelerate to `from_kmh`, then stand on the brakes and measure the distance.
fn braking_distance(from_kmh: f32, abs: bool) -> f32 {
    let mut testbed = VehicleTestbed::new(bugatti_veyron_super_sport());
    testbed.dynamics.abs_enabled = abs;

    // Get up to speed.
    let input = throttle(1.0);
    let mut guard = 0;
    while testbed.speed_kmh() < from_kmh && guard < 120 * 240 {
        testbed.step(&input, DT);
        guard += 1;
    }
    assert!(
        testbed.speed_kmh() >= from_kmh,
        "could not reach {from_kmh} km/h to start the braking test"
    );

    // Brake to a standstill.
    let start = testbed.body.translation;
    let stop = brake(1.0);
    let mut guard = 0;
    while testbed.speed_kmh() > 1.0 && guard < 60 * 240 {
        testbed.step(&stop, DT);
        guard += 1;
    }
    (testbed.body.translation - start).length()
}

#[test]
fn acceleration_matches_the_real_car() {
    let (_, run) = standing_start(&[100.0, 200.0, 300.0], 60.0);

    println!("\n--- standing start ---");
    for (mark, time) in &run.times {
        match time {
            Some(t) => println!("  0-{mark:>3.0} km/h : {t:6.2} s"),
            None => println!("  0-{mark:>3.0} km/h :   not reached"),
        }
    }
    println!("  distance after 60 s: {:.0} m", run.distance);
    println!("  peak speed in 60 s : {:.1} km/h", run.peak_kmh);

    let to_100 = run.times[0].1.expect("must reach 100 km/h");
    let to_200 = run.times[1].1.expect("must reach 200 km/h");
    let to_300 = run.times[2].1.expect("must reach 300 km/h");

    // Published figures for the Super Sport are 2.5 s, 6.7 s and 14.6 s. A
    // keyboard-friendly launch without launch control lands a little either
    // side of those, so the bands are deliberately generous - what matters is
    // that the ordering and the magnitudes stay physical.
    assert!(
        (1.8..=3.6).contains(&to_100),
        "0-100 km/h took {to_100:.2} s"
    );
    assert!(
        (5.0..=9.5).contains(&to_200),
        "0-200 km/h took {to_200:.2} s"
    );
    assert!(
        (11.0..=22.0).contains(&to_300),
        "0-300 km/h took {to_300:.2} s"
    );
    assert!(to_100 < to_200 && to_200 < to_300);
}

#[test]
fn top_speed_emerges_from_power_and_drag() {
    // Long enough for drag to balance the available thrust.
    let (testbed, run) = standing_start(&[400.0], 240.0);
    let terminal = testbed.speed_kmh();

    println!("\n--- top speed ---");
    println!("  peak      : {:.1} km/h", run.peak_kmh);
    println!("  terminal  : {terminal:.1} km/h");
    println!("  drag      : {:.0} N", testbed.dynamics.aero.drag_force);
    println!(
        "  downforce : {:.0} N",
        testbed.dynamics.aero.total_downforce()
    );
    println!("  rpm       : {:.0}", testbed.dynamics.rpm());
    println!("  gear      : {}", testbed.dynamics.transmission.display());

    assert!(
        (395.0..=440.0).contains(&run.peak_kmh),
        "top speed is {:.1} km/h, expected roughly 415 km/h",
        run.peak_kmh
    );
    // Nothing artificially clamps the car: it must be sitting below the
    // limiter, held there by aerodynamic drag.
    let cfg = bugatti_veyron_super_sport();
    assert!(
        testbed.dynamics.rpm() < cfg.engine.limiter_rpm,
        "the car is on the limiter, not drag limited"
    );
    assert!(
        testbed.dynamics.aero.drag_force > 5_000.0,
        "drag at top speed is only {:.0} N",
        testbed.dynamics.aero.drag_force
    );
}

#[test]
fn triple_power_improves_acceleration_and_maximum_speed() {
    let (normal_at_ten, normal_peak) = power_mode_run(1.0, 120.0);
    let (boosted_at_ten, boosted_peak) = power_mode_run(TRIPLE_POWER_MULTIPLIER, 120.0);

    println!("\n--- F7 triple power ---");
    println!("  speed at 10 s: {normal_at_ten:.1} -> {boosted_at_ten:.1} km/h");
    println!("  peak at 120 s: {normal_peak:.1} -> {boosted_peak:.1} km/h");

    assert!(
        boosted_at_ten > normal_at_ten * 1.15,
        "x3 power did not materially improve acceleration: {normal_at_ten:.1} -> {boosted_at_ten:.1} km/h"
    );
    assert!(
        boosted_peak > normal_peak + 10.0,
        "x3 power did not increase maximum speed: {normal_peak:.1} -> {boosted_peak:.1} km/h"
    );
}

#[test]
fn nitro_is_extreme_and_stable_for_every_car() {
    for config in [
        bugatti_veyron_super_sport(),
        ford_mustang_gt3(),
        mercedes_benz_sls_amg(),
        pagani_zonda_cinque(),
    ] {
        let name = config.name;
        let mut ordinary = VehicleTestbed::new(config.clone());
        let mut boosted = VehicleTestbed::new(config);
        let ordinary_input = throttle(1.0);
        let nitro_input = ControlInput {
            throttle: 1.0,
            nitro: 1.0,
            ..Default::default()
        };
        for _ in 0..(3.0 / DT) as usize {
            ordinary.step(&ordinary_input, DT);
            boosted.step(&nitro_input, DT);
        }

        assert!(boosted.speed_kmh().is_finite(), "{name} nitro blew up");
        assert!(
            boosted.speed_kmh() > ordinary.speed_kmh() + 100.0,
            "{name}: ordinary {:.1}, nitro {:.1} km/h",
            ordinary.speed_kmh(),
            boosted.speed_kmh()
        );
        assert!(
            boosted.body.rotation.is_finite(),
            "{name} rotation became invalid"
        );
    }
}

#[test]
fn every_gear_is_used_and_pulls_to_the_top_of_its_range() {
    let (_, run) = standing_start(&[], 90.0);
    println!("\n--- gearing ---");
    let cfg = bugatti_veyron_super_sport();
    for gear in 1..=cfg.transmission.gear_ratios.len() {
        println!(
            "  gear {gear}: max {:6.0} rpm at up to {:6.1} km/h",
            run.gear_rpm[gear], run.gear_speed[gear]
        );
    }
    for gear in 1..=cfg.transmission.gear_ratios.len() {
        assert!(
            run.gear_rpm[gear] > 1000.0,
            "gear {gear} was never engaged under full throttle"
        );
    }
    // Speeds must rise monotonically with the gear.
    for gear in 1..cfg.transmission.gear_ratios.len() {
        assert!(
            run.gear_speed[gear + 1] >= run.gear_speed[gear],
            "gear {} tops out below gear {}",
            gear + 1,
            gear
        );
    }
}

#[test]
fn braking_distances_are_realistic() {
    let from_100 = braking_distance(100.0, true);
    let from_200 = braking_distance(200.0, true);
    let from_100_no_abs = braking_distance(100.0, false);

    println!("\n--- braking ---");
    println!("  100-0 km/h (ABS on) : {from_100:6.1} m");
    println!("  200-0 km/h (ABS on) : {from_200:6.1} m");
    println!("  100-0 km/h (ABS off): {from_100_no_abs:6.1} m");

    // The real car stops from 100 km/h in about 31 m and from 200 km/h in
    // about 120 m.
    assert!(
        (22.0..=45.0).contains(&from_100),
        "100-0 took {from_100:.1} m"
    );
    assert!(
        (85.0..=175.0).contains(&from_200),
        "200-0 took {from_200:.1} m"
    );
    // Locking the wheels must cost distance, which is the whole point of ABS.
    assert!(
        from_100_no_abs >= from_100 - 0.5,
        "ABS off ({from_100_no_abs:.1} m) beat ABS on ({from_100:.1} m)"
    );
}

#[test]
fn the_car_tracks_straight_at_speed() {
    let mut testbed = VehicleTestbed::new(bugatti_veyron_super_sport());
    let input = throttle(1.0);
    // Accelerate hard for a long time and see how far it wanders sideways.
    for _ in 0..(120 * 60) {
        testbed.step(&input, DT);
    }
    let lateral = testbed.body.translation.x.abs();
    let yaw = (testbed.body.rotation * Vec3::NEG_Z).x.abs();
    let roll = (testbed.body.rotation * Vec3::Y).dot(Vec3::Y);

    println!("\n--- straight line stability ---");
    println!("  speed          : {:.1} km/h", testbed.speed_kmh());
    println!(
        "  lateral drift  : {lateral:.3} m over {:.0} m",
        testbed.distance
    );
    println!("  heading error  : {yaw:.4}");
    println!("  upright factor : {roll:.4}");

    assert!(lateral < 1.0, "car drifted {lateral:.2} m sideways");
    assert!(
        yaw < 0.02,
        "car is not pointing straight, heading error {yaw:.4}"
    );
    assert!(roll > 0.99, "car is not upright");
}

#[test]
fn hard_steering_at_speed_does_not_blow_up() {
    let mut testbed = VehicleTestbed::new(bugatti_veyron_super_sport());
    // Get to about 250 km/h.
    let go = throttle(1.0);
    let mut guard = 0;
    while testbed.speed_kmh() < 250.0 && guard < 120 * 120 {
        testbed.step(&go, DT);
        guard += 1;
    }

    // Slam the steering to full lock and hold it.
    let swerve = ControlInput {
        throttle: 1.0,
        steering: 1.0,
        ..Default::default()
    };
    for _ in 0..(120 * 6) {
        testbed.step(&swerve, DT);
        assert!(
            testbed.body.translation.is_finite() && testbed.body.linear_velocity.is_finite(),
            "physics diverged during the swerve"
        );
        assert!(
            testbed.body.linear_velocity.length() < 200.0,
            "speed exploded to {} m/s",
            testbed.body.linear_velocity.length()
        );
    }

    let yaw_rate = testbed.body.angular_velocity.y.abs();
    println!("\n--- swerve at speed ---");
    println!("  speed after   : {:.1} km/h", testbed.speed_kmh());
    println!("  yaw rate      : {yaw_rate:.3} rad/s");
    println!(
        "  any wheel sliding: {}",
        testbed.dynamics.any_wheel_sliding()
    );
    assert!(yaw_rate < 3.0, "yaw rate {yaw_rate:.2} rad/s is unphysical");
}

#[test]
fn low_speed_steering_has_arcade_response_without_spinning() {
    println!("\n--- arcade steering at city speed ---");
    for kind in VehicleKind::ALL {
        let mut testbed = VehicleTestbed::new(kind.config());
        let mut launch_guard = 0;
        while testbed.speed_kmh() < 35.0 && launch_guard < 120 * 20 {
            testbed.step(&throttle(0.55), DT);
            launch_guard += 1;
        }
        assert!(
            testbed.speed_kmh() >= 35.0,
            "{} did not reach the steering test speed",
            kind.display_name()
        );

        let initial_forward = testbed.body.rotation * Vec3::NEG_Z;
        let initial_right = testbed.body.rotation * Vec3::X;
        let input = ControlInput {
            throttle: 0.20,
            steering: 0.70,
            ..Default::default()
        };
        let mut peak_yaw_rate = 0.0_f32;
        for _ in 0..(120.0 * 1.2) as usize {
            testbed.step(&input, DT);
            peak_yaw_rate = peak_yaw_rate.max(testbed.body.angular_velocity.y.abs());
        }

        let final_forward = testbed.body.rotation * Vec3::NEG_Z;
        let heading_change = initial_forward.dot(final_forward).clamp(-1.0, 1.0).acos();
        let upright = (testbed.body.rotation * Vec3::Y).dot(Vec3::Y);
        println!(
            "  {:<34} {:>5.1} deg, yaw {:>5.3}, {:>5.1} km/h",
            kind.display_name(),
            heading_change.to_degrees(),
            peak_yaw_rate,
            testbed.speed_kmh()
        );

        assert!(
            heading_change > 0.30,
            "{} changed heading by only {:.1} degrees",
            kind.display_name(),
            heading_change.to_degrees()
        );
        assert!(
            final_forward.dot(initial_right) > 0.0,
            "{} turned left from a positive/right steering command",
            kind.display_name()
        );
        assert!(
            peak_yaw_rate < 1.8,
            "{} entered a low-speed spin",
            kind.display_name()
        );
        assert!(
            upright > 0.82,
            "{} tipped during the low-speed turn: {upright:.3}",
            kind.display_name()
        );
    }
}

#[test]
fn mustang_full_key_press_at_speed_does_not_turn_into_a_spin() {
    let mut testbed = VehicleTestbed::new(ford_mustang_gt3());
    let go = throttle(1.0);
    let mut guard = 0;
    while testbed.speed_kmh() < 180.0 && guard < 120 * 120 {
        testbed.step(&go, DT);
        guard += 1;
    }
    assert!(
        testbed.speed_kmh() >= 180.0,
        "Mustang did not reach test speed"
    );

    let initial_forward = testbed.body.rotation * Vec3::NEG_Z;
    let mut peak_yaw_rate = 0.0_f32;
    let mut steering = 0.0_f32;

    // Hold the right key for one second.  Ramp it at the Mustang-specific rack
    // rate, exactly as the game controller does, rather than teleporting the
    // steering from centre to full lock.
    for _ in 0..120 {
        steering = (steering + testbed.config.steering.input_rate * DT).min(1.0);
        testbed.step(
            &ControlInput {
                throttle: 1.0,
                steering,
                ..Default::default()
            },
            DT,
        );
        peak_yaw_rate = peak_yaw_rate.max(testbed.body.angular_velocity.y.abs());
    }

    let forward = testbed.body.rotation * Vec3::NEG_Z;
    let heading_dot = forward.dot(initial_forward);
    println!("\n--- Mustang reduced steering at 180 km/h ---");
    println!("  heading dot   : {heading_dot:.3}");
    println!("  peak yaw rate : {peak_yaw_rate:.3} rad/s");
    println!("  final speed   : {:.1} km/h", testbed.speed_kmh());

    assert!(
        heading_dot > 0.0,
        "Mustang rotated past 90 degrees during a one-second key press"
    );
    assert!(
        peak_yaw_rate < 1.5,
        "Mustang entered an excessive yaw: {peak_yaw_rate:.3} rad/s"
    );
}

#[test]
fn the_car_is_deterministic() {
    let script = |testbed: &mut VehicleTestbed| {
        for step in 0..(120 * 20) {
            let phase = step as f32 * DT;
            let input = ControlInput {
                throttle: if phase < 8.0 { 1.0 } else { 0.2 },
                brake: if (10.0..12.0).contains(&phase) {
                    1.0
                } else {
                    0.0
                },
                steering: (phase * 0.7).sin() * 0.4,
                handbrake: 0.0,
                nitro: 0.0,
            };
            testbed.step(&input, DT);
        }
    };

    let mut a = VehicleTestbed::new(bugatti_veyron_super_sport());
    let mut b = VehicleTestbed::new(bugatti_veyron_super_sport());
    script(&mut a);
    script(&mut b);

    assert_eq!(a.body.translation, b.body.translation);
    assert_eq!(a.body.rotation, b.body.rotation);
    assert_eq!(a.dynamics.rpm(), b.dynamics.rpm());
}

#[test]
fn wheels_spin_up_under_power_and_lock_without_abs() {
    // Full throttle from rest must produce visible wheel spin on the driven
    // axle: the tyre force is finite, so the hub accelerates faster than the car.
    let mut testbed = VehicleTestbed::new(bugatti_veyron_super_sport());
    let mut peak_slip: f32 = 0.0;
    let input = throttle(1.0);
    for _ in 0..(120 * 3) {
        testbed.step(&input, DT);
        for wheel in &testbed.dynamics.wheels {
            peak_slip = peak_slip.max(wheel.tire.slip_ratio);
        }
    }
    println!("\n--- traction ---");
    println!("  peak launch slip ratio: {peak_slip:.3}");
    assert!(
        peak_slip > 0.02,
        "no wheel slip at all under 1200 PS: {peak_slip:.4}"
    );

    // With ABS disabled, standing on the brakes must lock the wheels.
    let mut testbed = VehicleTestbed::new(bugatti_veyron_super_sport());
    testbed.dynamics.abs_enabled = false;
    let mut guard = 0;
    while testbed.speed_kmh() < 120.0 && guard < 120 * 120 {
        testbed.step(&input, DT);
        guard += 1;
    }
    let stop = brake(1.0);
    let mut min_slip: f32 = 0.0;
    for _ in 0..(120 * 2) {
        testbed.step(&stop, DT);
        for wheel in &testbed.dynamics.wheels {
            min_slip = min_slip.min(wheel.tire.slip_ratio);
        }
    }
    println!("  most negative braking slip (ABS off): {min_slip:.3}");
    assert!(min_slip < -0.3, "wheels never locked without ABS");
}

#[test]
fn downforce_and_drag_grow_with_speed() {
    let mut testbed = VehicleTestbed::new(bugatti_veyron_super_sport());
    let input = throttle(1.0);
    let mut samples = Vec::new();
    let mut next_mark = 100.0f32;
    for _ in 0..(120 * 120) {
        testbed.step(&input, DT);
        if testbed.speed_kmh() >= next_mark && next_mark <= 400.0 {
            samples.push((
                next_mark,
                testbed.dynamics.aero.drag_force,
                testbed.dynamics.aero.total_downforce(),
                testbed
                    .dynamics
                    .wheels
                    .iter()
                    .map(|w| w.normal_force)
                    .sum::<f32>(),
            ));
            next_mark += 100.0;
        }
    }

    println!("\n--- aerodynamics ---");
    for (speed, drag, downforce, load) in &samples {
        println!(
            "  {speed:5.0} km/h: drag {drag:7.0} N  downforce {downforce:7.0} N  total wheel load {load:8.0} N"
        );
    }
    assert!(samples.len() >= 3, "did not reach 300 km/h");
    for pair in samples.windows(2) {
        assert!(pair[1].1 > pair[0].1, "drag did not grow with speed");
        assert!(pair[1].2 > pair[0].2, "downforce did not grow with speed");
    }
    // Downforce must actually reach the tyres.
    let static_weight = bugatti_veyron_super_sport().mass * 9.81;
    let (_, _, downforce, load) = samples[samples.len() - 1];
    assert!(
        load > static_weight + downforce * 0.6,
        "wheel load {load:.0} N does not reflect {downforce:.0} N of downforce"
    );
}

#[test]
fn added_cars_launch_shift_and_remain_stable() {
    for config in [mercedes_benz_sls_amg(), pagani_zonda_cinque()] {
        let name = config.name;
        let mut testbed = VehicleTestbed::new(config);
        let initial_x = testbed.body.translation.x;
        let mut peak_speed = 0.0_f32;
        let mut highest_gear = 0;
        for _ in 0..(120 * 45) {
            testbed.step(&throttle(1.0), DT);
            peak_speed = peak_speed.max(testbed.speed_kmh());
            highest_gear = highest_gear.max(testbed.dynamics.transmission.gear);
            assert!(
                testbed.body.translation.is_finite()
                    && testbed.body.linear_velocity.is_finite()
                    && testbed.body.angular_velocity.is_finite(),
                "{name} physics diverged"
            );
        }
        println!(
            "{name}: {peak_speed:.1} km/h, gear {highest_gear}, lateral drift {:.3} m",
            (testbed.body.translation.x - initial_x).abs()
        );
        assert!(peak_speed > 220.0, "{name} is unexpectedly slow");
        assert!(
            highest_gear >= 4,
            "{name} did not shift through its gearbox"
        );
        assert!(
            (testbed.body.translation.x - initial_x).abs() < 0.25,
            "{name} drifts without steering input"
        );
        assert!(
            testbed.body.translation.y.abs() < 1.0,
            "{name} launched vertically"
        );
    }
}
