use super::*;
use jag_draw::MaskMode;
use std::path::{Path, PathBuf};

struct Asset(PathBuf);
impl Asset {
    fn new(extension: &str) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(std::env::temp_dir().join(format!(
            "jag-mask-{}-{nonce}.{extension}",
            std::process::id()
        )))
    }
    fn svg(&self, right: bool) {
        let x = if right { "20" } else { "0" };
        std::fs::write(&self.0, format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><rect x="{x}" width="20" height="40" fill="white"/></svg>"#)).unwrap();
    }
}
impl Drop for Asset {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn render_mask(s: &mut JagSurface, path: &Path, dpr: f32, offset: f32) -> Vec<u8> {
    s.set_logical_pixels(true);
    s.set_dpi_scale(dpr);
    s.set_web_color_composition(true);
    let mut c = s.begin_frame((96. * dpr) as u32, (96. * dpr) as u32);
    c.clear(ColorLinPremul::from_srgba_u8([255; 4]));
    c.push_transform(Transform2D::translate(offset, offset));
    let rect = Rect {
        x: 0.,
        y: 0.,
        w: 40.,
        h: 40.,
    };
    assert!(c.push_url_mask(path, rect, rect, [40., 40.], [false; 2], MaskMode::Alpha));
    c.fill_rect(
        0.,
        0.,
        40.,
        40.,
        Brush::Solid(ColorLinPremul::from_srgba_u8([0, 0, 255, 255])),
        1,
    );
    c.pop_filter();
    c.pop_transform();
    s.end_frame_headless(c).unwrap().2
}
#[test]
fn loaded_url_masks_reuse_pixels_and_invalidate_on_asset_and_scale_changes() {
    let asset = Asset::new("svg");
    let (mut warm, mut fresh) = surfaces();
    // A missing resource must not freeze its transparent placeholder.
    render_mask(&mut warm, &asset.0, 1., 10.);
    assert_eq!(warm.retained_layer_stats().hits, 0);
    for dpr in [1., 2.] {
        for right in [false, true] {
            asset.svg(right);
            let expected = render_mask(&mut fresh, &asset.0, dpr, 10.);
            assert_eq!(render_mask(&mut warm, &asset.0, dpr, 10.), expected);
            assert_eq!(
                warm.retained_layer_stats().hits,
                0,
                "changed source must rerasterize"
            );
            let moved = render_mask(&mut warm, &asset.0, dpr, 20.);
            assert_eq!(moved, render_mask(&mut fresh, &asset.0, dpr, 20.));
            assert!(
                warm.retained_layer_stats().hits > 0,
                "loaded mask should survive translation"
            );
            assert_eq!(warm.retained_layer_stats().rasterized_pixels, 0);
            let px = ((20. + if right { 30. } else { 10. }) * dpr) as usize;
            let py = (40. * dpr) as usize;
            let i = (py * (96. * dpr) as usize + px) * 4;
            assert_eq!(&moved[i..i + 4], &[0, 0, 255, 255]);
        }
    }
}

#[test]
fn pending_raster_mask_load_and_replacement_do_not_reuse_placeholder_pixels() {
    let asset = Asset::new("png");
    let (mut warm, _) = surfaces();
    let blank = render_mask(&mut warm, &asset.0, 1., 20.);
    assert!(blank.chunks_exact(4).all(|p| p == [255; 4]));
    for right in [false, true] {
        let mut bytes = vec![0; 40 * 40 * 4];
        for y in 0..40 {
            for x in 0..40 {
                if (x >= 20) == right {
                    bytes[(y * 40 + x) * 4..(y * 40 + x + 1) * 4].fill(255);
                }
            }
        }
        image::save_buffer(&asset.0, &bytes, 40, 40, image::ColorType::Rgba8).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let expected_blue = (40 * 96 + if right { 50 } else { 30 }) * 4;
        let expected_white = (40 * 96 + if right { 30 } else { 50 }) * 4;
        let loaded = loop {
            let pixels = render_mask(&mut warm, &asset.0, 1., 20.);
            if pixels[expected_blue..expected_blue + 4] == [0, 0, 255, 255]
                && pixels[expected_white..expected_white + 4] == [255; 4]
            {
                break pixels;
            }
            assert!(std::time::Instant::now() < deadline, "mask never updated");
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(render_mask(&mut warm, &asset.0, 1., 20.), loaded);
        assert!(warm.retained_layer_stats().hits > 0);
        assert_eq!(warm.retained_layer_stats().rasterized_pixels, 0);
    }
    std::fs::remove_file(&asset.0).unwrap();
    assert_eq!(render_mask(&mut warm, &asset.0, 1., 20.), blank);
    assert_eq!(warm.retained_layer_stats().hits, 0);
}
