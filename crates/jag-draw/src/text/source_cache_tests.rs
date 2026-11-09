use super::*;
use crate::{ColorLinPremul, FontStyle, TextRun, TextSourceSlice};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Provider(AtomicUsize);
impl TextProvider for Provider {
    fn rasterize_run(&self, _: &TextRun) -> Vec<RasterizedGlyph> {
        self.0.fetch_add(1, Ordering::Relaxed);
        vec![RasterizedGlyph {
            source: Some(0..3),
            offset: [0., 0.],
            mask: GlyphMask::Color(ColorMask {
                width: 10,
                height: 1,
                data: vec![255; 40],
            }),
        }]
    }
}

#[test]
fn source_fragments_reuse_masks_with_distinct_range_clip_and_device_size_keys() {
    let provider = Provider(AtomicUsize::new(0));
    let mut run = TextRun {
        features: Vec::new(),
        spacing: None,
        rtl: None,
        source: Some(TextSourceSlice {
            bytes: 0..1,
            clip_start: None,
            clip_end: Some(0.5),
        }),
        text: "source cache regression".into(),
        pos: [0., 0.],
        size: 10.,
        logical_size: 10.,
        weight: 400.,
        style: FontStyle::Normal,
        family: None,
        color: ColorLinPremul::from_srgba_u8([0, 0, 0, 255]),
    };
    let first = rasterize_run_cached(&provider, &run);
    assert_eq!(first[0].mask.width(), 5);
    assert!(Arc::ptr_eq(&first, &rasterize_run_cached(&provider, &run)));
    run.source.as_mut().unwrap().clip_end = Some(0.7);
    let wider = rasterize_run_cached(&provider, &run);
    assert_eq!(wider[0].mask.width(), 7);
    assert!(!Arc::ptr_eq(&first, &wider));
    run.source.as_mut().unwrap().bytes = 3..4;
    assert!(rasterize_run_cached(&provider, &run).is_empty());
    assert_eq!(
        provider.0.load(Ordering::Relaxed),
        1,
        "fragments share shaping context"
    );
    run.source.as_mut().unwrap().bytes = 0..1;
    run.size = 20.;
    assert_eq!(rasterize_run_cached(&provider, &run)[0].mask.width(), 10);
    assert_eq!(provider.0.load(Ordering::Relaxed), 2);
    invalidate_glyph_run_cache();
    run.size = 10.;
    let refreshed = rasterize_run_cached(&provider, &run);
    assert!(!Arc::ptr_eq(&wider, &refreshed));
    assert_eq!(provider.0.load(Ordering::Relaxed), 3);
}
