use super::*;
use jag_draw::{
    FilterEffect,
    spatial::{self, Matrix4},
};
fn matrix(x: f32, y: f32) -> [f32; 16] {
    spatial::affine(Transform2D::translate(x, y)).to_array()
}

#[test]
fn coplanar_child_backface_keeps_accumulated_parent_orientation() {
    let (mut surface, _) = surfaces();
    surface.set_logical_pixels(true);
    for facing in [1., -1.] {
        let mut parent = matrix(if facing < 0. { 120. } else { 0. }, 0.);
        parent[0] = facing;
        parent[10] = facing;
        let mut c = surface.begin_frame(120, 120);
        c.clear(ColorLinPremul::from_srgba_u8([255; 4]));
        c.push_spatial_layer(parent, Matrix4::identity().to_array(), true, false);
        c.push_spatial_layer(matrix(0., 0.), Matrix4::identity().to_array(), false, true);
        c.fill_rect(
            20.,
            20.,
            80.,
            80.,
            Brush::Solid(ColorLinPremul::from_srgba_u8([255, 0, 0, 255])),
            1,
        );
        c.pop_spatial_layer();
        c.pop_spatial_layer();
        let (_, _, pixels) = surface.end_frame_headless(c).unwrap();
        let expected = if facing > 0. {
            [255, 0, 0, 255]
        } else {
            [255; 4]
        };
        assert_eq!(&pixels[(60 * 120 + 60) * 4..][..4], &expected);
    }
}

#[test]
fn intersecting_spatial_planes_keep_per_pixel_depth() {
    let (mut retained, mut fresh) = surfaces();
    for surface in [&mut retained, &mut fresh] {
        surface.set_logical_pixels(true);
        let mut c = surface.begin_frame(120, 120);
        c.clear(ColorLinPremul::from_srgba_u8([255; 4]));
        c.push_spatial_layer(matrix(0., 0.), Matrix4::identity().to_array(), true, false);
        for (slope, color, z) in [(0.25, [255, 0, 0, 255], 1), (-0.25, [0, 0, 255, 255], 2)] {
            let mut transform = matrix(0., 0.);
            transform[2] = slope;
            transform[14] = -60. * slope;
            c.push_spatial_layer(transform, Matrix4::identity().to_array(), false, false);
            c.fill_rect(
                20.,
                20.,
                80.,
                80.,
                Brush::Solid(ColorLinPremul::from_srgba_u8(color)),
                z,
            );
            c.pop_spatial_layer();
        }
        c.pop_spatial_layer();
        let (_, _, pixels) = surface.end_frame_headless(c).unwrap();
        for (x, color) in [(40, [0, 0, 255, 255]), (80, [255, 0, 0, 255])] {
            assert_eq!(&pixels[(60 * 120 + x) * 4..][..4], &color);
        }
    }
}

#[test]
fn animated_detail_reuses_static_spatial_tiles_without_changing_pixels() {
    let (mut retained, mut fresh) = surfaces();
    for surface in [&mut retained, &mut fresh] {
        surface.set_logical_pixels(true);
    }
    let render = |surface: &mut JagSurface, red: u8| {
        let mut c = surface.begin_frame(1200, 180);
        c.clear(ColorLinPremul::from_srgba_u8([255; 4]));
        c.push_spatial_layer(
            matrix(0., -10000.),
            Matrix4::identity().to_array(),
            false,
            false,
        );
        for i in 0..12 {
            c.fill_rect(
                i as f32 * 90. + 8.,
                10020.,
                70.,
                100.,
                Brush::Solid(ColorLinPremul::from_srgba_u8([20, 40, 80, 255])),
                1,
            );
        }
        c.fill_rect(
            20.,
            10030.,
            10.,
            10.,
            Brush::Solid(ColorLinPremul::from_srgba_u8([red, 0, 0, 255])),
            2,
        );
        c.pop_spatial_layer();
        surface.end_frame_headless(c).unwrap().2
    };
    for red in [100, 200, 150] {
        assert_eq!(render(&mut retained, red), render(&mut fresh, red));
        if red != 100 {
            let stats = retained.retained_layer_stats();
            assert!(stats.hits >= 2 && stats.reused_pixels > 50000, "{stats:?}");
        }
    }
}
#[test]
fn spatial_visibility_uses_final_coordinates_and_keeps_filter_outsets() {
    let (mut surface, _) = surfaces();
    surface.set_logical_pixels(true);
    surface.set_web_color_composition(true);
    let white = ColorLinPremul::from_srgba_u8([255; 4]);
    for preserve in [false, true] {
        let mut c = surface.begin_frame(80, 60);
        c.clear(white);
        c.push_spatial_layer(
            matrix(1000., 0.),
            Matrix4::identity().to_array(),
            preserve,
            false,
        );
        c.fill_rect(
            0.,
            0.,
            20.,
            20.,
            Brush::Solid(ColorLinPremul::from_srgba_u8([0, 0, 255, 255])),
            1,
        );
        c.pop_spatial_layer();
        let (_, _, pixels) = surface.end_frame_headless(c).unwrap();
        assert!(pixels.chunks_exact(4).all(|p| p == [255; 4]));
        assert_eq!(surface.retained_layer_stats().rasterized_pixels, 0);
        let mut c = surface.begin_frame(80, 60);
        c.clear(white);
        c.push_spatial_layer(
            matrix(-1000., 0.),
            Matrix4::identity().to_array(),
            preserve,
            false,
        );
        c.push_spatial_layer(
            matrix(1030., 10.),
            Matrix4::identity().to_array(),
            false,
            false,
        );
        c.fill_rect(
            0.,
            0.,
            20.,
            20.,
            Brush::Solid(ColorLinPremul::from_srgba_u8([0, 0, 255, 255])),
            1,
        );
        c.pop_spatial_layer();
        c.pop_spatial_layer();
        let (_, _, pixels) = surface.end_frame_headless(c).unwrap();
        assert_eq!(
            &pixels[(20 * 80 + 40) * 4..(20 * 80 + 40) * 4 + 4],
            &[0, 0, 255, 255]
        );
    }
    let mut c = surface.begin_frame(80, 60);
    c.clear(white);
    c.push_spatial_layer(matrix(0., 0.), Matrix4::identity().to_array(), false, false);
    c.push_filter(FilterEffect::Blur(10.));
    c.push_spatial_layer(
        matrix(-18., 10.),
        Matrix4::identity().to_array(),
        false,
        false,
    );
    c.fill_rect(
        0.,
        0.,
        12.,
        30.,
        Brush::Solid(ColorLinPremul::from_srgba_u8([0, 0, 255, 255])),
        1,
    );
    c.pop_spatial_layer();
    c.pop_filter();
    c.pop_spatial_layer();
    let (_, _, pixels) = surface.end_frame_headless(c).unwrap();
    let p = &pixels[(25 * 80 + 1) * 4..(25 * 80 + 1) * 4 + 4];
    assert!(p[0] < 255 && p[2] == 255, "blur must reach viewport: {p:?}");
}
