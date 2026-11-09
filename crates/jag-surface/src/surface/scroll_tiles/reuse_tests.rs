use super::*;
use jag_draw::{Brush, ColorLinPremul, Painter, Viewport};
use std::sync::atomic::{AtomicU64, Ordering};

struct Provider(AtomicU64);
impl TextProvider for Provider {
    fn rasterize_run(&self, _: &jag_draw::TextRun) -> Vec<jag_draw::RasterizedGlyph> {
        Vec::new()
    }
    fn cache_tag(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

fn scene(
    offset: f32,
    changed: bool,
    dpi: f32,
    provider: Arc<Provider>,
) -> Arc<jag_draw::ScrollScene> {
    let base = Transform2D::translate(0., -offset);
    let mut painter = Painter::begin_frame(Viewport {
        width: 200,
        height: 200,
    });
    painter.push_transform(base);
    painter.rect(
        Rect {
            x: 10.,
            y: 20.,
            w: 40.,
            h: 30.,
        },
        Brush::Solid(ColorLinPremul::from_srgba_u8([255, 0, 0, 255])),
        0,
    );
    painter.pop_transform();
    let mut commands = vec![Command::PushScrollLayer {
        key: "document".into(),
        origin: [0., -offset],
        binding: jag_draw::ScrollBinding {
            key: "scroll".into(),
            value: [0., offset],
            factor: [-1., -1.],
            basis: [1., 0., 0., 1.],
        },
    }];
    commands.extend(painter.display_list().commands.iter().cloned());
    commands.push(Command::PushOpacity(if changed { 0.6 } else { 0.8 }));
    let mut painter = Painter::begin_frame(Viewport {
        width: 200,
        height: 200,
    });
    painter.push_transform(base);
    painter.rect(
        Rect {
            x: 10.,
            y: 70.,
            w: if changed { 80. } else { 40. },
            h: 30.,
        },
        Brush::Solid(ColorLinPremul::from_srgba_u8([0, 0, 255, 255])),
        1,
    );
    painter.pop_transform();
    commands.extend(painter.display_list().commands.iter().cloned());
    commands.push(Command::PopOpacity);
    commands.push(Command::PopScrollLayer);
    Arc::new(jag_draw::ScrollScene::new(
        commands.into(),
        base,
        dpi,
        Some(provider),
    ))
}

#[test]
fn fresh_commits_reuse_unchanged_content_space_runs_and_recompile_changed_runs() {
    let provider = Arc::new(Provider(AtomicU64::new(0)));
    let first = CompiledScene::new(scene(0., false, 2., provider.clone()), 2., None).unwrap();
    let moved =
        CompiledScene::new(scene(45., false, 2., provider.clone()), 2., Some(&first)).unwrap();
    assert_ne!(first.source.id, moved.source.id);
    assert_eq!(first.runs.len(), 2);
    for (a, b) in first.runs.iter().zip(&moved.runs) {
        assert!(
            Arc::ptr_eq(a, b),
            "scroll-only commit must reuse ink bounds and tile bins"
        );
    }
    let changed = CompiledScene::new(scene(50., true, 2., provider), 2., Some(&moved)).unwrap();
    assert!(Arc::ptr_eq(&moved.runs[0], &changed.runs[0]));
    assert!(!Arc::ptr_eq(&moved.runs[1], &changed.runs[1]));
    assert_eq!(changed.runs[1].tight.unwrap().w, 80.);
    assert!(changed.ops.iter().any(|op| matches!(op,
        Op::Draw { command, .. } if matches!(&**command, Command::PushOpacity(x) if *x == 0.6))));
}

#[test]
fn run_reuse_respects_font_generation_provider_dpi_and_raster_scale() {
    let provider = Arc::new(Provider(AtomicU64::new(0)));
    let first = CompiledScene::new(scene(0., false, 2., provider.clone()), 2., None).unwrap();
    for (dpi, scale) in [(1., 2.), (2., 1.)] {
        let changed =
            CompiledScene::new(scene(0., false, dpi, provider.clone()), scale, Some(&first))
                .unwrap();
        assert!(!Arc::ptr_eq(&first.runs[0], &changed.runs[0]));
    }
    let other = Arc::new(Provider(AtomicU64::new(0)));
    let changed = CompiledScene::new(scene(0., false, 2., other), 2., Some(&first)).unwrap();
    assert!(!Arc::ptr_eq(&first.runs[0], &changed.runs[0]));
    provider.0.store(1, Ordering::Relaxed);
    let changed = CompiledScene::new(scene(0., false, 2., provider), 2., Some(&first)).unwrap();
    assert!(!Arc::ptr_eq(&first.runs[0], &changed.runs[0]));
}
