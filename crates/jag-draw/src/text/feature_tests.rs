use super::*;
use crate::{ColorLinPremul, FontStyle, TextFeature, TextRun};

fn run(scale: f32) -> TextRun {
    TextRun {
        features: Vec::new(),
        source: None,
        spacing: None,
        rtl: None,
        text: "III".into(),
        pos: [0., 0.],
        size: 20. * scale,
        logical_size: 20.,
        weight: 400.,
        style: FontStyle::Normal,
        family: None,
        color: ColorLinPremul::from_srgba_u8([0, 0, 0, 255]),
    }
}

fn fingerprint(glyphs: &[RasterizedGlyph]) -> u64 {
    use std::hash::Hasher;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    for g in glyphs {
        g.offset.map(f32::to_bits).hash(&mut hash);
        match &g.mask {
            GlyphMask::Subpixel(m) => {
                (m.width, m.height).hash(&mut hash);
                m.data.hash(&mut hash);
            }
            GlyphMask::Color(m) => {
                (m.width, m.height).hash(&mut hash);
                m.data.hash(&mut hash);
            }
        }
    }
    hash.finish()
}

#[test]
fn opentype_alternates_change_measurement_and_cached_rasterization_at_each_dpi() {
    let p = JagTextProvider::from_bytes(
        include_bytes!("../../../../fonts/Geist/Geist-VariableFont_wght.ttf"),
        SubpixelOrientation::RGB,
    )
    .unwrap();
    for scale in [1., 2.] {
        let normal = run(scale);
        let mut alternate = normal.clone();
        // The bundled Geist version calls the barred I alternate ss05.
        alternate.features.push(TextFeature {
            tag: *b"ss05",
            value: 1,
        });
        assert!((p.measure_run(&alternate) - p.measure_run(&normal)).abs() > 1.);
        assert!(
            (p.shape_styled_run(&alternate)
                .unwrap()
                .glyphs
                .iter()
                .map(|g| g.x_advance)
                .sum::<f32>()
                - p.measure_run(&alternate))
            .abs()
                < 0.01
        );
        let a = rasterize_run_cached(&p, &normal);
        let b = rasterize_run_cached(&p, &alternate);
        assert_ne!(fingerprint(&a), fingerprint(&b));
        assert!(std::sync::Arc::ptr_eq(
            &b,
            &rasterize_run_cached(&p, &alternate)
        ));
        alternate.features.push(TextFeature {
            tag: *b"ss05",
            value: 0,
        });
        assert_eq!(p.measure_run(&normal), p.measure_run(&alternate));
        assert_eq!(
            fingerprint(&a),
            fingerprint(&rasterize_run_cached(&p, &alternate))
        );
    }
}
