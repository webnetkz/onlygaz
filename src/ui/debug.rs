//! The `F3` telemetry overlay.
//!
//! Everything the brief asks to be observable while tuning the car is here:
//! frame rate, speed, engine and gearbox state, pedal positions, and the full
//! per-wheel picture - wheel speed, slip ratio, slip angle, suspension travel
//! and vertical load - plus the current aerodynamic forces.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::text::FontSize;

use crate::vehicle::config::WheelId;
use crate::vehicle::physics::{Vehicle, VehicleTag};
use crate::vehicle::telemetry::PerformanceLog;

/// Whether the overlay is shown. Toggled with `F3`.
#[derive(Resource, Default)]
pub struct DebugOverlay {
    pub visible: bool,
}

#[derive(Component)]
pub struct DebugText;

pub fn spawn_debug_overlay(mut commands: Commands) {
    commands.spawn((
        Name::new("DebugOverlay"),
        DebugText,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(13.0),
            ..default()
        },
        TextColor(Color::srgb(0.85, 0.93, 1.0)),
        Node {
            position_type: PositionType::Absolute,
            top: px(16),
            left: px(24),
            padding: UiRect::all(px(12)),
            border_radius: BorderRadius::all(px(8)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.02, 0.03, 0.05, 0.62)),
        Visibility::Hidden,
    ));
}

pub fn toggle_debug_overlay(
    overlay: Res<DebugOverlay>,
    mut query: Query<&mut Visibility, With<DebugText>>,
) {
    if !overlay.is_changed() {
        return;
    }
    for mut visibility in &mut query {
        *visibility = if overlay.visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

pub fn update_debug_overlay(
    overlay: Res<DebugOverlay>,
    diagnostics: Res<DiagnosticsStore>,
    performance: Res<PerformanceLog>,
    vehicles: Query<(&Transform, &Vehicle), With<VehicleTag>>,
    mut query: Query<&mut Text, With<DebugText>>,
) {
    if !overlay.visible {
        return;
    }
    let Ok((transform, vehicle)) = vehicles.single() else {
        return;
    };
    let Ok(mut text) = query.single_mut() else {
        return;
    };

    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);

    let dynamics = &vehicle.dynamics;
    let config = &vehicle.config;

    let mut lines = Vec::with_capacity(24);
    lines.push(format!(
        "FPS {fps:5.1}    dist {:7.1} m    x {:6.2} m",
        transform.translation.z, transform.translation.x
    ));
    lines.push(format!(
        "speed {:7.2} km/h ({:6.2} m/s)   gear {}   rpm {:6.0}",
        vehicle.speed_kmh(),
        dynamics.forward_speed,
        dynamics.transmission.display(),
        dynamics.rpm()
    ));
    lines.push(format!(
        "throttle {:.2}  brake {:.2}  steer {:+.2}  handbrake {:.2}  clutch {:.2}",
        vehicle.input.throttle,
        vehicle.input.brake,
        vehicle.input.steering,
        vehicle.input.handbrake,
        dynamics.clutch_engagement
    ));
    lines.push(format!(
        "engine torque {:7.1} Nm   axle torque {:8.1} Nm   power {:6.0} kW   mode x{:.0}",
        dynamics.engine_torque,
        dynamics.axle_torque,
        dynamics.engine.power(dynamics.engine_torque.max(0.0)) / 1000.0,
        dynamics.power_multiplier,
    ));
    lines.push(format!(
        "drag {:7.0} N   downforce {:7.0} N (F {:5.0} / R {:5.0})   long {:+.2} g   lat {:+.2} g",
        dynamics.aero.drag_force,
        dynamics.aero.total_downforce(),
        dynamics.aero.downforce_front,
        dynamics.aero.downforce_rear,
        performance.longitudinal_g,
        performance.lateral_g
    ));
    lines.push(String::new());
    lines.push(
        "wheel   rpm    v_wheel   slipR    slipA    Fz(N)    Fx(N)     Fy(N)   susp(mm)  brake"
            .to_string(),
    );

    for id in WheelId::ALL {
        let wheel = &dynamics.wheels[id.index()];
        let wheel_cfg = config.wheel(id);
        let wheel_rpm = wheel.angular_velocity * 60.0 / core::f32::consts::TAU;
        lines.push(format!(
            "{:>5}  {:6.0}  {:7.2}  {:+7.3}  {:+6.2}°  {:7.0}  {:+8.0}  {:+8.0}  {:+7.1}  {:6.0}{}",
            id.short_name(),
            wheel_rpm,
            wheel.surface_speed(wheel_cfg.tire.radius),
            wheel.tire.slip_ratio,
            wheel.tire.slip_angle().to_degrees(),
            wheel.normal_force,
            wheel.forces.longitudinal,
            wheel.forces.lateral,
            wheel.compression * 1000.0,
            wheel.brake_torque,
            if wheel.abs_active { " ABS" } else { "" }
        ));
    }

    lines.push(String::new());
    lines.push(performance.summary());
    let gear_line: Vec<String> = (1..=config.transmission.gear_ratios.len())
        .map(|gear| {
            format!(
                "g{gear} {:.0}rpm/{:.0}kmh",
                performance.gear_max_rpm[gear], performance.gear_max_speed[gear]
            )
        })
        .collect();
    lines.push(gear_line.join("  "));

    **text = lines.join("\n");
}
