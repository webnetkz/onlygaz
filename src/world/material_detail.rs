//! "Detailed materials" off: cheap stand-ins for the expensive material
//! features, for budget phone GPUs.
//!
//! The car scans use see-through glass ([`StandardMaterial::specular_transmission`]),
//! clear-coated paint, normal and occlusion maps. Transmission alone costs a
//! copy of the whole screen and a separate render pass every frame, and every
//! other feature adds texture reads and shading to each pixel. With the
//! setting off:
//!
//! * transmissive glass becomes ordinary alpha-blended tinted glass;
//! * clear coat and anisotropy are dropped;
//! * normal and occlusion maps are dropped (the colour textures stay).
//!
//! The original values are remembered per material, so turning the setting
//! back on restores the exact authored look without reloading anything.

use bevy::asset::AssetEventSystems;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use super::lighting::LightingSettings;

/// How opaque fully transmissive glass becomes, before its own alpha.
const GLASS_OPACITY: f32 = 0.32;

/// The parts of a material that the simplified look removes.
#[derive(Debug, Clone, PartialEq)]
struct DetailedParts {
    normal_map_texture: Option<Handle<Image>>,
    occlusion_texture: Option<Handle<Image>>,
    clearcoat: f32,
    anisotropy_strength: f32,
    specular_transmission: f32,
    diffuse_transmission: f32,
    thickness: f32,
    alpha_mode: AlphaMode,
    base_color: Color,
}

impl DetailedParts {
    fn of(material: &StandardMaterial) -> Self {
        Self {
            normal_map_texture: material.normal_map_texture.clone(),
            occlusion_texture: material.occlusion_texture.clone(),
            clearcoat: material.clearcoat,
            anisotropy_strength: material.anisotropy_strength,
            specular_transmission: material.specular_transmission,
            diffuse_transmission: material.diffuse_transmission,
            thickness: material.thickness,
            alpha_mode: material.alpha_mode,
            base_color: material.base_color,
        }
    }

    fn restore(&self, material: &mut StandardMaterial) {
        material.normal_map_texture = self.normal_map_texture.clone();
        material.occlusion_texture = self.occlusion_texture.clone();
        material.clearcoat = self.clearcoat;
        material.anisotropy_strength = self.anisotropy_strength;
        material.specular_transmission = self.specular_transmission;
        material.diffuse_transmission = self.diffuse_transmission;
        material.thickness = self.thickness;
        material.alpha_mode = self.alpha_mode;
        material.base_color = self.base_color;
    }
}

/// Whether a material uses anything the simplified look removes.
fn has_detail(material: &StandardMaterial) -> bool {
    material.normal_map_texture.is_some()
        || material.occlusion_texture.is_some()
        || material.clearcoat > 0.0
        || material.anisotropy_strength > 0.0
        || material.specular_transmission > 0.0
        || material.diffuse_transmission > 0.0
}

/// Strip the expensive features. Returns what was removed, or `None` when
/// the material had nothing to strip.
fn simplify(material: &mut StandardMaterial) -> Option<DetailedParts> {
    if !has_detail(material) {
        return None;
    }
    let parts = DetailedParts::of(material);
    material.normal_map_texture = None;
    material.occlusion_texture = None;
    material.clearcoat = 0.0;
    material.anisotropy_strength = 0.0;
    let transmission = material
        .specular_transmission
        .max(material.diffuse_transmission)
        .clamp(0.0, 1.0);
    if transmission > 0.0 {
        material.specular_transmission = 0.0;
        material.diffuse_transmission = 0.0;
        material.thickness = 0.0;
        let alpha = material.base_color.alpha() * (1.0 - transmission * (1.0 - GLASS_OPACITY));
        material.base_color.set_alpha(alpha);
        if matches!(material.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_)) {
            material.alpha_mode = AlphaMode::Blend;
        }
    }
    Some(parts)
}

/// Originals of every simplified material.
#[derive(Resource, Default)]
struct SimplifiedMaterials {
    originals: HashMap<AssetId<StandardMaterial>, DetailedParts>,
    /// Whether the simplified look is currently applied.
    active: bool,
}

pub struct MaterialDetailPlugin;

impl Plugin for MaterialDetailPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SimplifiedMaterials>().add_systems(
            PostUpdate,
            (switch_material_detail, simplify_new_materials)
                .chain()
                // New materials are changed in the frame they arrive, before
                // the renderer prepares them: the detailed version is never
                // uploaded or compiled.
                .after(AssetEventSystems),
        );
    }
}

fn switch_material_detail(
    settings: Res<LightingSettings>,
    mut state: ResMut<SimplifiedMaterials>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let wanted = !settings.detailed_materials;
    if state.active == wanted {
        return;
    }
    state.active = wanted;
    if wanted {
        let ids: Vec<_> = materials.ids().collect();
        let mut simplified = 0usize;
        for id in ids {
            if let Some(mut material) = materials.get_mut(id)
                && let Some(parts) = simplify(&mut material)
            {
                state.originals.insert(id, parts);
                simplified += 1;
            }
        }
        info!("simplified materials: {simplified} changed");
    } else {
        let originals = std::mem::take(&mut state.originals);
        let restored = originals.len();
        for (id, parts) in originals {
            if let Some(mut material) = materials.get_mut(id) {
                parts.restore(&mut material);
            }
        }
        info!("detailed materials: {restored} restored");
    }
}

fn simplify_new_materials(
    mut events: MessageReader<AssetEvent<StandardMaterial>>,
    mut state: ResMut<SimplifiedMaterials>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let active = state.active;
    for event in events.read() {
        match *event {
            AssetEvent::Added { id } if active => {
                // Untracked: this frame's extraction already sees the change,
                // without a second "modified" upload.
                if let Some(material) = materials.get_mut_untracked(id)
                    && let Some(parts) = simplify(material)
                {
                    state.originals.insert(id, parts);
                }
            }
            AssetEvent::Removed { id } | AssetEvent::Unused { id } => {
                state.originals.remove(&id);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glass() -> StandardMaterial {
        StandardMaterial {
            base_color: Color::srgb(0.1, 0.12, 0.14),
            specular_transmission: 1.0,
            thickness: 0.01,
            clearcoat: 1.0,
            ..default()
        }
    }

    #[test]
    fn transmissive_glass_becomes_blended_tinted_glass_and_back() {
        let mut material = glass();
        let original = material.clone();
        let parts = simplify(&mut material).expect("glass has detail");
        assert_eq!(material.specular_transmission, 0.0);
        assert_eq!(material.clearcoat, 0.0);
        assert_eq!(material.alpha_mode, AlphaMode::Blend);
        assert!((material.base_color.alpha() - GLASS_OPACITY).abs() < 1.0e-6);
        assert!(!has_detail(&material));

        parts.restore(&mut material);
        assert_eq!(DetailedParts::of(&material), DetailedParts::of(&original));
    }

    #[test]
    fn plain_materials_are_left_alone() {
        let mut material = StandardMaterial {
            base_color: Color::srgb(0.5, 0.5, 0.5),
            ..default()
        };
        assert!(simplify(&mut material).is_none());
        assert_eq!(material.alpha_mode, AlphaMode::Opaque);
    }

    #[test]
    fn switching_the_setting_restores_every_material() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>()
            .insert_resource(LightingSettings::default())
            .add_plugins(MaterialDetailPlugin);
        // Strong handles keep the assets alive, as the material components do.
        let handle = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(glass());
        let id = handle.id();
        app.update();
        let transmission = |app: &App| {
            app.world()
                .resource::<Assets<StandardMaterial>>()
                .get(id)
                .unwrap()
                .specular_transmission
        };
        assert_eq!(transmission(&app), 1.0);

        app.world_mut()
            .resource_mut::<LightingSettings>()
            .detailed_materials = false;
        app.update();
        assert_eq!(transmission(&app), 0.0);

        // A material that arrives while the simple look is active.
        let late_handle = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(glass());
        let late = late_handle.id();
        app.update();
        let late_transmission = app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(late)
            .unwrap()
            .specular_transmission;
        assert_eq!(late_transmission, 0.0);

        app.world_mut()
            .resource_mut::<LightingSettings>()
            .detailed_materials = true;
        app.update();
        assert_eq!(transmission(&app), 1.0);
        drop((handle, late_handle));
    }
}
