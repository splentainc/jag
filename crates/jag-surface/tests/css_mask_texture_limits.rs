use jag_draw::{Brush, ColorLinPremul, MaskMode, Rect, Transform2D, wgpu};
use jag_surface::JagSurface;
use std::sync::Arc;

fn surface() -> JagSurface {
    let adapter =
        pollster::block_on(wgpu::Instance::default().request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let mut surface = JagSurface::new(
        Arc::new(device),
        Arc::new(queue),
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    surface.set_web_color_composition(true);
    surface
}

#[test]
fn oversized_generated_mask_keeps_full_tile_mapping_and_cached_upload() {
    let mut surface = surface();
    let limit = surface.device().limits().max_texture_dimension_2d;
    for vertical in [false, true] {
        for dpi in [1., 2.] {
            surface.set_dpi_scale(dpi);
            let length = (limit as f32 + 1440.) / dpi;
            let tile = Rect {
                x: 10.,
                y: 10.,
                w: if vertical { 24. } else { length },
                h: if vertical { length } else { 24. },
            };
            let end = if vertical {
                [tile.x, tile.y + tile.h]
            } else {
                [tile.x + tile.w, tile.y]
            };
            let brush = Brush::LinearGradient {
                start: [tile.x, tile.y],
                end,
                stops: vec![
                    (0., ColorLinPremul::from_srgba_u8([0; 4])),
                    (1., ColorLinPremul::from_srgba_u8([0, 0, 0, 255])),
                ],
            };
            let mut previous = None;
            // Sample a small viewport at three distant positions in the tile.
            // Clipping/cropping the logical mask would lose the middle/end.
            for fraction in [0.1, 0.5, 0.9, 0.9] {
                let mut canvas = surface.begin_frame((64. * dpi) as u32, (64. * dpi) as u32);
                canvas.clear(ColorLinPremul::from_srgba_u8([255; 4]));
                let offset = length * fraction;
                canvas.push_transform(if vertical {
                    Transform2D::translate(0., -offset)
                } else {
                    Transform2D::translate(-offset, 0.)
                });
                assert!(canvas.push_generated_mask(tile, &brush, MaskMode::Alpha));
                canvas.fill_rect(
                    tile.x,
                    tile.y,
                    tile.w,
                    tile.h,
                    Brush::Solid(ColorLinPremul::from_srgba_u8([0, 128, 0, 255])),
                    0,
                );
                canvas.pop_filter();
                canvas.pop_transform();
                let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
                let i = (((20. * dpi) as u32 * width + (20. * dpi) as u32) * 4) as usize;
                let pixel = &pixels[i..i + 4];
                let expected = (255. * (1. - (offset + 10.) / length)).round() as i32;
                assert!(
                    (pixel[0] as i32 - expected).abs() <= 2,
                    "vertical={vertical} dpi={dpi} fraction={fraction} pixel={pixel:?}"
                );
                assert_eq!(pixel[3], 255);
                if fraction == 0.9 {
                    if let Some(ref old) = previous {
                        assert_eq!(pixel, old);
                    }
                    previous = Some(pixel.to_vec());
                }
            }
        }
    }
}

#[test]
fn oversized_shadow_knockout_preserves_transparent_owner() {
    use jag_draw::{RoundedRadii, RoundedRect};
    let mut surface = surface();
    let limit = surface.device().limits().max_texture_dimension_2d;
    for vertical in [false, true] {
        for dpi in [1., 2.] {
            surface.set_dpi_scale(dpi);
            let length = (limit as f32 + 1440.) / dpi;
            let bounds = Rect {
                x: 10.,
                y: 10.,
                w: if vertical { 24. } else { length },
                h: if vertical { length } else { 24. },
            };
            let owner = RoundedRect {
                rect: Rect {
                    x: if vertical { 12. } else { 10. + length * 0.25 },
                    y: if vertical { 10. + length * 0.25 } else { 12. },
                    w: if vertical { 20. } else { length * 0.5 },
                    h: if vertical { length * 0.5 } else { 20. },
                },
                radii: RoundedRadii::default(),
            };
            for fraction in [0.1, 0.5, 0.9] {
                let mut canvas = surface.begin_frame((64. * dpi) as u32, (64. * dpi) as u32);
                canvas.clear(ColorLinPremul::from_srgba_u8([255; 4]));
                let offset = length * fraction;
                canvas.push_transform(if vertical {
                    Transform2D::translate(0., -offset)
                } else {
                    Transform2D::translate(-offset, 0.)
                });
                assert!(canvas.push_outer_shadow_mask(bounds, owner));
                canvas.fill_rect(
                    bounds.x,
                    bounds.y,
                    bounds.w,
                    bounds.h,
                    Brush::Solid(ColorLinPremul::from_srgba_u8([255, 0, 0, 255])),
                    0,
                );
                canvas.pop_filter();
                canvas.pop_transform();
                let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
                let i = (((20. * dpi) as u32 * width + (20. * dpi) as u32) * 4) as usize;
                let expected = if fraction == 0.5 {
                    [255; 4]
                } else {
                    [255, 0, 0, 255]
                };
                assert_eq!(
                    pixels[i..i + 4],
                    expected,
                    "vertical={vertical} dpi={dpi} fraction={fraction}"
                );
            }
        }
    }
}

#[test]
fn completed_canvas_upload_adapts_to_a_smaller_receiving_device() {
    let source = surface();
    let adapter =
        pollster::block_on(wgpu::Instance::default().request_adapter(&Default::default())).unwrap();
    let limits = wgpu::Limits {
        max_texture_dimension_2d: 4096,
        ..Default::default()
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: limits,
        ..Default::default()
    }))
    .unwrap();
    let mut receiver = JagSurface::new(
        Arc::new(device),
        Arc::new(queue),
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    receiver.set_web_color_composition(true);
    let rect = Rect {
        x: 0.,
        y: 0.,
        w: 6000.,
        h: 32.,
    };
    let mut canvas = source.begin_frame(64, 64);
    canvas.clear(ColorLinPremul::from_srgba_u8([255; 4]));
    canvas.push_transform(Transform2D::translate(-5000., 0.));
    let brush = Brush::LinearGradient {
        start: [0., 0.],
        end: [6000., 0.],
        stops: vec![
            (0., ColorLinPremul::from_srgba_u8([0; 4])),
            (1., ColorLinPremul::from_srgba_u8([0, 0, 0, 255])),
        ],
    };
    assert!(canvas.push_generated_mask(rect, &brush, MaskMode::Alpha));
    canvas.fill_rect(
        0.,
        0.,
        6000.,
        32.,
        Brush::Solid(ColorLinPremul::from_srgba_u8([0, 0, 0, 255])),
        0,
    );
    canvas.pop_filter();
    canvas.pop_transform();
    for _ in 0..2 {
        let (width, _, pixels) = receiver.end_frame_headless(canvas.clone()).unwrap();
        let i = ((16 * width + 20) * 4) as usize;
        assert!(
            (pixels[i] as i32 - 42).abs() <= 2,
            "{:?}",
            &pixels[i..i + 4]
        );
        assert_eq!(pixels[i + 3], 255);
    }
}
