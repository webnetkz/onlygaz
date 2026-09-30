//! Live left/right virtual rear-view mirrors, rendered only during a race.
use super::controller::GameCamera;
use crate::game::GameState;
use crate::vehicle::VehicleKind;
use crate::vehicle::damage::{BreakablePart, DamagePartKind};
use crate::vehicle::model_rig::SceneNodes;
use crate::vehicle::physics::Vehicle;
use crate::vehicle::physics::VehicleModelRoot;
use crate::world::lighting::LightingSettings;
use bevy::camera::RenderTarget;
use bevy::light::Skybox;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::world_serialization::WorldInstanceReady;

pub struct MirrorPlugin;
#[derive(Component)]
struct MirrorCamera(f32);
#[derive(Component)]
struct RealMirrorSurface {
    vehicle: Entity,
    side: Option<usize>,
}

#[derive(Resource, Clone)]
struct MirrorTextures([Handle<Image>; 2]);

fn mirror_side(kind: VehicleKind, name: &str) -> Option<usize> {
    let lower = name.to_ascii_lowercase();
    match kind {
        VehicleKind::Veyron => {
            if lower.contains("mirrorleft__mirror") {
                Some(0)
            } else if lower.contains("mirrorright__mirror") {
                Some(1)
            } else if lower.contains("interior_mirrormiddle__mirror") {
                // The centre mirror uses the driver's (left) rear camera.
                Some(0)
            } else {
                None
            }
        }
        VehicleKind::MercedesSlsAmg => lower.contains("mirrors_8").then_some(0),
        VehicleKind::PaganiZondaCinque => {
            if lower.contains("mirrorleft") {
                Some(0)
            } else if lower.contains("mirrorright") {
                Some(1)
            } else {
                None
            }
        }
        VehicleKind::MustangGt3 => None,
        _ => {
            if lower.contains("mirrorleft") || lower.contains("mirror_l") {
                Some(0)
            } else if lower.contains("mirrorright") || lower.contains("mirror_r") {
                Some(1)
            } else if lower.contains("rear mirror") || lower.contains("mirror_mirror") {
                Some(0)
            } else {
                None
            }
        }
    }
}

/// The supplied Veyron mirror meshes already use the render target's vertical
/// texture origin. Applying the generic Y correction a second time turns their
/// live image upside down. Horizontal reversal stays enabled for a real mirror.
fn mirror_texture_flips(kind: VehicleKind) -> (bool, bool) {
    (true, kind != VehicleKind::Veyron)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mirrors_follow_the_car_and_disable_their_render_cameras() {
        let mut app = App::new();
        let mut graphics = LightingSettings::default();
        graphics.apply_preset(crate::world::lighting::LightingQuality::Cinematic);
        app.init_resource::<Assets<Image>>()
            .insert_resource(State::new(GameState::Driving))
            .insert_resource(graphics)
            .add_systems(Startup, spawn_mirrors)
            .add_systems(Update, update_mirrors);
        let pose = Transform::from_xyz(10.0, 0.0, 50.0);
        let car = app
            .world_mut()
            .spawn((
                pose,
                Vehicle::new(crate::vehicle::VehicleKind::Veyron.config(), pose),
            ))
            .id();
        app.world_mut().spawn((
            GameCamera::new(car),
            Skybox::default(),
            bevy::camera::Exposure::default(),
        ));
        app.update();
        let world = app.world_mut();
        let mut query = world.query_filtered::<(&Camera, &Transform), With<MirrorCamera>>();
        assert_eq!(query.iter(world).count(), 2);
        for (camera, transform) in query.iter(world) {
            assert!(camera.is_active);
            assert!(transform.forward().dot(Vec3::NEG_Z) > 0.9);
            assert!((transform.translation.z - 50.0).abs() < 0.01);
        }
        world
            .resource_mut::<LightingSettings>()
            .apply_preset(crate::world::lighting::LightingQuality::Performance);
        app.update();
        let world = app.world_mut();
        let mut query = world.query_filtered::<&Camera, With<MirrorCamera>>();
        assert!(query.iter(world).all(|camera| !camera.is_active));
        assert_eq!(world.query::<&RealMirrorSurface>().iter(world).count(), 0);
    }

    #[test]
    fn veyron_mirrors_are_horizontal_but_never_upside_down() {
        assert_eq!(mirror_texture_flips(VehicleKind::Veyron), (true, false));
        assert_eq!(
            mirror_texture_flips(VehicleKind::MercedesSlsAmg),
            (true, true)
        );
    }
}

impl Plugin for MirrorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_mirrors)
            .add_observer(wire_real_mirrors)
            .add_systems(
                PostUpdate,
                (update_mirrors, set_real_mirror_enabled).before(TransformSystems::Propagate),
            );
    }
}

fn spawn_mirrors(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut textures = Vec::with_capacity(2);
    for side in [-1.0, 1.0] {
        let texture = images.add(Image::new_target_texture(
            384,
            160,
            TextureFormat::Rgba8Unorm,
            Some(TextureFormat::Rgba8UnormSrgb),
        ));
        commands.spawn((
            Name::new("Rear-view mirror camera"),
            MirrorCamera(side),
            Camera3d::default(),
            Camera {
                order: -1,
                is_active: false,
                ..default()
            },
            RenderTarget::Image(texture.clone().into()),
            Projection::Perspective(PerspectiveProjection {
                fov: 55.0_f32.to_radians(),
                near: 0.1,
                far: 1500.0,
                ..default()
            }),
            Msaa::Off,
            Transform::default(),
            Skybox::default(),
            bevy::camera::Exposure::default(),
        ));
        textures.push(texture);
    }
    commands.insert_resource(MirrorTextures([textures[0].clone(), textures[1].clone()]));
}

/// Replace only the authored glass of each physical mirror with a private live
/// render-target material. The housing and chrome remain untouched.
fn wire_real_mirrors(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    roots: Query<&VehicleModelRoot>,
    scene: SceneNodes,
    painted: Query<&MeshMaterial3d<StandardMaterial>>,
    textures: Res<MirrorTextures>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let root = ready.entity;
    let Ok(model) = roots.get(root) else { return };
    let mut wired = 0;
    for entity in scene.descendants(root) {
        let Some(side) = scene.find_in_ancestry(entity, root, |name| mirror_side(model.kind, name))
        else {
            continue;
        };
        if painted.get(entity).is_err() {
            continue;
        }
        let mut material = StandardMaterial {
            base_color_texture: Some(textures.0[side].clone()),
            base_color: Color::WHITE,
            perceptual_roughness: 0.04,
            metallic: 0.0,
            unlit: true,
            double_sided: true,
            cull_mode: None,
            ..default()
        };
        let (horizontal, vertical) = mirror_texture_flips(model.kind);
        material.flip(horizontal, vertical);
        commands.entity(entity).insert((
            MeshMaterial3d(materials.add(material)),
            RealMirrorSurface {
                vehicle: model.vehicle,
                side: if scene
                    .find_in_ancestry(entity, root, |name| {
                        name.to_ascii_lowercase()
                            .contains("interior_mirrormiddle")
                            .then_some(())
                    })
                    .is_some()
                {
                    None
                } else {
                    Some(side)
                },
            },
        ));
        wired += 1;
    }
    info!("real rear-view mirrors: {wired} model surfaces wired");
}

fn update_mirrors(
    state: Res<State<GameState>>,
    settings: Res<LightingSettings>,
    main: Query<(&GameCamera, &Skybox, &bevy::camera::Exposure), Without<MirrorCamera>>,
    cars: Query<(&Transform, &Vehicle), Without<MirrorCamera>>,
    mut mirrors: Query<
        (
            &MirrorCamera,
            &mut Camera,
            &mut Transform,
            &mut Skybox,
            &mut bevy::camera::Exposure,
        ),
        Without<GameCamera>,
    >,
) {
    let source = main.single().ok().and_then(|(camera, sky, exposure)| {
        cars.get(camera.target).ok().map(|car| (exposure, sky, car))
    });
    let active =
        settings.mirrors_enabled() && *state.get() == GameState::Driving && source.is_some();
    for (mirror, mut camera, mut pose, mut sky, mut exposure) in &mut mirrors {
        camera.is_active = active;
        if let Some((main_exposure, main_sky, (car_pose, car))) = source {
            // Physics forward is +Z. Side mounts sit outside each car's body.
            let half_width = car.config.track_width_front * 0.5 + 0.3;
            let eye = car_pose.transform_point(Vec3::new(mirror.0 * half_width, 1.25, 0.0));
            let direction = car_pose.rotation * Vec3::new(mirror.0 * 0.22, 0.0, -1.0);
            *pose =
                Transform::from_translation(eye).looking_to(direction, car_pose.rotation * Vec3::Y);
            *sky = main_sky.clone();
            *exposure = *main_exposure;
        }
    }
}

/// An off mirror must not retain the last rendered frame. Tinting its private
/// material black also makes the visual state unambiguous while both expensive
/// render cameras are asleep.
fn set_real_mirror_enabled(
    settings: Res<LightingSettings>,
    surfaces: Query<(&RealMirrorSurface, &MeshMaterial3d<StandardMaterial>)>,
    parts: Query<&BreakablePart>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let tint = if settings.mirrors_enabled() {
        Color::WHITE
    } else {
        Color::BLACK
    };
    for (mirror, handle) in &surfaces {
        let broken = mirror.side.is_some_and(|side| {
            parts.iter().any(|part| {
                part.vehicle == mirror.vehicle
                    && part.detached
                    && part.kind
                        == if side == 0 {
                            DamagePartKind::MirrorLeft
                        } else {
                            DamagePartKind::MirrorRight
                        }
            })
        });
        let tint = if broken { Color::BLACK } else { tint };
        if let Some(material) = materials.get(&handle.0) {
            if material.base_color != tint {
                if let Some(mut material) = materials.get_mut(&handle.0) {
                    material.base_color = tint;
                }
            }
        }
    }
}
