use super::*;

fn clipped_layer(surface: &mut JagSurface, dpr: f32, offset: f32, clip_bottom: f32) -> Vec<u8> {
    surface.set_logical_pixels(true);
    surface.set_dpi_scale(dpr);
    let mut canvas = surface.begin_frame((80. * dpr) as u32, (80. * dpr) as u32);
    canvas.clear(ColorLinPremul::from_srgba_u8([255; 4]));
    canvas.push_clip_rect(Rect {
        x: 0.,
        y: 0.,
        w: 80.,
        h: clip_bottom,
    });
    canvas.push_transform(Transform2D::translate(0., -offset));
    canvas.push_opacity(0.7);
    canvas.fill_rect(
        20.,
        30.,
        20.,
        20.,
        Brush::Solid(ColorLinPremul::from_srgba_u8([200, 20, 40, 255])),
        1,
    );
    canvas.pop_opacity();
    canvas.pop_transform();
    canvas.pop_clip();
    let pixels = surface.end_frame_headless(canvas).unwrap().2;
    surface.pass_manager().clear_external_textures();
    pixels
}

#[test]
fn ancestor_scissor_reuses_visible_pixels_but_invalidates_when_it_cuts_the_layer() {
    let (mut retained, mut fresh) = surfaces();
    for web in [false, true] {
        retained.set_web_color_composition(web);
        fresh.set_web_color_composition(web);
        for dpr in [1., 1.25, 2.] {
            clipped_layer(&mut retained, dpr, 0., 80.);
            for offset in [4., 8., 4.] {
                let actual = clipped_layer(&mut retained, dpr, offset, 80.);
                assert_eq!(actual, clipped_layer(&mut fresh, dpr, offset, 80.));
                let stats = retained.retained_layer_stats();
                assert_eq!(
                    stats.hits, 1,
                    "integer physical translation should reuse: {stats:?}"
                );
                assert_eq!(stats.rasterized_pixels, 0);
            }
            // Same target geometry, but an ancestor now cuts its interior.
            let clipped = clipped_layer(&mut retained, dpr, 4., 35.);
            assert_eq!(clipped, clipped_layer(&mut fresh, dpr, 4., 35.));
            assert_eq!(retained.retained_layer_stats().misses, 1);
            // Fractional physical translation still changes edge coverage.
            let fractional = clipped_layer(&mut retained, dpr, 4.3, 80.);
            assert_eq!(fractional, clipped_layer(&mut fresh, dpr, 4.3, 80.));
            assert_eq!(retained.retained_layer_stats().misses, 1);
        }
    }
}
