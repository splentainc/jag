//! Offscreen frames paint raw pixel draws (video frames), as window frames do.
use jag_draw::{ColorLinPremul, wgpu};
use jag_surface::JagSurface;
use std::sync::Arc;

#[test]
fn headless_frame_paints_raw_image_pixels() {
    let adapter =
        pollster::block_on(wgpu::Instance::default().request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let mut surface = JagSurface::new(
        Arc::new(device),
        Arc::new(queue),
        wgpu::TextureFormat::Bgra8UnormSrgb,
    );
    let mut canvas = surface.begin_frame(8, 4);
    canvas.clear(ColorLinPremul::from_srgba_u8([255, 255, 255, 255]));
    // A 2x2 opaque BGRA source: blue 255, green 0, red 0.
    let blue_bgra = [255u8, 0, 0, 255].repeat(4);
    canvas.draw_raw_image(blue_bgra, 2, 2, [0.0, 0.0], [4.0, 4.0], 1);

    let (width, height, rgba) = surface.end_frame_headless(canvas).unwrap();
    assert_eq!((width, height), (8, 4));
    let pixel = |x: usize, y: usize| &rgba[(y * 8 + x) * 4..][..4];
    assert_eq!(pixel(1, 1), [0, 0, 255, 255], "inside the raw image");
    assert_eq!(pixel(6, 1), [255, 255, 255, 255], "outside the raw image");
}
