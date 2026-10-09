//! Lights that cannot add anything are taken out of the renderer.
//!
//! Many lamps sit at zero intensity most of the time: the street lights and
//! headlights by day, the brake-lamp glow while not braking, the cabin fill
//! outside the cockpit view. A zero-intensity light still goes through light
//! clustering on the CPU and is still evaluated by every pixel it covers on
//! the GPU. Hiding it removes both costs; it comes back the moment its
//! intensity rises again.

use bevy::camera::visibility::VisibilitySystems;
use bevy::prelude::*;

/// Marks a light that this module hid, so it never un-hides a light that
/// other game code hid on purpose.
#[derive(Component)]
pub struct HiddenWhileDark;

pub struct LightCullingPlugin;

impl Plugin for LightCullingPlugin {
    fn build(&self, app: &mut App) {
        // Late in the frame, after the systems that set light intensities.
        app.add_systems(
            PostUpdate,
            hide_dark_lights.before(VisibilitySystems::VisibilityPropagate),
        );
    }
}

#[allow(clippy::type_complexity)]
fn hide_dark_lights(
    mut commands: Commands,
    mut lights: Query<
        (
            Entity,
            Option<&PointLight>,
            Option<&SpotLight>,
            &mut Visibility,
            Has<HiddenWhileDark>,
        ),
        Or<(With<PointLight>, With<SpotLight>)>,
    >,
) {
    for (entity, point, spot, mut visibility, hidden_here) in &mut lights {
        let intensity = point
            .map(|light| light.intensity)
            .or(spot.map(|light| light.intensity))
            .unwrap_or_default();
        if intensity <= 0.0 {
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
                commands.entity(entity).try_insert(HiddenWhileDark);
            }
        } else if hidden_here {
            if *visibility == Visibility::Hidden {
                *visibility = Visibility::Inherited;
            }
            commands.entity(entity).try_remove::<HiddenWhileDark>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_lights_are_hidden_until_they_shine_again() {
        let mut app = App::new();
        app.add_systems(Update, hide_dark_lights);
        let lamp = app
            .world_mut()
            .spawn((
                SpotLight {
                    intensity: 0.0,
                    ..default()
                },
                Visibility::Inherited,
            ))
            .id();
        let deliberately_hidden = app
            .world_mut()
            .spawn((
                PointLight {
                    intensity: 1_000.0,
                    ..default()
                },
                Visibility::Hidden,
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(lamp),
            Some(&Visibility::Hidden)
        );
        assert!(app.world().get::<HiddenWhileDark>(lamp).is_some());

        app.world_mut()
            .get_mut::<SpotLight>(lamp)
            .unwrap()
            .intensity = 5_000.0;
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(lamp),
            Some(&Visibility::Inherited)
        );
        assert!(app.world().get::<HiddenWhileDark>(lamp).is_none());

        // Hidden by other game code: left alone.
        assert_eq!(
            app.world().get::<Visibility>(deliberately_hidden),
            Some(&Visibility::Hidden)
        );
    }
}
