// Scroll tile indexing needs the exact ink bounds of offscreen text, but it
// does not need to keep those glyph masks resident in the raster cache.
const GLYPH_INK_CACHE_ENTRIES: usize = 8192;
const MAX_CACHED_INK_FEATURES: usize = 16;

#[derive(Clone, PartialEq, Eq, Hash)]
struct GlyphInkKey {
    run: GlyphRunKey,
    origin: [u32; 2],
    dpi: u32,
    font_tag: u64,
}

#[derive(Default)]
struct GlyphInkCache {
    bounds: std::collections::HashMap<GlyphInkKey, crate::scene::Rect>,
    order: std::collections::VecDeque<GlyphInkKey>,
}

impl GlyphInkCache {
    fn insert(&mut self, key: GlyphInkKey, bounds: crate::scene::Rect) {
        if self.bounds.contains_key(&key) {
            return;
        }
        if self.order.len() == GLYPH_INK_CACHE_ENTRIES
            && let Some(oldest) = self.order.pop_front()
        {
            self.bounds.remove(&oldest);
        }
        self.order.push_back(key.clone());
        self.bounds.insert(key, bounds);
    }
}

static GLYPH_INK_CACHE: std::sync::OnceLock<std::sync::Mutex<GlyphInkCache>> =
    std::sync::OnceLock::new();

/// Return the exact logical ink bounds for a physical-size run rasterized at
/// zero origin and placed at `origin` in logical pixels.
pub fn rasterized_run_ink_bounds(
    provider: &dyn TextProvider,
    run: &crate::scene::TextRun,
    origin: [f32; 2],
    dpi: f32,
) -> crate::scene::Rect {
    ink_bounds_with_cache(
        GLYPH_INK_CACHE.get_or_init(Default::default),
        provider,
        run,
        origin,
        dpi,
    )
}

fn ink_bounds_with_cache(
    cache: &std::sync::Mutex<GlyphInkCache>,
    provider: &dyn TextProvider,
    run: &crate::scene::TextRun,
    origin: [f32; 2],
    dpi: f32,
) -> crate::scene::Rect {
    let measure = || {
        glyph_ink_bounds(
            &rasterize_run_cached(provider, run),
            origin,
            dpi,
            run.logical_size,
        )
    };
    if run.features.len() > MAX_CACHED_INK_FEATURES {
        return measure();
    }
    let key = GlyphInkKey {
        run: glyph_run_key(provider, run),
        origin: origin.map(f32::to_bits),
        dpi: dpi.to_bits(),
        font_tag: provider.cache_tag(),
    };
    if let Some(bounds) = cache.lock().unwrap().bounds.get(&key).copied() {
        return bounds;
    }
    let bounds = measure();
    cache.lock().unwrap().insert(key, bounds);
    bounds
}

fn glyph_ink_bounds(
    glyphs: &[RasterizedGlyph],
    origin: [f32; 2],
    dpi: f32,
    logical_size: f32,
) -> crate::scene::Rect {
    use crate::scene::Rect;

    glyphs
        .iter()
        .map(|glyph| {
            let mut x = origin[0] + glyph.offset[0] / dpi;
            let mut y = origin[1] + glyph.offset[1] / dpi;
            if logical_size <= 15.0 {
                x = (x * dpi).round() / dpi;
                y = (y * dpi).round() / dpi;
            }
            Rect {
                x,
                y,
                w: glyph.mask.width() as f32 / dpi,
                h: glyph.mask.height() as f32 / dpi,
            }
        })
        .reduce(|a, b| {
            let x = a.x.min(b.x);
            let y = a.y.min(b.y);
            Rect {
                x,
                y,
                w: (a.x + a.w).max(b.x + b.w) - x,
                h: (a.y + a.h).max(b.y + b.h) - y,
            }
        })
        .unwrap_or_default()
}
