use std::sync::Arc;

use jag_draw::{
    Brush, ColorLinPremul, ColorMatrix, DropShadow, ExternalTextureId, FilterEffect, MaskComposite,
    MaskCompositeLayer, MaskEffect, MaskMode, SrgbColor, wgpu,
};
use jag_surface::JagSurface;

struct BlockTextProvider;

impl jag_draw::TextProvider for BlockTextProvider {
    fn rasterize_run(&self, _run: &jag_draw::TextRun) -> Vec<jag_draw::RasterizedGlyph> {
        vec![jag_draw::RasterizedGlyph {
            source: None,
            offset: [0.0, 0.0],
            mask: jag_draw::GlyphMask::Subpixel(jag_draw::SubpixelMask {
                width: 2,
                height: 2,
                format: jag_draw::MaskFormat::Rgba8,
                data: [255, 255, 255, 0].repeat(4),
            }),
        }]
    }

    fn cache_tag(&self) -> u64 {
        0x5445_5854_434c_4950
    }
}

fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let offset = ((y * width + x) * 4) as usize;
    pixels[offset..offset + 4].try_into().unwrap()
}

fn test_gpu() -> Option<(Arc<wgpu::Device>, Arc<wgpu::Queue>, JagSurface)> {
    let instance = wgpu::Instance::default();
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .ok()?;
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
    let device = Arc::new(device);
    let queue = Arc::new(queue);
    let mut surface = JagSurface::new(
        device.clone(),
        queue.clone(),
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    surface.set_frame_cache_enabled(false);
    Some((device, queue, surface))
}

fn test_surface() -> Option<JagSurface> {
    test_gpu().map(|(_, _, surface)| surface)
}

fn physical(logical: f32, scale: f32) -> u32 {
    (logical * scale).round() as u32
}

include!("opacity_compositor/part1.rs");
include!("opacity_compositor/part2.rs");
include!("opacity_compositor/part3.rs");
include!("opacity_compositor/part4.rs");
