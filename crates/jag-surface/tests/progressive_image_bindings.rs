use jag_draw::{ColorLinPremul, Command, Rect, wgpu};
use jag_surface::{Canvas, ImageFitMode, ImageResourceBindings, JagSurface};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
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
struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        static ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "jag-image-bindings-{}-{}",
            std::process::id(),
            ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn png(&self, name: &str, rgba: [u8; 4]) -> PathBuf {
        let path = self.0.join(name);
        image::RgbaImage::from_pixel(10, 10, image::Rgba(rgba))
            .save(&path)
            .unwrap();
        path
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn pixel(surface: &mut JagSurface, canvas: &Canvas, x: u32, y: u32) -> [u8; 4] {
    let (w, _, p) = surface.end_frame_headless(canvas.clone()).unwrap();
    let i = ((y * w + x) * 4) as usize;
    p[i..i + 4].try_into().unwrap()
}
fn await_pixel(
    surface: &mut JagSurface,
    canvas: &Canvas,
    x: u32,
    y: u32,
    predicate: impl Fn([u8; 4]) -> bool,
) -> [u8; 4] {
    let start = Instant::now();
    loop {
        let color = pixel(surface, canvas, x, y);
        if predicate(color) {
            return color;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "pixel {color:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn scene_id(canvas: &Canvas) -> u64 {
    match &canvas.display_list().commands[0] {
        Command::DrawScrollScene { scene, .. } => scene.id,
        _ => panic!("expected retained scene"),
    }
}
#[test]
fn completed_bindings_preserve_retained_clipping_opacity_and_scene_identity() {
    let mut surface = surface();
    let files = Files::new();
    let red = files.png("red.png", [255, 0, 0, 255]);
    let green = files.png("green.png", [0, 255, 0, 255]);
    for dpr in [1., 2.] {
        surface.set_dpi_scale(dpr);
        let mut c = surface.begin_frame((80. * dpr) as u32, (60. * dpr) as u32);
        c.clear(ColorLinPremul::from_srgba_u8([255; 4]));
        let capture = c.begin_scroll_scene_capture();
        c.push_scroll_layer("media", [0., 0.]);
        c.push_opacity(0.5);
        c.push_clip_rect(Rect {
            x: 5.,
            y: 5.,
            w: 35.,
            h: 35.,
        });
        let identity = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ];
        c.push_spatial_layer(identity, identity, false, false);
        c.draw_image(
            "https://images.test/one",
            [0., 0.],
            [70., 50.],
            ImageFitMode::Fill,
            1,
        );
        c.pop_spatial_layer();
        c.pop_clip();
        c.pop_opacity();
        c.pop_scroll_layer();
        assert!(c.finish_scroll_scene_commit(capture));
        let original_id = scene_id(&c);
        let x = (10. * dpr) as u32;
        let y = (10. * dpr) as u32;
        assert_eq!(pixel(&mut surface, &c, x, y), [255; 4]);
        let mut bindings = ImageResourceBindings::default();
        bindings.bind("https://images.test/one".into(), red.clone());
        let resolved = c.with_image_resources(&mut bindings);
        let again = c.with_image_resources(&mut bindings);
        assert_ne!(scene_id(&resolved), original_id);
        assert_eq!(scene_id(&resolved), scene_id(&again));
        assert_eq!(scene_id(&c), original_id);
        await_pixel(&mut surface, &resolved, x, y, |[r, g, b, _]| {
            r > 240 && g > 100 && g < 210 && b > 100 && b < 210
        });
        assert_eq!(
            pixel(&mut surface, &resolved, (60. * dpr) as u32, y),
            [255; 4]
        );
        bindings.bind("https://images.test/one".into(), green.clone());
        let changed = c.with_image_resources(&mut bindings);
        assert_ne!(scene_id(&resolved), scene_id(&changed));
        await_pixel(&mut surface, &changed, x, y, |[r, g, b, _]| {
            g > 240 && r > 100 && r < 210 && b > 100 && b < 210
        });
        bindings.clear();
        assert_eq!(
            scene_id(&c.with_image_resources(&mut bindings)),
            original_id
        );
    }
}
#[test]
fn side_channel_and_mime_discovered_svg_images_bind_without_moving_boxes() {
    let mut surface = surface();
    let files = Files::new();
    let red = files.png("side.png", [255, 0, 0, 255]);
    let svg = files.0.join("discovered.svg");
    std::fs::write(&svg,"<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'><rect width='10' height='10' fill='blue'/></svg>").unwrap();
    let mut c = surface.begin_frame(80, 40);
    c.clear(ColorLinPremul::from_srgba_u8([255; 4]));
    c.draw_image(
        "https://images.test/one",
        [0., 0.],
        [20., 20.],
        ImageFitMode::Fill,
        1,
    );
    c.draw_image(
        "https://images.test/no-extension",
        [30., 0.],
        [20., 20.],
        ImageFitMode::Fill,
        2,
    );
    let mut bindings = ImageResourceBindings::default();
    bindings.bind("https://images.test/one".into(), red);
    bindings.bind("https://images.test/no-extension".into(), svg);
    let resolved = c.with_image_resources(&mut bindings);
    await_pixel(&mut surface, &resolved, 10, 10, |p| p == [255, 0, 0, 255]);
    await_pixel(&mut surface, &resolved, 40, 10, |p| p == [0, 0, 255, 255]);
    assert_eq!(pixel(&mut surface, &resolved, 65, 10), [255; 4]);
}
