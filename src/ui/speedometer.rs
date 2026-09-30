//! The main heads-up display: speed, gear, engine speed and camera mode.

use bevy::prelude::*;
use bevy::text::FontSize;

use crate::audio::radio::RadioPlaylist;
use crate::camera::controller::GameCamera;
use crate::vehicle::physics::{Vehicle, VehicleTag};

/// Colour of the tachometer bar below the redline.
const RPM_BAR_COLOR: Color = Color::srgb(0.25, 0.75, 1.0);
/// Colour once the engine is in the last tenth before the limiter.
const RPM_BAR_REDLINE_COLOR: Color = Color::srgb(1.0, 0.25, 0.2);
const PANEL_BACKGROUND: Color = Color::srgba(0.02, 0.03, 0.05, 0.55);
const LABEL_COLOR: Color = Color::srgb(0.62, 0.68, 0.76);
const VALUE_COLOR: Color = Color::srgb(0.96, 0.97, 1.0);

#[derive(Component)]
pub struct HudRoot;

/// Which reading a piece of HUD text shows. One component and one query beats
/// five marker types and five mutually-exclusive queries.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum HudField {
    Speed,
    Gear,
    Rpm,
    Camera,
    Radio,
    Status,
}

#[derive(Component)]
pub struct RpmBarFill;

fn label(text: &str) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(13.0),
            ..default()
        },
        TextColor(LABEL_COLOR),
    )
}

fn value(text: &str, size: f32) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(VALUE_COLOR),
    )
}

pub fn spawn_hud(mut commands: Commands) {
    commands
        .spawn((
            Name::new("Hud"),
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                left: px(24),
                bottom: px(24),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                padding: UiRect::all(px(16)),
                border_radius: BorderRadius::all(px(10)),
                ..default()
            },
            BackgroundColor(PANEL_BACKGROUND),
        ))
        .with_children(|panel| {
            panel.spawn(label("SPEED"));
            panel.spawn((value("0 km/h", 46.0), HudField::Speed));

            // Tachometer bar.
            panel
                .spawn((
                    Node {
                        width: px(280),
                        height: px(10),
                        margin: UiRect::vertical(px(8)),
                        border_radius: BorderRadius::all(px(5)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.12)),
                ))
                .with_child((
                    Node {
                        width: percent(0.0),
                        height: percent(100.0),
                        border_radius: BorderRadius::all(px(5)),
                        ..default()
                    },
                    BackgroundColor(RPM_BAR_COLOR),
                    RpmBarFill,
                ));

            panel
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(28),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_children(|column| {
                        column.spawn(label("GEAR"));
                        column.spawn((value("1", 26.0), HudField::Gear));
                    });

                    row.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_children(|column| {
                        column.spawn(label("RPM"));
                        column.spawn((value("0", 26.0), HudField::Rpm));
                    });

                    row.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_children(|column| {
                        column.spawn(label("CAMERA"));
                        column.spawn((value("CHASE CLOSE", 20.0), HudField::Camera));
                    });
                });

            panel.spawn((value("RADIO  —", 14.0), HudField::Radio));
            panel.spawn((value("", 15.0), HudField::Status));
        });
}

/// Refresh the HUD from the car and the camera.
pub fn update_hud(
    vehicles: Query<&Vehicle, With<VehicleTag>>,
    cameras: Query<&GameCamera>,
    radio: Option<Res<RadioPlaylist>>,
    mut fields: Query<(&HudField, &mut Text)>,
    mut rpm_bar: Query<(&mut Node, &mut BackgroundColor), With<RpmBarFill>>,
) {
    let Ok(vehicle) = vehicles.single() else {
        return;
    };

    let speed_kmh = vehicle.speed_kmh();
    let engine_rpm = vehicle.dynamics.rpm();
    let redline = vehicle.config.engine.redline_rpm;
    let limiter = vehicle.config.engine.limiter_rpm;

    for (field, mut text) in &mut fields {
        let value = match field {
            HudField::Speed => format!("{:.0} km/h", speed_kmh.abs()),
            HudField::Gear => vehicle.dynamics.transmission.display(),
            HudField::Rpm => format_thousands(engine_rpm),
            HudField::Camera => cameras
                .single()
                .map(GameCamera::display_label)
                .unwrap_or_else(|_| "-".to_string()),
            HudField::Radio => radio
                .as_ref()
                .and_then(|radio| radio.current_title())
                .map(|title| format!("RADIO  {title}"))
                .unwrap_or_else(|| "RADIO  —".to_string()),
            HudField::Status => status_flags(vehicle),
        };
        if text.as_str() != value {
            **text = value;
        }
    }

    if let Ok((mut node, mut color)) = rpm_bar.single_mut() {
        let fraction = (engine_rpm / limiter.max(1.0)).clamp(0.0, 1.0);
        node.width = percent(fraction * 100.0);
        color.0 = if engine_rpm >= redline {
            RPM_BAR_REDLINE_COLOR
        } else {
            RPM_BAR_COLOR
        };
    }
}

/// The short status line under the readouts.
fn status_flags(vehicle: &Vehicle) -> String {
    let mut flags: Vec<&str> = Vec::new();
    if vehicle.dynamics.triple_power_enabled() {
        flags.push("POWER x3");
    }
    if vehicle.dynamics.transmission.is_shifting() {
        flags.push("SHIFT");
    }
    if vehicle.dynamics.wheels.iter().any(|w| w.abs_active) {
        flags.push("ABS");
    }
    if vehicle.dynamics.any_wheel_sliding() {
        flags.push("SLIP");
    }
    if vehicle.input.handbrake > 0.05 {
        flags.push("HANDBRAKE");
    }
    if vehicle.dynamics.engine.limiter_active {
        flags.push("LIMITER");
    }
    flags.join("  ")
}

/// `6400` becomes `6,400`, which is much easier to read at a glance.
pub fn format_thousands(value: f32) -> String {
    let rounded = value.max(0.0).round() as u64;
    let digits = rounded.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(character);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_separator() {
        assert_eq!(format_thousands(0.0), "0");
        assert_eq!(format_thousands(940.0), "940");
        assert_eq!(format_thousands(6400.4), "6,400");
        assert_eq!(format_thousands(12345.0), "12,345");
    }
}
