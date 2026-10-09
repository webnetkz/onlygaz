//! 3D scene below the screen's resolution, interface at full resolution.
//!
//! When [`LightingSettings::render_resolution`] asks for fewer pixels than the
//! window has, the game camera draws into an image of that size. A second,
//! interface-only camera then shows the image stretched over the whole screen
//! and draws the HUD, touch controls and menus on top at the screen's own
//! resolution, so text stays sharp. A phone's GPU spends most of its frame
//! shading 3D pixels: 540 lines instead of 1080 is a quarter of that work.
//!
//! At [`RenderResolution::Native`] (every desktop preset except Lite) nothing
//! of this exists and the game camera draws into the window as before.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraUpdateSystems, RenderTarget};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::window::{PrimaryWindow, WindowRef};

use super::controller::GameCamera;
use crate::world::lighting::{LightingSettings, RenderResolution};

/// Render layer of the presenting camera; nothing in the world uses it.
const PRESENTER_LAYER: usize = 31;

/// The reduced-resolution image the game camera draws into, and the entities
/// that show it.
#[derive(Resource, Default)]
pub struct SceneImage {
    image: Option<Handle<Image>>,
    size: UVec2,
    presenter: Option<Entity>,
    view: Option<Entity>,
}

impl SceneImage {
    /// Size of the reduced-resolution image, `None` while rendering natively.
    pub fn size(&self) -> Option<UVec2> {
        self.image.as_ref().map(|_| self.size)
    }
}

/// The interface-only camera that presents the scene image.
#[derive(Component)]
pub struct ScenePresenterCamera;

/// The full-screen interface node showing the scene image.
#[derive(Component)]
pub struct SceneImageView;

pub struct RenderScalePlugin;

impl Plugin for RenderScalePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SceneImage>().add_systems(
            PostUpdate,
            // Before the cameras compute their targets: the interface lays
            // itself out for its new camera in the same frame.
            sync_scene_resolution.before(CameraUpdateSystems),
        );
    }
}

/// Size of the scene image for `resolution` in a window of `window` physical
/// pixels, or `None` for native rendering.
pub fn scene_image_size(resolution: RenderResolution, window: UVec2) -> Option<UVec2> {
    resolution.scaled_size(window)
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sync_scene_resolution(
    mut commands: Commands,
    settings: Res<LightingSettings>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut scene: ResMut<SceneImage>,
    game_cameras: Query<(Entity, Option<&RenderTarget>, Has<IsDefaultUiCamera>), With<GameCamera>>,
    mut views: Query<&mut ImageNode, With<SceneImageView>>,
) {
    let Ok((camera, target, is_ui_camera)) = game_cameras.single() else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    let window_size = UVec2::new(window.physical_width(), window.physical_height());
    let wanted = scene_image_size(settings.render_resolution, window_size);

    let Some(size) = wanted else {
        // Native: draw straight into the window and host the interface again.
        if scene.image.is_some() || !is_ui_camera {
            commands
                .entity(camera)
                .insert((RenderTarget::Window(WindowRef::Primary), IsDefaultUiCamera));
            for entity in [scene.presenter.take(), scene.view.take()]
                .into_iter()
                .flatten()
            {
                commands.entity(entity).try_despawn();
            }
            scene.image = None;
            scene.size = UVec2::ZERO;
            info!("3D scene renders at the window's native resolution");
        }
        return;
    };

    if scene.image.is_none() || scene.size != size {
        let image = images.add(Image::new_target_texture(
            size.x,
            size.y,
            // The format Bevy renders colour targets in.
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
        if let Some(old) = scene.image.replace(image.clone())
            && old != image
        {
            images.remove(&old);
        }
        scene.size = size;
        for mut view in &mut views {
            view.image = image.clone();
        }
        info!(
            "3D scene renders at {}x{} for a {}x{} window",
            size.x, size.y, window_size.x, window_size.y
        );
    }
    let Some(image) = scene.image.clone() else {
        return;
    };

    let targets_image = matches!(
        target,
        Some(RenderTarget::Image(current)) if current.handle == image
    );
    if !targets_image || is_ui_camera {
        commands
            .entity(camera)
            .insert(RenderTarget::Image(image.clone().into()))
            .remove::<IsDefaultUiCamera>();
    }

    if scene.presenter.is_none() {
        let presenter = commands
            .spawn((
                Name::new("ScenePresenterCamera"),
                ScenePresenterCamera,
                Camera2d,
                Camera {
                    // After the game camera (order 0) has filled the image.
                    order: 1,
                    clear_color: ClearColorConfig::Custom(Color::BLACK),
                    ..default()
                },
                // The image is already tone mapped and dithered.
                Tonemapping::None,
                DebandDither::Disabled,
                Msaa::Off,
                IsDefaultUiCamera,
                // Sees no 3D entity at all (the interface ignores layers), so
                // visibility checks never consider the world for this camera.
                RenderLayers::layer(PRESENTER_LAYER),
            ))
            .id();
        scene.presenter = Some(presenter);
    }
    if scene.view.is_none() {
        let view = commands
            .spawn((
                Name::new("SceneImageView"),
                SceneImageView,
                ImageNode::new(image),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                // Below every other interface element, and never a touch target.
                GlobalZIndex(i32::MIN),
                bevy::picking::Pickable::IGNORE,
            ))
            .id();
        scene.view = Some(view);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phone_screens_render_the_scene_at_the_selected_height() {
        let phone = UVec2::new(2400, 1080);
        assert_eq!(
            scene_image_size(RenderResolution::P540, phone),
            Some(UVec2::new(1200, 540))
        );
        assert_eq!(
            scene_image_size(RenderResolution::P720, phone),
            Some(UVec2::new(1600, 720))
        );
        assert_eq!(scene_image_size(RenderResolution::Native, phone), None);
    }

    #[test]
    fn small_screens_render_natively() {
        let hd_phone = UVec2::new(1600, 720);
        assert_eq!(scene_image_size(RenderResolution::P720, hd_phone), None);
        assert_eq!(
            scene_image_size(RenderResolution::P540, hd_phone),
            Some(UVec2::new(1200, 540))
        );
        assert_eq!(scene_image_size(RenderResolution::P540, UVec2::ZERO), None);
    }
}
