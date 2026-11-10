//! Fixed content must keep every color/alpha byte when navigation evicts masks.
use jag_draw::{
    Brush, ColorLinPremul, ColorMask, FontStyle, GlyphMask, JagTextProvider, RasterizedGlyph,
    SubpixelOrientation, TextProvider, TextRun, wgpu,
};
use jag_surface::JagSurface;
use std::sync::Arc;

struct Pressure;
impl TextProvider for Pressure {
    fn rasterize_run(&self, _: &TextRun) -> Vec<RasterizedGlyph> {
        vec![RasterizedGlyph {
            source: None,
            offset: [0., 0.],
            mask: GlyphMask::Color(ColorMask {
                width: 1536,
                height: 1024,
                data: vec![0; 1536 * 1024 * 4],
            }),
        }]
    }
}

fn run(text: &str) -> TextRun {
    TextRun {
        features: Vec::new(),
        source: None,
        spacing: None,
        rtl: None,
        text: text.into(),
        pos: [8., 32.],
        size: 18.,
        logical_size: 18.,
        color: ColorLinPremul::from_srgba_u8([0, 50, 180, 255]),
        weight: 400.,
        style: FontStyle::Normal,
        family: None,
    }
}

fn render(
    surface: &mut JagSurface,
    provider: Arc<dyn TextProvider>,
    scale: f32,
    dark: bool,
) -> Vec<u8> {
    let mut canvas = surface.begin_frame((512. * scale) as u32, (160. * scale) as u32);
    canvas.set_text_provider(provider);
    canvas.clear(ColorLinPremul::from_srgba_u8(if dark {
        [20, 20, 20, 255]
    } else {
        [255; 4]
    }));
    canvas.draw_text_context(run("Fixed title: office café"), None, 1);
    canvas.push_opacity(0.75);
    let mut paragraph = run("Several words with العربية and हिन्दी stay in place.");
    paragraph.pos[1] = 74.;
    canvas.draw_text_context(
        paragraph,
        Some(&Brush::Solid(ColorLinPremul::from_srgba_u8([
            50, 100, 180, 255,
        ]))),
        2,
    );
    canvas.pop_opacity();
    surface.end_frame_headless(canvas).unwrap().2
}

#[test]
fn real_text_keeps_exact_rgba_after_mask_eviction_at_both_device_scales() {
    let adapter =
        pollster::block_on(wgpu::Instance::default().request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let (device, queue) = (Arc::new(device), Arc::new(queue));
    let provider = Arc::new(JagTextProvider::from_system_fonts(SubpixelOrientation::RGB).unwrap());
    for web in [false, true] {
        let mut surface = JagSurface::new(
            device.clone(),
            queue.clone(),
            wgpu::TextureFormat::Rgba8UnormSrgb,
        );
        surface.set_web_color_composition(web);
        surface.set_logical_pixels(true);
        surface.set_frame_cache_enabled(false);
        surface.set_retained_layer_budget(0);
        for scale in [1., 2.] {
            surface.set_dpi_scale(scale);
            for dark in [false, true] {
                let before = render(&mut surface, provider.clone(), scale, dark);
                let cached = jag_draw::rasterize_run_cached(&*provider, &run("eviction lease"));
                let old = Arc::downgrade(&cached);
                drop(cached);
                let pressure = Pressure;
                for index in 0..3 {
                    jag_draw::rasterize_run_cached(
                        &pressure,
                        &run(&format!("new document {web} {scale} {dark} {index}")),
                    );
                }
                assert!(
                    old.upgrade().is_none(),
                    "the preceding document's mask must really be evicted"
                );
                let after = render(&mut surface, provider.clone(), scale, dark);
                assert_eq!(
                    before, after,
                    "RGBA changed: web={web} scale={scale} dark={dark}"
                );
            }
        }
    }
}
