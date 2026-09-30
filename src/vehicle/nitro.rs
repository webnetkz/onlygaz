//! Nitro visual effects: layered additive blue flames at each exhaust outlet
//! and a short-range blue glow on the rear bodywork and road.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::VehicleKind;
use super::physics::Vehicle;
use crate::game::GameState;

// The Veyron model has a dedicated central nitro outlet. Keep the flame at
// the rear lip of that pipe instead of approximating a pair of side exhausts.
const VEYRON_EXHAUSTS: [Vec3; 1] = [Vec3::new(0.0, 0.404, 2.247)];
const DUAL_HYPERCAR_EXHAUSTS: [Vec3; 2] =
    [Vec3::new(-0.52, 0.37, 2.18), Vec3::new(0.52, 0.37, 2.18)];
const MUSTANG_EXHAUSTS: [Vec3; 2] = [Vec3::new(-0.57, 0.34, 2.45), Vec3::new(0.57, 0.34, 2.45)];
const SLS_EXHAUSTS: [Vec3; 2] = [Vec3::new(-0.60, 0.35, 2.31), Vec3::new(0.60, 0.35, 2.31)];
// The Zonda's central four-pipe cluster is one of the defining details of the
// car, so every authored outlet receives its own flame.
const ZONDA_EXHAUSTS: [Vec3; 4] = [
    Vec3::new(-0.105, 0.55, 2.18),
    Vec3::new(0.105, 0.55, 2.18),
    Vec3::new(-0.105, 0.74, 2.18),
    Vec3::new(0.105, 0.74, 2.18),
];

const OUTER_RADIUS: f32 = 0.135;
const OUTER_LENGTH: f32 = 1.70;
const CORE_RADIUS: f32 = 0.070;
const CORE_LENGTH: f32 = 0.92;
const GLOW_LUMENS: f32 = 18_000.0;
const GLOW_RANGE: f32 = 7.5;

#[derive(Resource)]
pub struct NitroVfxAssets {
    cone: Handle<Mesh>,
    outer_material: Handle<StandardMaterial>,
    core_material: Handle<StandardMaterial>,
}

#[derive(Component)]
pub struct NitroFlame {
    vehicle: Entity,
    outlet: Vec3,
    radius: f32,
    max_length: f32,
    phase: f32,
}

#[derive(Component)]
pub struct NitroGlow {
    vehicle: Entity,
    phase: f32,
}

pub fn exhaust_outlets(kind: VehicleKind) -> &'static [Vec3] {
    match kind {
        VehicleKind::Veyron => &VEYRON_EXHAUSTS,
        VehicleKind::MustangGt3 => &MUSTANG_EXHAUSTS,
        VehicleKind::MercedesSlsAmg => &SLS_EXHAUSTS,
        VehicleKind::PaganiZondaCinque => &ZONDA_EXHAUSTS,
        VehicleKind::MercedesAmgOne | VehicleKind::KoenigseggJesko => &DUAL_HYPERCAR_EXHAUSTS,
        VehicleKind::LamborghiniAventador
        | VehicleKind::FerrariEnzo
        | VehicleKind::LamborghiniRevuelto => &ZONDA_EXHAUSTS,
        VehicleKind::LamborghiniCountach
        | VehicleKind::FordGt2006
        | VehicleKind::FordMustang1965 => &SLS_EXHAUSTS,
    }
}

pub fn setup_nitro_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let cone = meshes.add(Cone::new(1.0, 1.0));
    let outer_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.01, 0.10, 1.0, 0.34),
        emissive: LinearRgba::rgb(1_200.0, 15_000.0, 90_000.0),
        emissive_exposure_weight: 1.0,
        alpha_mode: AlphaMode::Add,
        metallic: 0.0,
        perceptual_roughness: 1.0,
        ..default()
    });
    let core_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.40, 0.82, 1.0, 0.72),
        emissive: LinearRgba::rgb(18_000.0, 75_000.0, 180_000.0),
        emissive_exposure_weight: 1.0,
        alpha_mode: AlphaMode::Add,
        metallic: 0.0,
        perceptual_roughness: 1.0,
        ..default()
    });
    commands.insert_resource(NitroVfxAssets {
        cone,
        outer_material,
        core_material,
    });
}

pub fn spawn_nitro_vfx(
    commands: &mut Commands,
    vehicle: Entity,
    kind: VehicleKind,
    assets: &NitroVfxAssets,
) {
    let outlets = exhaust_outlets(kind);
    for (index, &outlet) in outlets.iter().enumerate() {
        let phase = index as f32 * 1.731;
        spawn_flame_layer(
            commands,
            vehicle,
            outlet,
            OUTER_RADIUS,
            OUTER_LENGTH,
            phase,
            assets.cone.clone(),
            assets.outer_material.clone(),
            "Outer",
        );
        spawn_flame_layer(
            commands,
            vehicle,
            outlet,
            CORE_RADIUS,
            CORE_LENGTH,
            phase + 0.63,
            assets.cone.clone(),
            assets.core_material.clone(),
            "Core",
        );
    }

    let centre = outlets.iter().copied().sum::<Vec3>() / outlets.len() as f32;
    commands.spawn((
        Name::new(format!("NitroGlow{}", kind.display_name())),
        NitroGlow {
            vehicle,
            phase: outlets.len() as f32,
        },
        PointLight {
            color: Color::srgb(0.04, 0.25, 1.0),
            intensity: 0.0,
            range: GLOW_RANGE,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_translation(centre + Vec3::Z * 0.25),
        Visibility::Hidden,
        ChildOf(vehicle),
    ));
}

#[allow(clippy::too_many_arguments)]
fn spawn_flame_layer(
    commands: &mut Commands,
    vehicle: Entity,
    outlet: Vec3,
    radius: f32,
    max_length: f32,
    phase: f32,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    layer: &'static str,
) {
    commands.spawn((
        Name::new(format!("NitroFlame{layer}")),
        NitroFlame {
            vehicle,
            outlet,
            radius,
            max_length,
            phase,
        },
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform {
            translation: outlet,
            // Bevy cones point along +Y; the car's rear is local +Z.
            rotation: Quat::from_rotation_x(core::f32::consts::FRAC_PI_2),
            scale: Vec3::ZERO,
        },
        Visibility::Hidden,
        NotShadowCaster,
        ChildOf(vehicle),
    ));
}

pub fn animate_nitro_vfx(
    time: Res<Time>,
    state: Res<State<GameState>>,
    vehicles: Query<&Vehicle>,
    mut flames: Query<(&NitroFlame, &mut Transform, &mut Visibility), Without<NitroGlow>>,
    mut glows: Query<(&NitroGlow, &mut PointLight, &mut Visibility), Without<NitroFlame>>,
) {
    let elapsed = time.elapsed_secs();
    for (flame, mut transform, mut visibility) in &mut flames {
        let strength = if *state.get() == GameState::Driving {
            vehicles
                .get(flame.vehicle)
                .map(Vehicle::nitro_strength)
                .unwrap_or(0.0)
        } else {
            0.0
        };
        if strength <= 0.01 {
            *visibility = Visibility::Hidden;
            continue;
        }

        *visibility = Visibility::Inherited;
        let fast_wave = (elapsed * 53.0 + flame.phase).sin();
        let slow_wave = (elapsed * 21.0 + flame.phase * 1.7).sin();
        let flicker = 0.88 + fast_wave * 0.08 + slow_wave * 0.04;
        let length = flame.max_length * (0.20 + 0.80 * strength.sqrt()) * flicker;
        let width = flame.radius * (0.72 + 0.28 * strength) * (0.96 + fast_wave * 0.04);
        transform.translation = flame.outlet + Vec3::Z * (length * 0.5);
        transform.scale = Vec3::new(width, length, width);
    }

    for (glow, mut light, mut visibility) in &mut glows {
        let strength = if *state.get() == GameState::Driving {
            vehicles
                .get(glow.vehicle)
                .map(Vehicle::nitro_strength)
                .unwrap_or(0.0)
        } else {
            0.0
        };
        let flicker = 0.90 + 0.10 * (elapsed * 41.0 + glow.phase).sin();
        light.intensity = GLOW_LUMENS * strength * flicker;
        *visibility = if strength > 0.01 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_car_has_exhausts_behind_its_rear_axle() {
        for kind in VehicleKind::ALL {
            let config = kind.config();
            let rear_axle = config
                .wheels
                .iter()
                .filter(|wheel| !wheel.id.is_front())
                .map(|wheel| wheel.anchor.z)
                .fold(f32::NEG_INFINITY, f32::max);
            let outlets = exhaust_outlets(kind);
            assert!(!outlets.is_empty());
            assert!(outlets.iter().all(|outlet| outlet.z > rear_axle));
        }
    }

    #[test]
    fn authored_exhaust_layouts_keep_their_pipe_counts() {
        assert_eq!(exhaust_outlets(VehicleKind::PaganiZondaCinque).len(), 4);
        assert_eq!(exhaust_outlets(VehicleKind::Veyron).len(), 1);
        assert_eq!(exhaust_outlets(VehicleKind::MustangGt3).len(), 2);
        assert_eq!(exhaust_outlets(VehicleKind::MercedesSlsAmg).len(), 2);
    }

    #[test]
    fn veyron_nitro_uses_the_central_exhaust_tip() {
        assert_eq!(
            exhaust_outlets(VehicleKind::Veyron),
            &[Vec3::new(0.0, 0.404, 2.247)]
        );
        assert_eq!(exhaust_outlets(VehicleKind::MercedesAmgOne).len(), 2);
        assert_eq!(exhaust_outlets(VehicleKind::KoenigseggJesko).len(), 2);
    }
}
