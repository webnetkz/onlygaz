//! Camera follow logic.
//!
//! Chase views smooth their offset in the car's local frame. The car's rendered
//! translation is then added without filtering, so normal forward, lateral and
//! vertical movement cannot leave the camera one physics tick behind.
//!
//! The bumper, bonnet and cockpit views bypass smoothing completely. Their
//! position and rotation are rebuilt directly from the car's interpolated
//! [`Transform`] every frame, making them rigid mounts with no drift or zoom.

use bevy::prelude::*;

use super::modes::{CameraMode, CameraModeConfig, FovCurve};
use crate::vehicle::config::VehicleConfig;
use crate::vehicle::physics::Vehicle;

/// Temporary exterior profile view selected while an angle-bracket key is held.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Reflect)]
pub enum SideView {
    #[default]
    None,
    Left,
    Right,
}

/// State of the single game camera.
#[derive(Component)]
pub struct GameCamera {
    /// Which car to follow.
    pub target: Entity,
    pub mode: CameraMode,
    /// Whether the current camera type is mirrored to look behind the car.
    pub rear_view: bool,
    /// A held left/right profile view. This temporarily overrides the regular
    /// camera mode without changing the mode selected with `C`.
    pub side_view: SideView,
    /// Smoothed eye position, world space.
    pub position: Vec3,
    /// Smoothed aim point, world space.
    pub look_at: Vec3,
    /// Smoothed field of view [rad].
    pub fov: f32,
    /// Smoothed eye offset from the car, expressed in world axes. Keeping the
    /// offset rather than the absolute position prevents translation lag.
    eye_offset: Vec3,
    /// Smoothed aim offset from the car, expressed in world axes.
    target_offset: Vec3,
    /// Length of the current mode's eye offset, for diagnostics.
    nominal_offset: f32,
    initialised: bool,
}

impl GameCamera {
    pub fn new(target: Entity) -> Self {
        Self {
            target,
            mode: CameraMode::default(),
            rear_view: false,
            side_view: SideView::None,
            position: Vec3::ZERO,
            look_at: Vec3::ZERO,
            fov: 65.0_f32.to_radians(),
            eye_offset: Vec3::ZERO,
            target_offset: Vec3::ZERO,
            nominal_offset: 0.0,
            initialised: false,
        }
    }

    /// Distance from the car that this mode nominally sits at [m]. Used by the
    /// tests to check the camera is not trailing.
    pub fn mode_offset_length(&self) -> f32 {
        self.nominal_offset
    }

    pub fn cycle(&mut self) {
        self.mode = self.mode.next();
        // Interior and exterior views are far apart; snap rather than sweep the
        // camera through the bodywork.
        self.initialised = false;
    }

    /// Follow a newly selected car and discard interpolation state from the
    /// previous one, so the preview never sweeps through the scene.
    pub fn set_target(&mut self, target: Entity) {
        self.target = target;
        self.rear_view = false;
        self.side_view = SideView::None;
        self.initialised = false;
    }

    /// Match the rear-facing state to a held input. Returns whether the mount
    /// changed, so callers can avoid resetting interpolation every frame.
    pub fn set_rear_view(&mut self, rear_view: bool) -> bool {
        if self.rear_view == rear_view {
            return false;
        }
        self.rear_view = rear_view;
        self.initialised = false;
        true
    }

    /// Match the temporary profile view to held input while preserving the
    /// regular camera choice. Returns whether the mount changed.
    pub fn set_side_view(&mut self, side_view: SideView) -> bool {
        if self.side_view == side_view {
            return false;
        }
        self.side_view = side_view;
        self.initialised = false;
        true
    }

    pub fn display_label(&self) -> String {
        if self.side_view == SideView::Left {
            "LEFT PROFILE".to_string()
        } else if self.side_view == SideView::Right {
            "RIGHT PROFILE".to_string()
        } else if self.rear_view {
            format!("REAR {}", self.mode.label())
        } else {
            self.mode.label().to_string()
        }
    }
}

/// Camera tuning, kept as a resource so it can be edited at runtime.
#[derive(Resource)]
pub struct CameraSettings {
    pub modes: Vec<CameraModeConfig>,
    pub fov: FovCurve,
    /// Base field of view used before the speed term is applied [rad].
    pub base_fov: f32,
}

/// Cockpit mount adjustment in the car's local frame (-Z forward, +Y up).
#[derive(Debug, Clone, Resource, Reflect)]
#[reflect(Resource)]
pub struct CockpitCameraSettings {
    pub offset: Vec3,
    pub pitch_degrees: f32,
}

impl Default for CockpitCameraSettings {
    fn default() -> Self {
        Self {
            offset: Vec3::new(0.0, 0.0, 0.35),
            pitch_degrees: 0.0,
        }
    }
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            modes: super::modes::default_modes(),
            fov: FovCurve::default(),
            base_fov: 65.0_f32.to_radians(),
        }
    }
}

impl CameraSettings {
    pub fn config(&self, mode: CameraMode) -> &CameraModeConfig {
        self.modes
            .iter()
            .find(|c| c.mode == mode)
            .unwrap_or(&self.modes[0])
    }
}

/// Exponential smoothing factor that is independent of the frame rate.
#[inline]
pub fn smoothing_factor(rate: f32, dt: f32) -> f32 {
    if rate <= 0.0 {
        return 1.0;
    }
    1.0 - (-rate * dt).exp()
}

/// Exact world-space pose for a camera rigidly mounted to the car.
fn mounted_pose(car: &Transform, config: &CameraModeConfig) -> (Vec3, Quat, Vec3) {
    let local_direction = (config.target - config.eye).normalize_or(Vec3::NEG_Z);
    let local_rotation = Transform::IDENTITY
        .looking_to(local_direction, Vec3::Y)
        .rotation;
    (
        car.translation + car.rotation * config.eye,
        car.rotation * local_rotation,
        car.translation + car.rotation * config.target,
    )
}

/// The rear cockpit camera is mounted directly on the roof rather than inside
/// the cabin. These coordinates are on the supplied Veyron roof mesh: the
/// camera sits a few centimetres above its crown and aims along the rear glass.
const ROOF_REAR_EYE: Vec3 = Vec3::new(0.0, 1.28, 0.18);
const ROOF_REAR_TARGET: Vec3 = Vec3::new(0.0, 1.16, 25.0);

fn rear_view_config(mode: CameraMode, mut config: CameraModeConfig) -> CameraModeConfig {
    if mode.is_interior() {
        config.eye = ROOF_REAR_EYE;
        config.target = ROOF_REAR_TARGET;
        config.position_smoothing = 0.0;
        config.rotation_smoothing = 0.0;
        config.yaw_only = false;
        config.fov_speed_scale = 0.0;
    } else {
        // Mirroring both mount points preserves the character of every view:
        // bumper remains bumper, hood remains hood, and chase remains chase.
        config.eye.z = -config.eye.z;
        config.target.z = -config.target.z;
    }
    config
}

/// Place the wheel camera at this car's authored driver eye. The base mode
/// supplies the pitch and viewing distance; only the mount point changes, so
/// every car looks forward from its own steering-wheel position.
fn steering_wheel_view_config(
    vehicle: &VehicleConfig,
    mut config: CameraModeConfig,
) -> CameraModeConfig {
    let direction = (config.target - config.eye).normalize_or(Vec3::NEG_Z);
    let distance = (config.target - config.eye).length();
    config.eye = vehicle.driver_eye;
    config.target = config.eye + direction * distance;
    config
}

/// Build a stable exterior camera that keeps the complete car profile in
/// frame. Collider dimensions make the distance work for every selectable car
/// instead of assuming that all models are Veyron-sized.
fn side_view_config(vehicle: &VehicleConfig, side: SideView) -> CameraModeConfig {
    let half_length = vehicle
        .collider_boxes
        .iter()
        .map(|(centre, half)| centre.z.abs() + half.z)
        .fold(2.3_f32, f32::max);
    let distance = (half_length * 2.65).max(6.2);
    let x = match side {
        SideView::Left => -distance,
        SideView::Right => distance,
        SideView::None => 0.0,
    };
    CameraModeConfig {
        mode: CameraMode::ChaseClose,
        eye: Vec3::new(x, 1.65, 0.35),
        target: Vec3::new(0.0, 0.72, -0.15),
        position_smoothing: 12.0,
        rotation_smoothing: 14.0,
        yaw_only: true,
        fov_speed_scale: 0.0,
    }
}

/// Drive the camera from the car's transform and velocity.
pub fn follow_vehicle(
    time: Res<Time>,
    settings: Res<CameraSettings>,
    cockpit: Res<CockpitCameraSettings>,
    vehicles: Query<(&Transform, &Vehicle), Without<GameCamera>>,
    mut cameras: Query<(&mut GameCamera, &mut Transform, &mut Projection)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    for (mut camera, mut transform, mut projection) in &mut cameras {
        let Ok((car, vehicle)) = vehicles.get(camera.target) else {
            continue;
        };
        let mut configured = settings.config(camera.mode).clone();
        if camera.side_view != SideView::None {
            configured = side_view_config(&vehicle.config, camera.side_view);
        } else if camera.rear_view {
            configured = rear_view_config(camera.mode, configured);
        } else if camera.mode == CameraMode::Cockpit {
            let base_direction = (configured.target - configured.eye).normalize_or(Vec3::NEG_Z);
            let distance = (configured.target - configured.eye).length();
            configured.eye += cockpit.offset + vehicle.config.cockpit_camera_offset;
            let aimed_direction =
                Quat::from_rotation_y(vehicle.config.cockpit_camera_yaw_degrees.to_radians())
                    * Quat::from_rotation_x(cockpit.pitch_degrees.to_radians())
                    * base_direction;
            configured.target = configured.eye + aimed_direction * distance;
        } else if camera.mode == CameraMode::SteeringWheel {
            configured = steering_wheel_view_config(&vehicle.config, configured);
        }
        let config = &configured;
        camera.nominal_offset = config.eye.length();

        let speed_kmh = vehicle.speed_kmh().abs();
        let fov_from_speed = settings.fov.sample(speed_kmh);
        let desired_fov =
            settings.base_fov + (fov_from_speed - settings.base_fov) * config.fov_speed_scale;

        if !config.yaw_only {
            // A mounted camera must never lag, smooth or move independently of
            // the chassis. Build its complete pose in the car's local frame.
            let (position, rotation, look_at) = mounted_pose(car, config);
            camera.eye_offset = position - car.translation;
            camera.target_offset = look_at - car.translation;
            camera.position = position;
            camera.look_at = look_at;
            camera.fov = desired_fov;
            camera.initialised = true;

            transform.translation = camera.position;
            transform.rotation = rotation;
            if let Projection::Perspective(perspective) = &mut *projection {
                perspective.fov = camera.fov;
            }
            continue;
        }

        // Chase cameras use a yaw-only frame so bumps, pitch and body roll do
        // not shake or tilt the horizon.
        let basis = {
            // Yaw only, built explicitly: `Quat::from_rotation_arc` picks an
            // arbitrary axis when the two vectors are antiparallel, which is
            // exactly the case when the car happens to face +Z.
            let forward = car.rotation * Vec3::NEG_Z;
            Quat::from_rotation_y(f32::atan2(-forward.x, -forward.z))
        };

        let desired_eye_offset = basis * config.eye;
        let desired_target_offset = basis * config.target;

        if !camera.initialised {
            camera.eye_offset = desired_eye_offset;
            camera.target_offset = desired_target_offset;
            camera.fov = desired_fov;
            camera.initialised = true;
        } else {
            let position_blend = smoothing_factor(config.position_smoothing, dt);
            let rotation_blend = smoothing_factor(config.rotation_smoothing, dt);
            let fov_blend = smoothing_factor(3.0, dt);
            let eye_offset = camera.eye_offset;
            let target_offset = camera.target_offset;
            camera.eye_offset = eye_offset + (desired_eye_offset - eye_offset) * position_blend;
            camera.target_offset =
                target_offset + (desired_target_offset - target_offset) * rotation_blend;
            camera.fov += (desired_fov - camera.fov) * fov_blend;
        }

        // Follow the already interpolated car translation exactly. Only the
        // orbit offsets above are smoothed, so there is no speed-dependent lag.
        camera.position = car.translation + camera.eye_offset;
        camera.look_at = car.translation + camera.target_offset;
        transform.translation = camera.position;
        transform.look_at(camera.look_at, Vec3::Y);

        if let Projection::Perspective(perspective) = &mut *projection {
            perspective.fov = camera.fov;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothing_is_between_zero_and_one() {
        for dt in [1.0 / 240.0, 1.0 / 60.0, 1.0 / 15.0] {
            let f = smoothing_factor(8.0, dt);
            assert!((0.0..=1.0).contains(&f), "{f}");
        }
        // A very high rate is effectively instantaneous.
        assert!(smoothing_factor(1000.0, 1.0 / 60.0) > 0.99);
        // Zero rate snaps, which is what we want for "no smoothing".
        assert_eq!(smoothing_factor(0.0, 0.016), 1.0);
    }

    #[test]
    fn faster_smoothing_converges_sooner() {
        let slow = smoothing_factor(2.0, 1.0 / 60.0);
        let fast = smoothing_factor(20.0, 1.0 / 60.0);
        assert!(fast > slow);
    }

    #[test]
    fn cockpit_view_has_the_requested_z_offset() {
        let cockpit = CockpitCameraSettings::default();
        assert!((cockpit.offset.z - 0.35).abs() < f32::EPSILON);
    }

    #[test]
    fn mustang_cockpit_view_is_mounted_at_the_drivers_head() {
        let veyron = crate::vehicle::presets::bugatti_veyron_super_sport();
        let mustang = crate::vehicle::presets::ford_mustang_gt3();
        let shared_eye = super::super::modes::default_modes()
            .into_iter()
            .find(|config| config.mode == CameraMode::Cockpit)
            .unwrap()
            .eye
            + CockpitCameraSettings::default().offset;

        assert_eq!(veyron.cockpit_camera_offset, Vec3::ZERO);
        assert!((shared_eye + mustang.cockpit_camera_offset).distance(mustang.driver_eye) < 1.0e-5);
        assert_eq!(mustang.cockpit_camera_yaw_degrees, 0.0);
    }

    #[test]
    fn mounted_pose_preserves_the_exact_local_camera_transform() {
        let config = super::super::modes::default_modes()
            .into_iter()
            .find(|config| config.mode == CameraMode::Cockpit)
            .unwrap();
        let car = Transform::from_translation(Vec3::new(12.0, 1.4, -80.0))
            .with_rotation(Quat::from_euler(EulerRot::YXZ, 0.7, -0.08, 0.12));

        let (position, rotation, target) = mounted_pose(&car, &config);
        let inverse = car.rotation.inverse();
        let local_position = inverse * (position - car.translation);
        let local_target = inverse * (target - car.translation);
        let expected_local_rotation = Transform::IDENTITY
            .looking_to((config.target - config.eye).normalize(), Vec3::Y)
            .rotation;

        assert!(local_position.distance(config.eye) < 1.0e-5);
        assert!(local_target.distance(config.target) < 1.0e-5);
        // `q` and `-q` represent the same rotation, so compare the absolute
        // dot product instead of quaternion components or signs.
        assert!((inverse * rotation).dot(expected_local_rotation).abs() > 1.0 - 1.0e-5);
    }

    #[test]
    fn rear_view_mirrors_every_exterior_mount() {
        for config in super::super::modes::default_modes() {
            if config.mode.is_interior() {
                continue;
            }
            let rear = rear_view_config(config.mode, config.clone());
            assert!((rear.eye.z + config.eye.z).abs() < 1.0e-6);
            assert!((rear.target.z + config.target.z).abs() < 1.0e-6);
        }
    }

    #[test]
    fn rear_interior_views_are_on_the_roof() {
        for mode in [CameraMode::Cockpit, CameraMode::SteeringWheel] {
            let config = super::super::modes::default_modes()
                .into_iter()
                .find(|config| config.mode == mode)
                .unwrap();
            let rear = rear_view_config(mode, config);
            assert!(rear.eye.y > 1.15, "{mode:?} rear camera is not on the roof");
            assert!(rear.target.z > 0.0, "{mode:?} must look backwards");
        }
    }

    #[test]
    fn every_selectable_car_has_a_wheel_view_at_its_driver_eye() {
        let base = super::super::modes::default_modes()
            .into_iter()
            .find(|config| config.mode == CameraMode::SteeringWheel)
            .unwrap();

        for kind in crate::vehicle::VehicleKind::ALL {
            let vehicle = kind.config();
            let view = steering_wheel_view_config(&vehicle, base.clone());
            assert_eq!(view.eye, vehicle.driver_eye, "{}", vehicle.name);
            assert!(
                view.target.z < view.eye.z,
                "{} does not look forward",
                vehicle.name
            );

            let wheel_from_eye = vehicle.steering_wheel_center - view.eye;
            assert!(
                wheel_from_eye.z < 0.0,
                "{} wheel is not ahead",
                vehicle.name
            );
            assert!(
                wheel_from_eye.y < 0.0,
                "{} wheel is not below eye level",
                vehicle.name
            );
            assert!(
                wheel_from_eye.x.abs() < 0.05,
                "{} eye is not aligned with its wheel",
                vehicle.name
            );
        }
    }

    #[test]
    fn side_views_are_symmetric_and_fit_every_car() {
        for kind in crate::vehicle::VehicleKind::ALL {
            let vehicle = kind.config();
            let left = side_view_config(&vehicle, SideView::Left);
            let right = side_view_config(&vehicle, SideView::Right);
            assert!(left.eye.x < -6.0, "{} left view", vehicle.name);
            assert_eq!(left.eye.x, -right.eye.x, "{} symmetry", vehicle.name);
            assert_eq!(left.target, right.target, "{} target", vehicle.name);
            assert_eq!(left.fov_speed_scale, 0.0);
        }
    }
}
