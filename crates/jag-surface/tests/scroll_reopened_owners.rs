use jag_draw::{Brush, ColorLinPremul, wgpu};
use jag_surface::JagSurface;
use std::sync::Arc;

fn surface() -> JagSurface {
    let adapter =
        pollster::block_on(wgpu::Instance::default().request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    JagSurface::new(
        Arc::new(device),
        Arc::new(queue),
        wgpu::TextureFormat::Rgba8UnormSrgb,
    )
}

#[test]
fn reopened_scroll_owner_keeps_distinct_pixels_on_initial_and_retained_frames() {
    let mut surface = surface();
    for dpr in [1., 2.] {
        surface.set_dpi_scale(dpr);
        let mut c = surface.begin_frame((100. * dpr) as u32, (64. * dpr) as u32);
        c.clear(ColorLinPremul::from_srgba_u8([255; 4]));
        let capture = c.begin_scroll_scene_capture();
        for (x, w, h, color) in [
            (5., 30., 12., [0, 0, 255, 255]),
            (50., 12., 30., [0, 180, 0, 255]),
        ] {
            c.push_scroll_layer("reopened-owner", [0., 0.]);
            c.fill_rect(
                x,
                10.,
                w,
                h,
                Brush::Solid(ColorLinPremul::from_srgba_u8(color)),
                1,
            );
            c.pop_scroll_layer();
        }
        let retained = c.finish_retained_scroll_scene(capture).unwrap();
        let (width, _, pixels) = surface.end_frame_headless(c).unwrap();
        let assert_color = |pixels: &[u8], x: f32, y: f32, color: [u8; 4]| {
            let i = (((y * dpr) as u32 * width + (x * dpr) as u32) * 4) as usize;
            assert_eq!(&pixels[i..i + 4], &color, "DPR {dpr}, at {x},{y}");
        };
        assert_color(&pixels, 10., 15., [0, 0, 255, 255]);
        assert_color(&pixels, 55., 35., [0, 180, 0, 255]);
        for offset in [0., 4., 0.] {
            let mut c = surface.begin_frame((100. * dpr) as u32, (64. * dpr) as u32);
            c.clear(ColorLinPremul::from_srgba_u8([255; 4]));
            assert!(
                c.replay_retained_scroll_scene(
                    &retained,
                    &[("reopened-owner".into(), [0., offset])]
                        .into_iter()
                        .collect()
                )
            );
            let (_, _, pixels) = surface.end_frame_headless(c).unwrap();
            assert_color(&pixels, 10., 15. + offset, [0, 0, 255, 255]);
            assert_color(&pixels, 55., 35. + offset, [0, 180, 0, 255]);
        }
    }
}
