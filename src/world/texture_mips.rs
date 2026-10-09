//! Mipmaps for textures loaded from PNG and JPEG files (phone build).
//!
//! PNG and JPEG hold a single image, so Bevy uploads them without mipmaps. A
//! surface seen at a distance - the road ahead, a car a hundred metres away -
//! then samples a texture many times larger than its size on screen: every
//! pixel reads texels far apart, which wastes the GPU's memory bandwidth
//! (scarce on a phone) and makes fine detail shimmer. This module adds the full
//! mipmap chain to every file-loaded texture in the frame it arrives, before
//! it is first uploaded, filtering colour textures in linear light.
//!
//! The phone build also lets go of the CPU copy of every 3D texture once the
//! GPU has it: the phone's memory is shared by both, and nothing in the game
//! reads those pixels back.

use bevy::asset::{AssetEventSystems, RenderAssetUsages};
use bevy::image::TextureFormatPixelInfo;
use bevy::prelude::*;
use bevy::render::render_resource::{TextureDimension, TextureFormat};
use bevy::tasks::{ComputeTaskPool, TaskPool};

/// Textures under these folders are only ever sampled by the GPU.
const GPU_ONLY_FOLDERS: [&str; 4] = ["models/", "traffic/", "environment/", "effects/"];
/// Converted to cubemaps on the CPU after loading (see `lighting`): no mips,
/// and their pixels stay available.
const SKYBOX_FOLDER: &str = "environment/skyboxes/";
/// Below this size a texture costs too little to bother.
const MIN_EDGE: u32 = 16;

pub struct TextureMipsPlugin;

impl Plugin for TextureMipsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, add_mipmaps.after(AssetEventSystems));
    }
}

/// Channel layout of a format this module can filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Layout {
    channels: usize,
    srgb: bool,
}

fn layout(format: TextureFormat) -> Option<Layout> {
    match format {
        TextureFormat::Rgba8UnormSrgb => Some(Layout {
            channels: 4,
            srgb: true,
        }),
        TextureFormat::Rgba8Unorm => Some(Layout {
            channels: 4,
            srgb: false,
        }),
        TextureFormat::Rg8Unorm => Some(Layout {
            channels: 2,
            srgb: false,
        }),
        TextureFormat::R8Unorm => Some(Layout {
            channels: 1,
            srgb: false,
        }),
        _ => None,
    }
}

/// Number of levels in a full mip chain for a `width` × `height` image.
pub fn mip_level_count(width: u32, height: u32) -> u32 {
    32 - width.max(height).max(1).leading_zeros()
}

/// sRGB-encoded byte -> linear value.
struct SrgbTables {
    to_linear: [f32; 256],
    /// Linear value quantised to `ENCODE_STEPS` -> sRGB byte.
    to_srgb: Vec<u8>,
}

const ENCODE_STEPS: usize = 8192;

impl SrgbTables {
    fn new() -> Self {
        let mut to_linear = [0.0; 256];
        for (value, linear) in to_linear.iter_mut().enumerate() {
            let c = value as f32 / 255.0;
            *linear = if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            };
        }
        let to_srgb = (0..=ENCODE_STEPS)
            .map(|step| {
                let l = step as f32 / ENCODE_STEPS as f32;
                let c = if l <= 0.003_130_8 {
                    l * 12.92
                } else {
                    1.055 * l.powf(1.0 / 2.4) - 0.055
                };
                (c * 255.0).round().clamp(0.0, 255.0) as u8
            })
            .collect();
        Self { to_linear, to_srgb }
    }

    fn encode(&self, linear: f32) -> u8 {
        let index = (linear.clamp(0.0, 1.0) * ENCODE_STEPS as f32).round() as usize;
        self.to_srgb[index]
    }
}

/// The next smaller level of a mip chain: each texel is the average of the
/// (up to) 2×2 texels it covers. Colour channels of sRGB images are averaged
/// in linear light; alpha and non-colour data are averaged as stored.
fn downsample(
    source: &[u8],
    width: u32,
    height: u32,
    layout: Layout,
    tables: &SrgbTables,
) -> (Vec<u8>, u32, u32) {
    let next_width = (width / 2).max(1);
    let next_height = (height / 2).max(1);
    let channels = layout.channels;
    let mut out = vec![0u8; (next_width * next_height) as usize * channels];
    let row = width as usize * channels;
    for y in 0..next_height {
        let y0 = (y * 2).min(height - 1) as usize;
        let y1 = (y * 2 + 1).min(height - 1) as usize;
        for x in 0..next_width {
            let x0 = (x * 2).min(width - 1) as usize;
            let x1 = (x * 2 + 1).min(width - 1) as usize;
            let corners = [
                y0 * row + x0 * channels,
                y0 * row + x1 * channels,
                y1 * row + x0 * channels,
                y1 * row + x1 * channels,
            ];
            let target = (y as usize * next_width as usize + x as usize) * channels;
            for channel in 0..channels {
                let colour = layout.srgb && channel < 3;
                out[target + channel] = if colour {
                    let sum: f32 = corners
                        .iter()
                        .map(|&at| tables.to_linear[source[at + channel] as usize])
                        .sum();
                    tables.encode(sum * 0.25)
                } else {
                    let sum: u32 = corners.iter().map(|&at| source[at + channel] as u32).sum();
                    ((sum + 2) / 4) as u8
                };
            }
        }
    }
    (out, next_width, next_height)
}

/// Level 0 followed by every smaller level, as Bevy stores mipmapped 2D
/// images. Returns the data and the number of levels.
fn mip_chain(level0: Vec<u8>, width: u32, height: u32, layout: Layout) -> (Vec<u8>, u32) {
    let tables = SrgbTables::new();
    let levels = mip_level_count(width, height);
    let mut chain = Vec::with_capacity(level0.len() * 4 / 3 + 16);
    chain.extend_from_slice(&level0);
    let (mut current, mut w, mut h) = (level0, width, height);
    for _ in 1..levels {
        let (next, next_width, next_height) = downsample(&current, w, h, layout, &tables);
        chain.extend_from_slice(&next);
        current = next;
        w = next_width;
        h = next_height;
    }
    (chain, levels)
}

/// Whether an image (loaded from `path`) can and should get mipmaps.
fn wants_mipmaps(image: &Image, path: &str) -> bool {
    let descriptor = &image.texture_descriptor;
    !path.starts_with(SKYBOX_FOLDER)
        && image.data.is_some()
        && descriptor.mip_level_count == 1
        && descriptor.dimension == TextureDimension::D2
        && descriptor.size.depth_or_array_layers == 1
        && descriptor.size.width.max(descriptor.size.height) >= MIN_EDGE
        && image.texture_view_descriptor.is_none()
        && layout(descriptor.format).is_some()
        && image.data.as_ref().is_some_and(|data| {
            descriptor.format.pixel_size().is_ok_and(|size| {
                data.len()
                    == size * descriptor.size.width as usize * descriptor.size.height as usize
            })
        })
}

fn gpu_only(path: &str) -> bool {
    !path.starts_with(SKYBOX_FOLDER)
        && GPU_ONLY_FOLDERS
            .iter()
            .any(|folder| path.starts_with(folder))
}

struct Job {
    id: AssetId<Image>,
    data: Vec<u8>,
    width: u32,
    height: u32,
    layout: Layout,
    release_cpu_copy: bool,
    result: Option<(Vec<u8>, u32)>,
}

fn add_mipmaps(
    mut events: MessageReader<AssetEvent<Image>>,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
) {
    let mut jobs = Vec::new();
    for event in events.read() {
        let AssetEvent::Added { id } = *event else {
            continue;
        };
        // Only images loaded from files: render targets, font atlases and
        // generated images have no path and are left alone.
        let Some(path) = asset_server.get_path(id) else {
            continue;
        };
        let path = path.path().to_string_lossy().replace('\\', "/");
        // Untracked: this frame's extraction uploads the finished chain once.
        let Some(image) = images.get_mut_untracked(id) else {
            continue;
        };
        if !wants_mipmaps(image, &path) {
            continue;
        }
        let Some(layout) = layout(image.texture_descriptor.format) else {
            continue;
        };
        let Some(data) = image.data.take() else {
            continue;
        };
        jobs.push(Job {
            id,
            data,
            width: image.texture_descriptor.size.width,
            height: image.texture_descriptor.size.height,
            layout,
            release_cpu_copy: gpu_only(&path),
            result: None,
        });
    }
    if jobs.is_empty() {
        return;
    }

    let pool = ComputeTaskPool::get_or_init(TaskPool::default);
    pool.scope(|scope| {
        for job in &mut jobs {
            scope.spawn(async move {
                let data = std::mem::take(&mut job.data);
                job.result = Some(mip_chain(data, job.width, job.height, job.layout));
            });
        }
    });

    let mut texels = 0u64;
    for job in jobs {
        let Some(image) = images.get_mut_untracked(job.id) else {
            continue;
        };
        let Some((chain, levels)) = job.result else {
            continue;
        };
        texels += (chain.len() / job.layout.channels) as u64;
        image.data = Some(chain);
        image.texture_descriptor.mip_level_count = levels;
        if job.release_cpu_copy {
            image.asset_usage = RenderAssetUsages::RENDER_WORLD;
        }
    }
    debug!("mipmaps generated: {texels} texels");
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::render::render_resource::Extent3d;

    #[test]
    fn mip_chains_go_down_to_one_texel() {
        assert_eq!(mip_level_count(1, 1), 1);
        assert_eq!(mip_level_count(1024, 1024), 11);
        assert_eq!(mip_level_count(512, 3072), 12);
        assert_eq!(mip_level_count(1000, 600), 10);

        let layout = Layout {
            channels: 4,
            srgb: true,
        };
        let (chain, levels) = mip_chain(vec![128; 8 * 4 * 4], 8, 4, layout);
        assert_eq!(levels, 4);
        // 8x4 + 4x2 + 2x1 + 1x1 texels.
        assert_eq!(chain.len(), (32 + 8 + 2 + 1) * 4);
    }

    #[test]
    fn colour_is_averaged_in_linear_light_and_alpha_as_stored() {
        let tables = SrgbTables::new();
        // Black and white texels: linear average 0.5 is sRGB 188, not 128.
        let source = [0, 0, 0, 0, 255, 255, 255, 255];
        let layout = Layout {
            channels: 4,
            srgb: true,
        };
        let (out, w, h) = downsample(&source, 2, 1, layout, &tables);
        assert_eq!((w, h), (1, 1));
        assert!((187..=189).contains(&out[0]), "{}", out[0]);
        assert_eq!(out[3], 128);

        let linear = Layout {
            channels: 1,
            srgb: false,
        };
        let (out, _, _) = downsample(&[0, 255, 255, 0], 2, 2, linear, &tables);
        assert_eq!(out, vec![128]);
    }

    #[test]
    fn odd_sizes_reuse_the_edge_texels() {
        let tables = SrgbTables::new();
        let layout = Layout {
            channels: 1,
            srgb: false,
        };
        let (out, w, h) = downsample(&[10, 20, 30], 3, 1, layout, &tables);
        assert_eq!((w, h), (1, 1));
        assert_eq!(out, vec![15]);
    }

    #[test]
    fn only_plain_file_textures_get_mipmaps() {
        let image = Image::new_fill(
            Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[255, 0, 0, 255],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        assert!(wants_mipmaps(&image, "traffic/bus/textures/body.png"));
        assert!(!wants_mipmaps(&image, "environment/skyboxes/day.png"));
        let tiny = Image::new_fill(
            Extent3d {
                width: 8,
                height: 8,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[255, 0, 0, 255],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        assert!(!wants_mipmaps(&tiny, "ui/icon.png"));
        let mut mipmapped = image.clone();
        mipmapped.texture_descriptor.mip_level_count = 7;
        assert!(!wants_mipmaps(&mipmapped, "ui/icon.png"));
        assert!(gpu_only("models/bugatti/textures/body.png"));
        assert!(!gpu_only("ui/onlygaz_logo.png"));
        assert!(!gpu_only("environment/skyboxes/night.png"));
    }
}
