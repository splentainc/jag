use super::*;
use jag_draw::{MaskComposite, MaskCompositeLayer, MaskMode};

fn masked(
    surface: &mut JagSurface,
    dpr: f32,
    transform: Transform2D,
    mode: MaskMode,
    repeat: bool,
    reversed: bool,
) -> Vec<u8> {
    surface.set_logical_pixels(true);
    surface.set_dpi_scale(dpr);
    surface.set_web_color_composition(true);
    let mut c = surface.begin_frame((120. * dpr) as u32, (120. * dpr) as u32);
    c.clear(ColorLinPremul::from_srgba_u8([255; 4]));
    c.push_transform(transform);
    let paint = Rect {
        x: 0.,
        y: 0.,
        w: 40.,
        h: 40.,
    };
    let tile = Rect { w: 20., ..paint };
    let transparent = ColorLinPremul::from_srgba_u8([0; 4]);
    let white = ColorLinPremul::from_srgba_u8([255; 4]);
    let (a, b) = if reversed {
        (white, transparent)
    } else {
        (transparent, white)
    };
    let brush = Brush::LinearGradient {
        start: [0., 0.],
        end: [20., 0.],
        stops: vec![(0., a), (0.5, a), (0.5, b), (1., b)],
    };
    let mask = c
        .resolve_generated_mask_pattern(paint, tile, [20., 40.], [repeat, false], &brush, mode)
        .unwrap();
    assert!(c.push_mask_group(vec![MaskCompositeLayer {
        mask,
        composite: MaskComposite::Add,
        text_clip: false
    }]));
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
    surface.end_frame_headless(c).unwrap().2
}
#[test]
fn mapped_masks_keep_pixels_during_reuse_transform_mode_repeat_and_source_changes() {
    let (mut warm, mut fresh) = surfaces();
    for dpr in [1., 2.] {
        for transform in [
            Transform2D::translate(40., 40.),
            Transform2D {
                m: [0., 1., -1., 0., 80., 20.],
            },
            Transform2D {
                m: [-1., 0., 0., 1., 80., 20.],
            },
            Transform2D {
                m: [1., 0.3, 0.2, 1., 30., 20.],
            },
        ] {
            for mode in [MaskMode::Alpha, MaskMode::Luminance] {
                for repeat in [false, true] {
                    for reversed in [false, true] {
                        let expected = masked(&mut fresh, dpr, transform, mode, repeat, reversed);
                        masked(&mut warm, dpr, transform, mode, repeat, reversed);
                        let actual = masked(&mut warm, dpr, transform, mode, repeat, reversed);
                        assert_eq!(actual, expected);
                        assert!(warm.retained_layer_stats().hits > 0);
                        let sample = |x: f32| {
                            let [a, b, c, d, e, f] = transform.m;
                            let px = ((a * x + c * 20. + e) * dpr).floor() as usize;
                            let py = ((b * x + d * 20. + f) * dpr).floor() as usize;
                            let i = (py * (120. * dpr) as usize + px) * 4;
                            &actual[i..i + 4]
                        };
                        assert_eq!(
                            sample(5.),
                            if reversed {
                                &[0, 0, 255, 255]
                            } else {
                                &[255; 4]
                            },
                            "left dpr={dpr} transform={transform:?} mode={mode:?} repeat={repeat} reversed={reversed}"
                        );
                        assert_eq!(
                            sample(15.),
                            if reversed {
                                &[255; 4]
                            } else {
                                &[0, 0, 255, 255]
                            },
                            "right dpr={dpr} transform={transform:?} mode={mode:?} repeat={repeat} reversed={reversed}"
                        );
                    }
                }
            }
        }
    }
}
