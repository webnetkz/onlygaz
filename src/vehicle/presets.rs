//! Concrete car definitions.
//!
//! Adding another car means adding another function here; nothing in the solver
//! is specialised to the Veyron.

use bevy::prelude::*;

use super::config::*;

/// Degrees to radians, for readability in the tables below.
const fn deg(d: f32) -> f32 {
    d * core::f32::consts::PI / 180.0
}

/// km/h to m/s.
const fn kmh(v: f32) -> f32 {
    v / 3.6
}

// ---------------------------------------------------------------------------
// Geometry measured from the supplied glTF model (see `docs/model_analysis.md`).
// The model is 1:1 in metres; these are the tyre centres after the 180 deg yaw
// flip that converts the model's "+Z forward" authoring to Bevy's "-Z forward".
// ---------------------------------------------------------------------------

/// Longitudinal position of the front axle, vehicle local [m].
const FRONT_AXLE_Z: f32 = -1.2905;
/// Longitudinal position of the rear axle, vehicle local [m].
const REAR_AXLE_Z: f32 = 1.4165;
/// Half of the front track [m].
const FRONT_HALF_TRACK: f32 = 0.8635;
/// Half of the rear track [m].
const REAR_HALF_TRACK: f32 = 0.8075;
/// Strut anchor height above the ground for the front axle [m].
const FRONT_ANCHOR_Y: f32 = 0.62;
/// Strut anchor height above the ground for the rear axle [m].
const REAR_ANCHOR_Y: f32 = 0.66;

const FRONT_TIRE: TireConfig = TireConfig {
    // Michelin PAX 265/680 R500A.
    radius: 0.340,
    width: 0.265,
    inertia: 1.6,
    peak_friction: 1.45,
    nominal_load: 4500.0,
    load_sensitivity: 0.11,
    friction_clamp: (0.80, 1.95),
    // Peaks at a slip ratio of ~0.16 and still holds ~90 % of peak at full
    // lock; a classic Pacejka Fx shape.
    longitudinal: MagicFormula {
        b: 11.5,
        c: 1.90,
        e: 0.97,
    },
    // Peaks at ~9 degrees of slip angle and falls to ~76 % well past it.
    lateral: MagicFormula {
        b: 9.8,
        c: 1.45,
        e: -0.50,
    },
    relaxation_long: 0.35,
    relaxation_lat: 0.60,
    rolling_resistance: 0.013,
    rolling_resistance_speed: 0.00006,
};

const REAR_TIRE: TireConfig = TireConfig {
    // Michelin PAX 365/710 R540A.
    radius: 0.3577,
    width: 0.365,
    inertia: 2.1,
    peak_friction: 1.50,
    nominal_load: 5200.0,
    load_sensitivity: 0.11,
    friction_clamp: (0.80, 1.95),
    longitudinal: MagicFormula {
        b: 11.5,
        c: 1.90,
        e: 0.97,
    },
    // The wider rear tyre peaks a little later.
    lateral: MagicFormula {
        b: 9.2,
        c: 1.45,
        e: -0.50,
    },
    relaxation_long: 0.35,
    relaxation_lat: 0.60,
    rolling_resistance: 0.013,
    rolling_resistance_speed: 0.00006,
};

// Spring rates are picked so the static sag is ~55 mm, which puts the natural
// frequency at ~2.1 Hz - firm, as befits the car.
const FRONT_SUSPENSION: SuspensionConfig = SuspensionConfig {
    rest_length: 0.33,
    max_compression: 0.10,
    max_droop: 0.09,
    spring_stiffness: 76_000.0,
    progressive_stiffness: 260_000.0,
    damper_compression: 4_200.0,
    damper_rebound: 6_000.0,
    bump_stop_stiffness: 600_000.0,
    anti_roll_stiffness: 30_000.0,
};

const REAR_SUSPENSION: SuspensionConfig = SuspensionConfig {
    rest_length: 0.36,
    max_compression: 0.10,
    max_droop: 0.09,
    spring_stiffness: 88_000.0,
    progressive_stiffness: 300_000.0,
    damper_compression: 4_900.0,
    damper_rebound: 6_900.0,
    bump_stop_stiffness: 600_000.0,
    anti_roll_stiffness: 20_000.0,
};

const FRONT_BRAKE_TORQUE: f32 = 5_200.0;
const REAR_BRAKE_TORQUE: f32 = 3_200.0;
const HANDBRAKE_TORQUE: f32 = 5_000.0;

fn wheel(id: WheelId) -> WheelConfig {
    let front = id.is_front();
    let sign = if id.is_left() { -1.0 } else { 1.0 };
    WheelConfig {
        id,
        anchor: if front {
            Vec3::new(sign * FRONT_HALF_TRACK, FRONT_ANCHOR_Y, FRONT_AXLE_Z)
        } else {
            Vec3::new(sign * REAR_HALF_TRACK, REAR_ANCHOR_Y, REAR_AXLE_Z)
        },
        steerable: front,
        tire: if front { FRONT_TIRE } else { REAR_TIRE },
        suspension: if front {
            FRONT_SUSPENSION
        } else {
            REAR_SUSPENSION
        },
        brake_torque: if front {
            FRONT_BRAKE_TORQUE
        } else {
            REAR_BRAKE_TORQUE
        },
        handbrake_torque: if front { 0.0 } else { HANDBRAKE_TORQUE },
    }
}

/// 2010 Bugatti Veyron 16.4 Super Sport.
///
/// The published figures the numbers below are aimed at:
/// 1200 PS (883 kW) at 6400 rpm, 1500 N·m from 3000-5000 rpm, 1888 kg,
/// 7-speed dual clutch, permanent all-wheel drive, 431 km/h (limited to 415
/// for customer cars), 0-100 km/h in 2.5 s.
pub fn bugatti_veyron_super_sport() -> VehicleConfig {
    VehicleConfig {
        name: "Bugatti Veyron 16.4 Super Sport",
        mass: 1888.0,
        // Roughly 60 % of the equivalent solid box - typical for a car whose
        // mass is concentrated low and between the axles.
        inertia: Vec3::new(2700.0, 2900.0, 700.0),
        // 45 / 55 front-rear static weight split, centre of gravity 0.44 m up.
        center_of_mass: Vec3::new(0.0, 0.44, 0.199),
        wheel_base: REAR_AXLE_Z - FRONT_AXLE_Z,
        track_width_front: 2.0 * FRONT_HALF_TRACK,
        track_width_rear: 2.0 * REAR_HALF_TRACK,
        wheels: [
            wheel(WheelId::FrontLeft),
            wheel(WheelId::FrontRight),
            wheel(WheelId::RearLeft),
            wheel(WheelId::RearRight),
        ],
        engine: EngineConfig {
            // 8.0 l quad-turbo W16. The plateau between 3000 and 5000 rpm is
            // the published 1500 N·m; peak power lands at 6400 rpm.
            torque_curve: TorqueCurve::new(vec![
                (700.0, 320.0),
                (1000.0, 600.0),
                (1500.0, 900.0),
                (2000.0, 1150.0),
                (2500.0, 1360.0),
                (3000.0, 1500.0),
                (3500.0, 1500.0),
                (4000.0, 1500.0),
                (4500.0, 1500.0),
                (5000.0, 1500.0),
                (5500.0, 1450.0),
                (6000.0, 1390.0),
                (6400.0, 1320.0),
                (6800.0, 1180.0),
                (7000.0, 980.0),
            ]),
            inertia: 0.42,
            idle_rpm: 900.0,
            redline_rpm: 6800.0,
            limiter_rpm: 6900.0,
            limiter_hysteresis_rpm: 250.0,
            engine_brake_constant: 25.0,
            engine_brake_per_rad: 0.28,
            idle_control_torque: 180.0,
        },
        transmission: TransmissionConfig {
            gear_ratios: vec![2.83, 1.95, 1.46, 1.14, 0.92, 0.73, 0.60],
            reverse_ratio: 2.60,
            final_drive_ratio: 3.44,
            efficiency: 0.90,
            upshift_rpm: 6600.0,
            downshift_rpm: 3100.0,
            shift_time: 0.12,
            shift_cooldown: 0.55,
            clutch_capacity: 2200.0,
            clutch_lock_speed: 4.0,
        },
        drivetrain: Drivetrain::Awd,
        front_torque_bias: 0.30,
        differential_locking: 0.45,
        brakes: BrakeConfig {
            front_torque: FRONT_BRAKE_TORQUE,
            rear_torque: REAR_BRAKE_TORQUE,
            handbrake_torque: HANDBRAKE_TORQUE,
            abs_slip_target: 0.14,
            abs_gain: 8.0,
            abs_enabled: true,
        },
        steering: SteeringConfig {
            max_angle: deg(35.0),
            speed_curve: vec![
                (0.0, deg(35.0)),
                (kmh(60.0), deg(22.0)),
                (kmh(120.0), deg(12.0)),
                (kmh(200.0), deg(6.0)),
                (kmh(300.0), deg(3.0)),
                (kmh(400.0), deg(1.8)),
            ],
            input_rate: 3.6,
            return_rate: 6.0,
            ackermann: true,
            steering_wheel_rotation: deg(220.0),
        },
        aero: AeroConfig {
            air_density: 1.225,
            drag_coefficient: 0.38,
            frontal_area: 2.07,
            lift_coefficient_front: 0.114,
            lift_coefficient_rear: 0.170,
            drag_center: Vec3::new(0.0, 0.55, 0.0),
            downforce_center_front: Vec3::new(0.0, 0.35, -1.60),
            downforce_center_rear: Vec3::new(0.0, 0.90, 1.60),
        },
        // Two convex boxes: the low, wide tub and the cabin. Both sit above the
        // contact patches so they never fight the suspension ray casts.
        collider_boxes: vec![
            (Vec3::new(0.0, 0.42, 0.00), Vec3::new(0.95, 0.30, 2.15)),
            (Vec3::new(0.0, 0.92, -0.05), Vec3::new(0.72, 0.22, 0.72)),
        ],
        driver_eye: Vec3::new(-0.388, 1.00, -0.10),
        cockpit_camera_offset: Vec3::ZERO,
        cockpit_camera_yaw_degrees: 0.0,
        steering_wheel_center: Vec3::new(-0.388, 0.766, -0.540),
        steering_wheel_axis: Vec3::new(0.0, 0.407, 0.914),
        solver_substeps: 4,
    }
}

// ---------------------------------------------------------------------------
// Ford Mustang GT3
// ---------------------------------------------------------------------------

const MUSTANG_FRONT_AXLE_Z: f32 = -1.36;
const MUSTANG_REAR_AXLE_Z: f32 = 1.36;
const MUSTANG_FRONT_HALF_TRACK: f32 = 0.855;
const MUSTANG_REAR_HALF_TRACK: f32 = 0.845;

const MUSTANG_FRONT_TIRE: TireConfig = TireConfig {
    radius: 0.345,
    width: 0.325,
    inertia: 1.75,
    peak_friction: 1.82,
    nominal_load: 3_250.0,
    load_sensitivity: 0.10,
    friction_clamp: (1.05, 2.15),
    longitudinal: MagicFormula {
        b: 12.4,
        c: 1.88,
        e: 0.96,
    },
    lateral: MagicFormula {
        b: 10.8,
        c: 1.48,
        e: -0.35,
    },
    relaxation_long: 0.28,
    relaxation_lat: 0.48,
    rolling_resistance: 0.015,
    rolling_resistance_speed: 0.00007,
};

const MUSTANG_REAR_TIRE: TireConfig = TireConfig {
    radius: 0.350,
    width: 0.325,
    inertia: 1.85,
    peak_friction: 1.86,
    nominal_load: 3_250.0,
    load_sensitivity: 0.10,
    friction_clamp: (1.05, 2.20),
    longitudinal: MagicFormula {
        b: 12.4,
        c: 1.88,
        e: 0.96,
    },
    lateral: MagicFormula {
        b: 10.5,
        c: 1.48,
        e: -0.35,
    },
    relaxation_long: 0.28,
    relaxation_lat: 0.48,
    rolling_resistance: 0.015,
    rolling_resistance_speed: 0.00007,
};

const MUSTANG_FRONT_SUSPENSION: SuspensionConfig = SuspensionConfig {
    rest_length: 0.285,
    max_compression: 0.075,
    max_droop: 0.070,
    spring_stiffness: 96_000.0,
    progressive_stiffness: 360_000.0,
    damper_compression: 6_200.0,
    damper_rebound: 8_500.0,
    bump_stop_stiffness: 850_000.0,
    anti_roll_stiffness: 48_000.0,
};

const MUSTANG_REAR_SUSPENSION: SuspensionConfig = SuspensionConfig {
    rest_length: 0.295,
    max_compression: 0.080,
    max_droop: 0.075,
    spring_stiffness: 104_000.0,
    progressive_stiffness: 380_000.0,
    damper_compression: 6_500.0,
    damper_rebound: 8_900.0,
    bump_stop_stiffness: 850_000.0,
    anti_roll_stiffness: 42_000.0,
};

const MUSTANG_FRONT_BRAKE_TORQUE: f32 = 5_900.0;
const MUSTANG_REAR_BRAKE_TORQUE: f32 = 4_100.0;
const MUSTANG_HANDBRAKE_TORQUE: f32 = 5_200.0;

fn mustang_wheel(id: WheelId) -> WheelConfig {
    let front = id.is_front();
    let sign = if id.is_left() { -1.0 } else { 1.0 };
    WheelConfig {
        id,
        anchor: if front {
            Vec3::new(sign * MUSTANG_FRONT_HALF_TRACK, 0.670, MUSTANG_FRONT_AXLE_Z)
        } else {
            Vec3::new(sign * MUSTANG_REAR_HALF_TRACK, 0.690, MUSTANG_REAR_AXLE_Z)
        },
        steerable: front,
        tire: if front {
            MUSTANG_FRONT_TIRE
        } else {
            MUSTANG_REAR_TIRE
        },
        suspension: if front {
            MUSTANG_FRONT_SUSPENSION
        } else {
            MUSTANG_REAR_SUSPENSION
        },
        brake_torque: if front {
            MUSTANG_FRONT_BRAKE_TORQUE
        } else {
            MUSTANG_REAR_BRAKE_TORQUE
        },
        handbrake_torque: if front { 0.0 } else { MUSTANG_HANDBRAKE_TORQUE },
    }
}

/// 2024 Ford Mustang GT3.
///
/// The fixed facts are Ford's 5.4-litre naturally aspirated Coyote V8,
/// rear-mounted six-speed transaxle, rear-wheel drive, bespoke SLA suspension
/// and carbon body/aero package. Exact GT3 output, mass and aero are subject to
/// series Balance of Performance, so this preset represents a neutral 500 PS,
/// 1300 kg baseline rather than pretending one event's BoP sheet is universal.
pub fn ford_mustang_gt3() -> VehicleConfig {
    VehicleConfig {
        name: "Ford Mustang GT3",
        mass: 1_300.0,
        inertia: Vec3::new(1_850.0, 2_250.0, 570.0),
        // Slight rear bias from the rear transaxle, with a racing-low CG.
        center_of_mass: Vec3::new(0.0, 0.365, 0.055),
        wheel_base: MUSTANG_REAR_AXLE_Z - MUSTANG_FRONT_AXLE_Z,
        track_width_front: 2.0 * MUSTANG_FRONT_HALF_TRACK,
        track_width_rear: 2.0 * MUSTANG_REAR_HALF_TRACK,
        wheels: [
            mustang_wheel(WheelId::FrontLeft),
            mustang_wheel(WheelId::FrontRight),
            mustang_wheel(WheelId::RearLeft),
            mustang_wheel(WheelId::RearRight),
        ],
        engine: EngineConfig {
            // Broad naturally aspirated curve. 500 N·m at 7000 rpm is 498 PS.
            torque_curve: TorqueCurve::new(vec![
                (1_100.0, 255.0),
                (2_000.0, 330.0),
                (3_000.0, 385.0),
                (4_000.0, 430.0),
                (5_000.0, 466.0),
                (6_000.0, 490.0),
                (7_000.0, 500.0),
                (7_400.0, 470.0),
                (7_700.0, 420.0),
            ]),
            inertia: 0.25,
            idle_rpm: 1_100.0,
            redline_rpm: 7_500.0,
            limiter_rpm: 7_700.0,
            limiter_hysteresis_rpm: 220.0,
            engine_brake_constant: 30.0,
            engine_brake_per_rad: 0.30,
            idle_control_torque: 110.0,
        },
        transmission: TransmissionConfig {
            gear_ratios: vec![2.92, 2.06, 1.60, 1.31, 1.11, 0.96],
            reverse_ratio: 2.70,
            final_drive_ratio: 3.70,
            efficiency: 0.92,
            upshift_rpm: 7_450.0,
            downshift_rpm: 4_400.0,
            shift_time: 0.070,
            shift_cooldown: 0.30,
            clutch_capacity: 1_050.0,
            clutch_lock_speed: 4.5,
        },
        drivetrain: Drivetrain::Rwd,
        front_torque_bias: 0.0,
        differential_locking: 0.62,
        brakes: BrakeConfig {
            front_torque: MUSTANG_FRONT_BRAKE_TORQUE,
            rear_torque: MUSTANG_REAR_BRAKE_TORQUE,
            handbrake_torque: MUSTANG_HANDBRAKE_TORQUE,
            abs_slip_target: 0.12,
            abs_gain: 9.5,
            abs_enabled: true,
        },
        steering: SteeringConfig {
            // A keyboard key is an abrupt full-lock request.  The light,
            // rear-wheel-drive GT3 used to retain 15 degrees at 120 km/h and
            // 8 degrees at 200 km/h, which was enough to provoke snap
            // oversteer.  Keep a useful pit-lane lock, then taper the rack much
            // more aggressively than the Veyron as road speed rises.
            max_angle: deg(24.0),
            speed_curve: vec![
                (0.0, deg(24.0)),
                (kmh(40.0), deg(19.0)),
                (kmh(60.0), deg(15.0)),
                (kmh(100.0), deg(10.0)),
                (kmh(120.0), deg(7.5)),
                (kmh(160.0), deg(4.8)),
                (kmh(200.0), deg(3.2)),
                (kmh(240.0), deg(2.2)),
                (kmh(280.0), deg(1.5)),
            ],
            input_rate: 2.4,
            return_rate: 7.0,
            ackermann: true,
            steering_wheel_rotation: deg(190.0),
        },
        aero: AeroConfig {
            air_density: 1.225,
            drag_coefficient: 0.55,
            frontal_area: 2.10,
            lift_coefficient_front: 0.50,
            lift_coefficient_rear: 0.78,
            drag_center: Vec3::new(0.0, 0.46, 0.0),
            downforce_center_front: Vec3::new(0.0, 0.22, -1.52),
            downforce_center_rear: Vec3::new(0.0, 0.72, 1.48),
        },
        collider_boxes: vec![
            (Vec3::new(0.0, 0.39, 0.02), Vec3::new(0.99, 0.27, 2.42)),
            (Vec3::new(0.0, 0.84, -0.04), Vec3::new(0.72, 0.27, 0.92)),
        ],
        driver_eye: Vec3::new(-0.36, 1.03, -0.24),
        // Align the gameplay camera with the Mustang driver's configured eye
        // point. The shared cockpit mount including its +0.35 m adjustment is
        // (0.0, 0.96, 0.25), hence this vehicle-specific delta.
        cockpit_camera_offset: Vec3::new(-0.36, 0.07, -0.49),
        cockpit_camera_yaw_degrees: 0.0,
        steering_wheel_center: Vec3::new(-0.36, 0.79, -0.58),
        steering_wheel_axis: Vec3::new(0.0, 0.35, 0.94),
        solver_substeps: 4,
    }
}

// ---------------------------------------------------------------------------
// 2010 Mercedes-Benz SLS AMG
// ---------------------------------------------------------------------------

const SLS_FRONT_TIRE: TireConfig = TireConfig {
    radius: 0.349,
    width: 0.306,
    inertia: 1.72,
    peak_friction: 1.42,
    nominal_load: 3_850.0,
    load_sensitivity: 0.10,
    friction_clamp: (0.90, 1.78),
    longitudinal: MagicFormula {
        b: 11.8,
        c: 1.9,
        e: 0.97,
    },
    lateral: MagicFormula {
        b: 10.0,
        c: 1.46,
        e: -0.42,
    },
    relaxation_long: 0.32,
    relaxation_lat: 0.54,
    rolling_resistance: 0.013,
    rolling_resistance_speed: 0.000065,
};

const SLS_REAR_TIRE: TireConfig = TireConfig {
    radius: 0.360,
    width: 0.316,
    inertia: 1.92,
    peak_friction: 1.46,
    nominal_load: 4_150.0,
    ..SLS_FRONT_TIRE
};

const SLS_FRONT_SUSPENSION: SuspensionConfig = SuspensionConfig {
    rest_length: 0.300,
    max_compression: 0.095,
    max_droop: 0.085,
    spring_stiffness: 72_000.0,
    progressive_stiffness: 270_000.0,
    damper_compression: 4_400.0,
    damper_rebound: 6_300.0,
    bump_stop_stiffness: 650_000.0,
    anti_roll_stiffness: 34_000.0,
};

const SLS_REAR_SUSPENSION: SuspensionConfig = SuspensionConfig {
    spring_stiffness: 79_000.0,
    progressive_stiffness: 290_000.0,
    damper_compression: 4_800.0,
    damper_rebound: 6_800.0,
    anti_roll_stiffness: 30_000.0,
    ..SLS_FRONT_SUSPENSION
};

fn sls_wheel(id: WheelId) -> WheelConfig {
    let front = id.is_front();
    let sign = if id.is_left() { -1.0 } else { 1.0 };
    WheelConfig {
        id,
        anchor: Vec3::new(
            sign * if front { 0.848 } else { 0.846 },
            if front { 0.650 } else { 0.660 },
            if front { -1.405 } else { 1.405 },
        ),
        steerable: front,
        tire: if front { SLS_FRONT_TIRE } else { SLS_REAR_TIRE },
        suspension: if front {
            SLS_FRONT_SUSPENSION
        } else {
            SLS_REAR_SUSPENSION
        },
        brake_torque: if front { 5_300.0 } else { 3_500.0 },
        handbrake_torque: if front { 0.0 } else { 4_800.0 },
    }
}

/// Production SLS AMG with the 420 kW / 650 N·m M159 V8, rear transaxle,
/// seven-speed dual-clutch transmission and official 47:53 weight balance.
pub fn mercedes_benz_sls_amg() -> VehicleConfig {
    VehicleConfig {
        name: "Mercedes-Benz SLS AMG",
        mass: 1_620.0,
        inertia: Vec3::new(2_300.0, 2_700.0, 650.0),
        center_of_mass: Vec3::new(0.0, 0.435, 0.084),
        wheel_base: 2.810,
        track_width_front: 1.696,
        track_width_rear: 1.692,
        wheels: [
            sls_wheel(WheelId::FrontLeft),
            sls_wheel(WheelId::FrontRight),
            sls_wheel(WheelId::RearLeft),
            sls_wheel(WheelId::RearRight),
        ],
        engine: EngineConfig {
            torque_curve: TorqueCurve::new(vec![
                (800.0, 250.0),
                (1_500.0, 390.0),
                (2_500.0, 520.0),
                (3_500.0, 610.0),
                (4_750.0, 650.0),
                (5_500.0, 645.0),
                (6_200.0, 625.0),
                (6_800.0, 590.0),
                (7_200.0, 525.0),
            ]),
            inertia: 0.31,
            idle_rpm: 800.0,
            redline_rpm: 7_100.0,
            limiter_rpm: 7_250.0,
            limiter_hysteresis_rpm: 240.0,
            engine_brake_constant: 30.0,
            engine_brake_per_rad: 0.31,
            idle_control_torque: 125.0,
        },
        transmission: TransmissionConfig {
            gear_ratios: vec![3.40, 2.19, 1.63, 1.29, 1.03, 0.84, 0.67],
            reverse_ratio: 2.79,
            final_drive_ratio: 3.67,
            efficiency: 0.92,
            upshift_rpm: 7_000.0,
            downshift_rpm: 3_100.0,
            shift_time: 0.10,
            shift_cooldown: 0.42,
            clutch_capacity: 1_050.0,
            clutch_lock_speed: 4.0,
        },
        drivetrain: Drivetrain::Rwd,
        front_torque_bias: 0.0,
        differential_locking: 0.52,
        brakes: BrakeConfig {
            front_torque: 5_300.0,
            rear_torque: 3_500.0,
            handbrake_torque: 4_800.0,
            abs_slip_target: 0.14,
            abs_gain: 8.5,
            abs_enabled: true,
        },
        steering: SteeringConfig {
            max_angle: deg(34.0),
            speed_curve: vec![
                (0.0, deg(34.0)),
                (kmh(60.0), deg(22.0)),
                (kmh(120.0), deg(11.0)),
                (kmh(200.0), deg(5.0)),
                (kmh(300.0), deg(2.3)),
                (kmh(360.0), deg(1.7)),
            ],
            input_rate: 3.0,
            return_rate: 6.8,
            ackermann: true,
            steering_wheel_rotation: deg(230.0),
        },
        aero: AeroConfig {
            air_density: 1.225,
            drag_coefficient: 0.36,
            frontal_area: 2.05,
            lift_coefficient_front: 0.10,
            lift_coefficient_rear: 0.16,
            drag_center: Vec3::new(0.0, 0.54, -0.10),
            downforce_center_front: Vec3::new(0.0, 0.30, -1.48),
            downforce_center_rear: Vec3::new(0.0, 0.62, 1.45),
        },
        collider_boxes: vec![
            (Vec3::new(0.0, 0.39, 0.00), Vec3::new(1.00, 0.29, 2.31)),
            (Vec3::new(0.0, 0.93, 0.42), Vec3::new(0.77, 0.27, 0.92)),
        ],
        driver_eye: Vec3::new(-0.414, 1.08, 0.42),
        // Keep the centred Bugatti composition, but move 62 cm farther back so
        // the complete SLS wheel, binnacle and centre console remain in frame.
        cockpit_camera_offset: Vec3::new(0.0, 0.0, 0.62),
        cockpit_camera_yaw_degrees: 0.0,
        steering_wheel_center: Vec3::new(-0.414, 0.80, 0.105),
        steering_wheel_axis: Vec3::new(0.0, 0.32, 0.95),
        solver_substeps: 4,
    }
}

// ---------------------------------------------------------------------------
// 2009 Pagani Zonda Cinque
// ---------------------------------------------------------------------------

const ZONDA_FRONT_TIRE: TireConfig = TireConfig {
    radius: 0.336,
    // Factory Pirelli P Zero 255/35 R19.
    width: 0.255,
    inertia: 1.42,
    peak_friction: 1.60,
    nominal_load: 2_800.0,
    load_sensitivity: 0.09,
    friction_clamp: (1.00, 1.95),
    longitudinal: MagicFormula {
        b: 12.3,
        c: 1.9,
        e: 0.96,
    },
    lateral: MagicFormula {
        b: 10.9,
        c: 1.48,
        e: -0.35,
    },
    relaxation_long: 0.27,
    relaxation_lat: 0.45,
    rolling_resistance: 0.014,
    rolling_resistance_speed: 0.000065,
};

const ZONDA_REAR_TIRE: TireConfig = TireConfig {
    radius: 0.351,
    // Factory Pirelli P Zero 335/30 R20. The former mesh-bounds value of
    // 393 mm incorrectly made its skid mark wider than the Veyron's 365 mm
    // Michelin, even though the actual Pagani tyre is narrower.
    width: 0.335,
    inertia: 1.82,
    peak_friction: 1.64,
    nominal_load: 3_150.0,
    ..ZONDA_FRONT_TIRE
};

const ZONDA_FRONT_SUSPENSION: SuspensionConfig = SuspensionConfig {
    rest_length: 0.280,
    max_compression: 0.080,
    max_droop: 0.072,
    spring_stiffness: 67_000.0,
    progressive_stiffness: 320_000.0,
    damper_compression: 4_700.0,
    damper_rebound: 6_600.0,
    bump_stop_stiffness: 780_000.0,
    anti_roll_stiffness: 40_000.0,
};

const ZONDA_REAR_SUSPENSION: SuspensionConfig = SuspensionConfig {
    spring_stiffness: 76_000.0,
    progressive_stiffness: 350_000.0,
    damper_compression: 5_100.0,
    damper_rebound: 7_200.0,
    anti_roll_stiffness: 35_000.0,
    ..ZONDA_FRONT_SUSPENSION
};

fn zonda_wheel(id: WheelId) -> WheelConfig {
    let front = id.is_front();
    let sign = if id.is_left() { -1.0 } else { 1.0 };
    WheelConfig {
        id,
        anchor: Vec3::new(
            sign * 0.848,
            if front { 0.641 } else { 0.634 },
            if front { -1.360 } else { 1.360 },
        ),
        steerable: front,
        tire: if front {
            ZONDA_FRONT_TIRE
        } else {
            ZONDA_REAR_TIRE
        },
        suspension: if front {
            ZONDA_FRONT_SUSPENSION
        } else {
            ZONDA_REAR_SUSPENSION
        },
        brake_torque: if front { 5_700.0 } else { 3_900.0 },
        handbrake_torque: if front { 0.0 } else { 4_800.0 },
    }
}

/// Road-going Zonda Cinque: 678 CV / 780 N·m naturally aspirated AMG V12,
/// six-speed Cima sequential transmission, 1,210 kg dry mass and the factory's
/// quoted 750 kg of aerodynamic load beyond 300 km/h.
pub fn pagani_zonda_cinque() -> VehicleConfig {
    VehicleConfig {
        name: "Pagani Zonda Cinque",
        mass: 1_210.0,
        inertia: Vec3::new(1_500.0, 1_800.0, 420.0),
        center_of_mass: Vec3::new(0.0, 0.365, 0.082),
        wheel_base: 2.720,
        track_width_front: 1.696,
        track_width_rear: 1.696,
        wheels: [
            zonda_wheel(WheelId::FrontLeft),
            zonda_wheel(WheelId::FrontRight),
            zonda_wheel(WheelId::RearLeft),
            zonda_wheel(WheelId::RearRight),
        ],
        engine: EngineConfig {
            torque_curve: TorqueCurve::new(vec![
                (900.0, 260.0),
                (1_800.0, 430.0),
                (2_800.0, 620.0),
                (4_000.0, 780.0),
                (5_000.0, 790.0),
                (6_000.0, 778.0),
                (6_400.0, 744.0),
                (7_000.0, 660.0),
                (7_500.0, 560.0),
            ]),
            inertia: 0.27,
            idle_rpm: 900.0,
            redline_rpm: 7_300.0,
            limiter_rpm: 7_500.0,
            limiter_hysteresis_rpm: 240.0,
            engine_brake_constant: 29.0,
            engine_brake_per_rad: 0.28,
            idle_control_torque: 135.0,
        },
        transmission: TransmissionConfig {
            gear_ratios: vec![2.69, 1.86, 1.42, 1.13, 0.92, 0.72],
            reverse_ratio: 2.55,
            final_drive_ratio: 3.73,
            efficiency: 0.92,
            upshift_rpm: 7_250.0,
            downshift_rpm: 3_600.0,
            shift_time: 0.08,
            shift_cooldown: 0.30,
            clutch_capacity: 1_250.0,
            clutch_lock_speed: 4.8,
        },
        drivetrain: Drivetrain::Rwd,
        front_torque_bias: 0.0,
        differential_locking: 0.60,
        brakes: BrakeConfig {
            front_torque: 5_700.0,
            rear_torque: 3_900.0,
            handbrake_torque: 4_800.0,
            abs_slip_target: 0.12,
            abs_gain: 9.5,
            abs_enabled: true,
        },
        steering: SteeringConfig {
            max_angle: deg(31.0),
            speed_curve: vec![
                (0.0, deg(31.0)),
                (kmh(60.0), deg(20.0)),
                (kmh(120.0), deg(9.0)),
                (kmh(200.0), deg(4.2)),
                (kmh(300.0), deg(2.2)),
                (kmh(360.0), deg(1.6)),
            ],
            input_rate: 2.8,
            return_rate: 7.2,
            ackermann: true,
            steering_wheel_rotation: deg(210.0),
        },
        aero: AeroConfig {
            air_density: 1.225,
            drag_coefficient: 0.45,
            frontal_area: 1.95,
            lift_coefficient_front: 0.42,
            lift_coefficient_rear: 0.52,
            drag_center: Vec3::new(0.0, 0.47, 0.0),
            downforce_center_front: Vec3::new(0.0, 0.22, -1.43),
            downforce_center_rear: Vec3::new(0.0, 0.70, 1.42),
        },
        collider_boxes: vec![
            (Vec3::new(0.0, 0.34, 0.02), Vec3::new(0.97, 0.27, 2.08)),
            (Vec3::new(0.0, 0.82, 0.38), Vec3::new(0.72, 0.25, 0.82)),
        ],
        driver_eye: Vec3::new(-0.315, 1.00, -0.08),
        // Match the centred, slightly pulled-back Bugatti cockpit view.
        cockpit_camera_offset: Vec3::ZERO,
        cockpit_camera_yaw_degrees: 0.0,
        steering_wheel_center: Vec3::new(-0.315, 0.695, -0.647),
        steering_wheel_axis: Vec3::new(0.0, 0.34, 0.94),
        solver_substeps: 5,
    }
}

// ---------------------------------------------------------------------------
// Additional production cars supplied as glTF archives. These share the same
// high-fidelity tyre/suspension model as the Zonda, while every powertrain,
// mass distribution, geometry, gearing and aero package is configured below.

#[allow(clippy::too_many_arguments)]
fn road_car(
    name: &'static str,
    mass: f32,
    power_ps: f32,
    peak_torque: f32,
    redline: f32,
    gears: &[f32],
    final_drive: f32,
    drivetrain: Drivetrain,
    wheel_base: f32,
    front_track: f32,
    rear_track: f32,
    front_tire: (f32, f32),
    rear_tire: (f32, f32),
    drag: f32,
    area: f32,
    downforce: (f32, f32),
) -> VehicleConfig {
    let mut config = pagani_zonda_cinque();
    config.name = name;
    config.mass = mass;
    config.inertia = Vec3::new(mass * 1.15, mass * 1.42, mass * 0.34);
    config.center_of_mass = Vec3::new(0.0, 0.40, wheel_base * 0.02);
    config.wheel_base = wheel_base;
    config.track_width_front = front_track;
    config.track_width_rear = rear_track;
    let target_power_w = power_ps * 735.5;
    let peak_power_torque = target_power_w / (redline * core::f32::consts::TAU / 60.0);
    config.engine = EngineConfig {
        torque_curve: TorqueCurve::new(vec![
            (800.0, peak_torque * 0.34),
            (1_500.0, peak_torque * 0.58),
            (2_500.0, peak_torque * 0.82),
            (3_500.0, peak_torque * 0.96),
            (4_500.0, peak_torque),
            (redline * 0.78, peak_torque.min(peak_power_torque * 1.18)),
            (redline, peak_power_torque),
            (redline + 250.0, peak_power_torque * 0.78),
        ]),
        inertia: if redline > 9_000.0 { 0.19 } else { 0.28 },
        idle_rpm: if redline > 9_000.0 { 1_100.0 } else { 850.0 },
        redline_rpm: redline,
        limiter_rpm: redline + 180.0,
        limiter_hysteresis_rpm: 230.0,
        engine_brake_constant: 28.0,
        engine_brake_per_rad: 0.29,
        idle_control_torque: peak_torque * 0.18,
    };
    config.transmission.gear_ratios = gears.to_vec();
    config.transmission.final_drive_ratio = final_drive;
    config.transmission.upshift_rpm = redline - 100.0;
    config.transmission.downshift_rpm = redline * 0.45;
    config.transmission.shift_time = if gears.len() >= 8 { 0.055 } else { 0.10 };
    config.transmission.clutch_capacity = peak_torque * 1.65;
    config.drivetrain = drivetrain;
    config.front_torque_bias = if drivetrain == Drivetrain::Awd {
        0.32
    } else {
        0.0
    };
    config.differential_locking = if drivetrain == Drivetrain::Rwd {
        0.58
    } else {
        0.44
    };
    for wheel in &mut config.wheels {
        let front = wheel.id.is_front();
        let sign = if wheel.id.is_left() { -1.0 } else { 1.0 };
        wheel.anchor.x = sign * if front { front_track } else { rear_track } * 0.5;
        wheel.anchor.z = if front {
            -wheel_base * 0.5
        } else {
            wheel_base * 0.5
        };
        let (radius, width) = if front { front_tire } else { rear_tire };
        wheel.tire.radius = radius;
        wheel.tire.width = width;
        wheel.tire.inertia = 1.4 + width * 1.8;
        wheel.tire.nominal_load = mass * 9.81 / 4.0;
        wheel.anchor.y = wheel.suspension.rest_length + radius;
    }
    config.aero.drag_coefficient = drag;
    config.aero.frontal_area = area;
    config.aero.lift_coefficient_front = downforce.0;
    config.aero.lift_coefficient_rear = downforce.1;
    config.aero.downforce_center_front.z = -wheel_base * 0.53;
    config.aero.downforce_center_rear.z = wheel_base * 0.53;
    config.collider_boxes = vec![
        (
            Vec3::new(0.0, 0.36, 0.0),
            Vec3::new(front_track * 0.58, 0.28, wheel_base * 0.82),
        ),
        (
            Vec3::new(0.0, 0.82, 0.24),
            Vec3::new(front_track * 0.43, 0.25, wheel_base * 0.34),
        ),
    ];
    config.driver_eye = Vec3::new(-0.34, 0.98, 0.08);
    config.cockpit_camera_offset = Vec3::ZERO;
    config.steering_wheel_center = Vec3::new(-0.34, 0.72, -0.42);
    config.steering_wheel_axis = Vec3::new(0.0, 0.34, 0.94);
    config
}

pub fn mercedes_amg_one() -> VehicleConfig {
    road_car(
        "Mercedes-AMG ONE",
        1_695.0,
        1_063.0,
        1_050.0,
        11_000.0,
        &[3.13, 2.10, 1.55, 1.24, 1.02, 0.84, 0.69],
        3.30,
        Drivetrain::Awd,
        2.720,
        1.720,
        1.700,
        (0.345, 0.285),
        (0.355, 0.335),
        0.35,
        1.92,
        (0.62, 0.88),
    )
}

pub fn lamborghini_aventador() -> VehicleConfig {
    let mut config = road_car(
        "Lamborghini Aventador LP700-4",
        1_575.0,
        700.0,
        690.0,
        8_350.0,
        &[3.91, 2.44, 1.81, 1.46, 1.19, 0.97, 0.84],
        2.87,
        Drivetrain::Awd,
        2.700,
        1.720,
        1.700,
        (0.344, 0.255),
        (0.355, 0.335),
        0.33,
        2.05,
        (0.18, 0.24),
    );
    // The low windscreen and deep dashboard need the eye slightly lower and
    // farther back than the common centred cockpit mount.
    config.cockpit_camera_offset = Vec3::new(0.0, -0.12, 0.15);
    // Its cabin is lower than the other imported road cars. Reuse that proven
    // cockpit height/depth on the driver's side for the wheel-aligned view.
    config.driver_eye = Vec3::new(-0.34, 0.84, 0.30);
    config
}

pub fn koenigsegg_jesko() -> VehicleConfig {
    road_car(
        "Koenigsegg Jesko",
        1_420.0,
        1_600.0,
        1_500.0,
        8_500.0,
        &[4.61, 3.22, 2.41, 1.90, 1.55, 1.29, 1.09, 0.93, 0.79],
        2.73,
        Drivetrain::Rwd,
        2.700,
        1.740,
        1.700,
        (0.345, 0.265),
        (0.358, 0.325),
        0.28,
        1.88,
        (0.72, 1.05),
    )
}

pub fn ferrari_enzo() -> VehicleConfig {
    road_car(
        "Ferrari Enzo",
        1_365.0,
        660.0,
        657.0,
        8_200.0,
        &[3.15, 2.18, 1.57, 1.19, 0.94, 0.76],
        4.10,
        Drivetrain::Rwd,
        2.650,
        1.660,
        1.650,
        (0.337, 0.245),
        (0.350, 0.345),
        0.36,
        1.98,
        (0.22, 0.34),
    )
}

pub fn lamborghini_countach() -> VehicleConfig {
    road_car(
        "Lamborghini Countach 5000 QV",
        1_490.0,
        455.0,
        500.0,
        7_500.0,
        &[2.23, 1.52, 1.15, 0.94, 0.78],
        4.09,
        Drivetrain::Rwd,
        2.450,
        1.590,
        1.610,
        (0.335, 0.225),
        (0.352, 0.345),
        0.42,
        1.95,
        (0.08, 0.12),
    )
}

pub fn lamborghini_revuelto() -> VehicleConfig {
    let mut config = road_car(
        "Lamborghini Revuelto",
        1_772.0,
        1_015.0,
        900.0,
        9_500.0,
        &[3.08, 2.19, 1.63, 1.29, 1.03, 0.84, 0.69, 0.58],
        3.23,
        Drivetrain::Awd,
        2.779,
        1.720,
        1.700,
        (0.350, 0.265),
        (0.360, 0.355),
        0.32,
        2.00,
        (0.34, 0.48),
    );
    // Keep the steering wheel and both digital screens in frame below the
    // Revuelto's unusually thick roof header.
    config.cockpit_camera_offset = Vec3::new(-0.32, -0.06, -0.35);
    config
}

pub fn ford_gt_2006() -> VehicleConfig {
    road_car(
        "2006 Ford GT",
        1_581.0,
        558.0,
        678.0,
        6_500.0,
        &[2.61, 1.71, 1.23, 0.94, 0.77, 0.63],
        3.36,
        Drivetrain::Rwd,
        2.710,
        1.620,
        1.660,
        (0.343, 0.235),
        (0.358, 0.315),
        0.35,
        1.98,
        (0.14, 0.20),
    )
}

pub fn ford_mustang_1965() -> VehicleConfig {
    road_car(
        "1965 Ford Mustang GT",
        1_430.0,
        271.0,
        423.0,
        5_800.0,
        &[2.78, 1.93, 1.36, 1.00],
        3.25,
        Drivetrain::Rwd,
        2.743,
        1.450,
        1.440,
        (0.335, 0.215),
        (0.340, 0.225),
        0.46,
        2.05,
        (0.02, 0.03),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn torque_curve_matches_published_figures() {
        let cfg = bugatti_veyron_super_sport();
        // 1500 N·m plateau.
        assert!((cfg.engine.torque_curve.torque_at(4000.0) - 1500.0).abs() < 1.0);
        let (power_w, rpm) = cfg.engine.torque_curve.peak_power();
        let ps = power_w / 735.5;
        assert!(
            (1150.0..=1260.0).contains(&ps),
            "peak power {ps:.0} PS at {rpm:.0} rpm is not ~1200 PS"
        );
        assert!((5800.0..=6800.0).contains(&rpm), "peak power rpm {rpm}");
    }

    #[test]
    fn geometry_matches_the_real_car() {
        let cfg = bugatti_veyron_super_sport();
        assert!((cfg.wheel_base - 2.707).abs() < 0.01, "wheelbase");
        assert!((cfg.track_width_front - 1.727).abs() < 0.02, "front track");
        assert!((cfg.track_width_rear - 1.615).abs() < 0.02, "rear track");
        let front_fraction = cfg.static_front_weight_fraction();
        assert!(
            (0.42..=0.48).contains(&front_fraction),
            "static front weight fraction {front_fraction:.3}"
        );
        // The centre of mass must sit below the geometric centre of the body.
        let body_centre_y = cfg.collider_boxes[0].0.y;
        assert!(cfg.center_of_mass.y <= body_centre_y + 0.05);
    }

    #[test]
    fn steering_angle_falls_off_with_speed() {
        let cfg = bugatti_veyron_super_sport();
        let low = cfg.steering.max_angle_at(0.0);
        let high = cfg.steering.max_angle_at(kmh(350.0));
        assert!(low > high * 8.0, "low {low} high {high}");
        assert!(high > 0.0);
    }

    #[test]
    fn mustang_gt3_is_a_complete_distinct_preset() {
        let cfg = ford_mustang_gt3();
        let (power_w, _) = cfg.engine.torque_curve.peak_power();
        let power_ps = power_w / 735.5;
        assert!((485.0..=510.0).contains(&power_ps), "{power_ps:.1} PS");
        assert_eq!(cfg.drivetrain, Drivetrain::Rwd);
        assert_eq!(cfg.transmission.gear_ratios.len(), 6);
        assert!((1_250.0..=1_350.0).contains(&cfg.mass));
        assert!(cfg.aero.lift_coefficient_rear > cfg.aero.lift_coefficient_front);
        assert!(
            cfg.wheels
                .iter()
                .all(|wheel| wheel.tire.peak_friction > 1.7)
        );
    }

    #[test]
    fn mustang_uses_its_own_reduced_steering_curve() {
        let mustang = ford_mustang_gt3();
        let veyron = bugatti_veyron_super_sport();

        for speed_kmh in [0.0, 60.0, 120.0, 200.0, 280.0] {
            let speed = kmh(speed_kmh);
            assert!(
                mustang.steering.max_angle_at(speed) < veyron.steering.max_angle_at(speed),
                "Mustang steering is not calmer than Veyron at {speed_kmh} km/h"
            );
        }
        assert!(mustang.steering.input_rate < veyron.steering.input_rate);
        assert_eq!(mustang.steering.return_rate, 7.0);
    }

    #[test]
    fn remaining_added_cars_have_distinct_complete_powertrains() {
        let sls = mercedes_benz_sls_amg();
        let zonda = pagani_zonda_cinque();

        assert_eq!(sls.transmission.gear_ratios.len(), 7);
        assert_eq!(zonda.transmission.gear_ratios.len(), 6);
        assert_eq!(sls.drivetrain, Drivetrain::Rwd);
        assert_eq!(zonda.drivetrain, Drivetrain::Rwd);
        assert!((sls.mass - 1_620.0).abs() < 1.0);
        assert!((zonda.mass - 1_210.0).abs() < 1.0);

        let sls_ps = sls.engine.torque_curve.peak_power().0 / 735.5;
        let zonda_ps = zonda.engine.torque_curve.peak_power().0 / 735.5;
        assert!((555.0..=590.0).contains(&sls_ps), "SLS: {sls_ps:.1} PS");
        assert!(
            (660.0..=700.0).contains(&zonda_ps),
            "Zonda: {zonda_ps:.1} PS"
        );
    }

    #[test]
    fn mustang_camera_is_mounted_at_its_driver_eye() {
        let shared_eye = Vec3::new(0.0, 0.96, 0.25);
        let mustang = ford_mustang_gt3();
        assert!(
            (shared_eye + mustang.cockpit_camera_offset).distance(mustang.driver_eye) < 1.0e-5,
            "Mustang cockpit mount does not match driver eye"
        );
    }

    #[test]
    fn sls_camera_is_farther_back_while_zonda_keeps_the_bugatti_view() {
        let veyron = bugatti_veyron_super_sport();
        let sls = mercedes_benz_sls_amg();
        let zonda = pagani_zonda_cinque();

        assert_eq!(veyron.cockpit_camera_offset, Vec3::ZERO);
        assert!((sls.cockpit_camera_offset.z - 0.62).abs() < f32::EPSILON);
        assert_eq!(sls.cockpit_camera_offset.x, 0.0);
        assert_eq!(sls.cockpit_camera_offset.y, 0.0);
        assert_eq!(zonda.cockpit_camera_offset, veyron.cockpit_camera_offset);
        assert_eq!(sls.cockpit_camera_yaw_degrees, 0.0);
        assert_eq!(zonda.cockpit_camera_yaw_degrees, 0.0);
    }
}
