use jag_draw::{
    Brush, ColorLinPremul, MaskComposite, MaskCompositeLayer, MaskMode, Rect, Transform2D, wgpu,
};
use jag_surface::JagSurface;
use std::sync::Arc;
fn surface(format: wgpu::TextureFormat) -> JagSurface {
    let a =
        pollster::block_on(wgpu::Instance::default().request_adapter(&Default::default())).unwrap();
    let (d, q) = pollster::block_on(a.request_device(&Default::default())).unwrap();
    JagSurface::new(Arc::new(d), Arc::new(q), format)
}
#[test]
fn generated_css_mask_survives_page_translation_and_web_composition() {
    for format in [
        wgpu::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    ] {
        for web in [false, true] {
            for scale in [1.0, 2.0] {
                let mut s = surface(format);
                s.set_web_color_composition(web);
                s.set_logical_pixels(true);
                s.set_dpi_scale(scale);
                let mut c = s.begin_frame((500. * scale) as u32, (180. * scale) as u32);
                c.clear(ColorLinPremul::from_srgba_u8([255; 4]));
                c.fill_rect(
                    0.,
                    0.,
                    500.,
                    180.,
                    Brush::Solid(ColorLinPremul::from_srgba_u8([255; 4])),
                    0,
                );
                c.push_clip_rect(Rect {
                    x: 0.,
                    y: 40.,
                    w: 500.,
                    h: 140.,
                });
                c.push_transform(Transform2D::translate(0., 40.));
                let rect = Rect {
                    x: 260.,
                    y: 20.,
                    w: 80.,
                    h: 80.,
                };
                let brush = Brush::LinearGradient {
                    start: [260., 20.],
                    end: [340., 20.],
                    stops: vec![
                        (0., ColorLinPremul::from_srgba_u8([0, 0, 0, 0])),
                        (0.5, ColorLinPremul::from_srgba_u8([0, 0, 0, 0])),
                        (0.5, ColorLinPremul::from_srgba_u8([0, 0, 0, 255])),
                        (1., ColorLinPremul::from_srgba_u8([0, 0, 0, 255])),
                    ],
                };
                let mask = c
                    .resolve_generated_mask_pattern(
                        rect,
                        rect,
                        [80., 80.],
                        [false; 2],
                        &brush,
                        MaskMode::Alpha,
                    )
                    .unwrap();
                assert!(c.push_mask_group(vec![MaskCompositeLayer {
                    mask,
                    composite: MaskComposite::Add,
                    text_clip: false
                }]));
                c.fill_rect(
                    260.,
                    20.,
                    80.,
                    80.,
                    Brush::Solid(ColorLinPremul::from_srgba_u8([0, 128, 0, 255])),
                    5,
                );
                c.pop_filter();
                c.pop_transform();
                c.pop_clip();
                let (w, _, pixels) = s.end_frame_headless(c).unwrap();
                let at = |x: f32, y: f32| -> [u8; 4] {
                    let i = (((y * scale) as u32 * w + (x * scale) as u32) * 4) as usize;
                    pixels[i..i + 4].try_into().unwrap()
                };
                assert_eq!(at(330., 100.), [0, 128, 0, 255], "web={web} scale={scale}");
                assert_eq!(at(270., 100.), [255; 4], "web={web} scale={scale}");
            }
        }
    }
}

#[test]
fn svg_url_masks_decode_without_an_extension_and_keep_y_orientation() {
    let path = std::env::temp_dir().join(format!("jag-svg-mask-{}", std::process::id()));
    std::fs::write(&path,r#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="80"><rect x="0" y="40" width="40" height="40"/></svg>"#).unwrap();
    for scale in [1., 2.] {
        for count in [1, 2, 3] {
            let mut s = surface(wgpu::TextureFormat::Bgra8UnormSrgb);
            s.set_web_color_composition(true);
            s.set_logical_pixels(true);
            s.set_dpi_scale(scale);
            let mut c = s.begin_frame((120. * scale) as u32, (160. * scale) as u32);
            c.clear(ColorLinPremul::from_srgba_u8([255; 4]));
            c.push_transform(Transform2D::translate(4., 40.));
            c.push_opacity(0.8);
            let rect = Rect {
                x: 20.,
                y: 20.,
                w: 80.,
                h: 80.,
            };
            let mask = c
                .resolve_url_mask(&path, rect, rect, [80.; 2], [false; 2], MaskMode::Alpha)
                .unwrap();
            assert!(
                c.push_mask_group(
                    (0..count)
                        .map(|_| MaskCompositeLayer {
                            mask,
                            composite: MaskComposite::Intersect,
                            text_clip: false
                        })
                        .collect()
                )
            );
            c.fill_rect(
                20.,
                20.,
                80.,
                80.,
                Brush::Solid(ColorLinPremul::from_srgba_u8([0, 0, 255, 255])),
                1,
            );
            c.pop_filter();
            c.pop_opacity();
            c.pop_transform();
            let (w, _, pixels) = s.end_frame_headless(c).unwrap();
            let at = |x: f32, y: f32| -> [u8; 4] {
                let i = (((y * scale) as u32 * w + (x * scale) as u32) * 4) as usize;
                pixels[i..i + 4].try_into().unwrap()
            };
            assert_eq!(at(44., 120.), [51, 51, 255, 255], "bottom-left DPR {scale}");
            assert_eq!(at(44., 80.), [255; 4], "top-left DPR {scale}");
            assert_eq!(at(84., 120.), [255; 4], "bottom-right DPR {scale}");
        }
    }
    std::fs::remove_file(path).unwrap();
}
