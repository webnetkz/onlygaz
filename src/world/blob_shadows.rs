//! Soft "blob" shadows under the cars when shadow maps are off.
//!
//! Without any shadow a car seems to hover above the tarmac. The Lite preset
//! turns the sun's shadow maps off - re-drawing every car into two cascades is
//! far too expensive for a budget phone - and puts a soft dark patch on the
//! road under each car instead: one transparent quad per car.

use bevy::asset::RenderAssetUsages;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::lighting::LightingSettings;
use super::road::RoadSpec;

/// Height of the patch above the road surface [m]: above the lane markings.
const LIFT: f32 = 0.07;
/// Darkness at the centre of the patch.
const OPACITY: f32 = 0.7;
/// How much larger than the car's footprint the soft patch is: the body
/// hides the middle, so the visible part is the fringe around the car.
const SPREAD: f32 = 1.35;
/// Width of the soft edge, as a fraction of the patch's half size.
const FALLOFF: f32 = 0.42;
const TEXTURE_SIZE: u32 = 64;

/// A car that gets a blob shadow: half of its footprint (x = width,
/// y = length) in its own frame [m].
#[derive(Component, Debug, Clone, Copy)]
pub struct BlobShadowCaster {
    pub half_extents: Vec2,
}

/// The shadow patch of one [`BlobShadowCaster`].
#[derive(Component)]
pub struct BlobShadow {
    caster: Entity,
}

#[derive(Resource)]
struct BlobShadowAssets {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

pub struct BlobShadowPlugin;

impl Plugin for BlobShadowPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_blob_shadow_assets)
            .add_systems(Update, spawn_blob_shadows)
            .add_systems(
                PostUpdate,
                place_blob_shadows.before(TransformSystems::Propagate),
            );
    }
}

/// Opacity of the patch at `(u, v)` in `-1..=1` across the footprint: flat in
/// the middle, fading out towards a rounded-rectangle edge.
fn blob_alpha(u: f32, v: f32) -> f32 {
    // Superellipse: closer to the car's rectangular outline than a circle.
    let r = (u.abs().powi(4) + v.abs().powi(4)).powf(0.25);
    let t = ((1.0 - r) / FALLOFF).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn blob_texture() -> Image {
    let mut data = Vec::with_capacity((TEXTURE_SIZE * TEXTURE_SIZE * 4) as usize);
    for y in 0..TEXTURE_SIZE {
        for x in 0..TEXTURE_SIZE {
            let u = (x as f32 + 0.5) / TEXTURE_SIZE as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / TEXTURE_SIZE as f32 * 2.0 - 1.0;
            let alpha = (blob_alpha(u, v) * 255.0).round() as u8;
            data.extend_from_slice(&[0, 0, 0, alpha]);
        }
    }
    Image::new(
        Extent3d {
            width: TEXTURE_SIZE,
            height: TEXTURE_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

fn setup_blob_shadow_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let texture = images.add(blob_texture());
    commands.insert_resource(BlobShadowAssets {
        mesh: meshes.add(Plane3d::default().mesh().size(1.0, 1.0)),
        material: materials.add(StandardMaterial {
            base_color: Color::srgba(0.0, 0.0, 0.0, OPACITY),
            base_color_texture: Some(texture),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
    });
}

fn spawn_blob_shadows(
    mut commands: Commands,
    assets: Option<Res<BlobShadowAssets>>,
    settings: Res<LightingSettings>,
    casters: Query<Entity, Added<BlobShadowCaster>>,
) {
    let Some(assets) = assets else {
        return;
    };
    for caster in &casters {
        commands.spawn((
            Name::new("BlobShadow"),
            BlobShadow { caster },
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.material.clone()),
            NotShadowCaster,
            NotShadowReceiver,
            Transform::default(),
            if settings.uses_blob_shadows() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            },
        ));
    }
}

/// Follow each car along the road surface, turning with its heading only.
fn place_blob_shadows(
    mut commands: Commands,
    settings: Res<LightingSettings>,
    road: Res<RoadSpec>,
    casters: Query<(&Transform, &BlobShadowCaster), Without<BlobShadow>>,
    mut shadows: Query<(Entity, &BlobShadow, &mut Transform, &mut Visibility)>,
) {
    let wanted = if settings.uses_blob_shadows() {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for (entity, shadow, mut transform, mut visibility) in &mut shadows {
        let Ok((car, caster)) = casters.get(shadow.caster) else {
            commands.entity(entity).try_despawn();
            continue;
        };
        visibility.set_if_neq(wanted);
        if wanted == Visibility::Hidden {
            continue;
        }
        // Heading only: the patch stays flat on the road while the body
        // pitches and rolls.
        let forward = car.rotation * Vec3::Z;
        let yaw = forward.x.atan2(forward.z);
        let size = caster.half_extents * 2.0 * SPREAD;
        *transform =
            Transform::from_xyz(car.translation.x, road.surface_y + LIFT, car.translation.z)
                .with_rotation(Quat::from_rotation_y(yaw))
                .with_scale(Vec3::new(size.x, 1.0, size.y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_patch_is_solid_in_the_middle_and_fades_out_at_the_edge() {
        assert!((blob_alpha(0.0, 0.0) - 1.0).abs() < 1.0e-6);
        assert!(blob_alpha(0.3, 0.2) > 0.95);
        assert_eq!(blob_alpha(1.0, 0.0), 0.0);
        assert_eq!(blob_alpha(0.99, 0.99), 0.0);
        assert!(blob_alpha(0.7, 0.0) > blob_alpha(0.8, 0.0));
        // Still dark at the edge of the car's own footprint.
        assert!(blob_alpha(1.0 / SPREAD, 0.0) > 0.5);
    }

    #[test]
    fn the_texture_is_black_with_a_soft_alpha() {
        let image = blob_texture();
        let data = image.data.as_ref().unwrap();
        assert_eq!(data.len(), (TEXTURE_SIZE * TEXTURE_SIZE * 4) as usize);
        let centre = ((TEXTURE_SIZE / 2 * TEXTURE_SIZE + TEXTURE_SIZE / 2) * 4) as usize;
        assert_eq!(&data[centre..centre + 3], &[0, 0, 0]);
        assert!(data[centre + 3] > 250);
        assert_eq!(data[3], 0, "corner is transparent");
    }
}
