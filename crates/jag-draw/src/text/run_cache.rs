// Bound retained raster bytes as well as run count. Navigation may introduce
// different paragraphs with very different mask sizes; an entry limit alone
// leaves that storage uncontrolled. Callers keep independent Arc leases.
const GLYPH_RUN_CACHE_BYTES: usize = 16 * 1024 * 1024;

struct CachedGlyphRun {
    glyphs: std::sync::Arc<Vec<RasterizedGlyph>>,
    bytes: usize,
    used: u64,
}

#[derive(Default)]
struct GlyphRunState {
    map: std::collections::HashMap<GlyphRunKey, CachedGlyphRun>,
    bytes: usize,
    clock: u64,
}

struct GlyphRunCache {
    state: std::sync::Mutex<GlyphRunState>,
    max_entries: usize,
}

impl GlyphRunCache {
    fn new(max_entries: usize) -> Self {
        Self {
            state: std::sync::Mutex::new(GlyphRunState::default()),
            max_entries: max_entries.max(1),
        }
    }

    fn get(&self, key: &GlyphRunKey) -> Option<std::sync::Arc<Vec<RasterizedGlyph>>> {
        let mut state = self.state.lock().unwrap();
        state.clock = state.clock.saturating_add(1);
        let used = state.clock;
        let entry = state.map.get_mut(key)?;
        entry.used = used;
        Some(entry.glyphs.clone())
    }

    fn insert(
        &self,
        key: GlyphRunKey,
        glyphs: Vec<RasterizedGlyph>,
    ) -> std::sync::Arc<Vec<RasterizedGlyph>> {
        let mut state = self.state.lock().unwrap();
        state.clock = state.clock.saturating_add(1);
        let used = state.clock;
        if let Some(entry) = state.map.get_mut(&key) {
            entry.used = used;
            return entry.glyphs.clone();
        }
        let bytes = glyph_run_bytes(&key, &glyphs, glyphs.capacity());
        let glyphs = std::sync::Arc::new(glyphs);
        // A large answer still renders; it must not displace the entire hot
        // working set or become an unbounded cache entry.
        if bytes > GLYPH_RUN_CACHE_BYTES {
            return glyphs;
        }
        while state.map.len() >= self.max_entries
            || state.bytes.saturating_add(bytes) > GLYPH_RUN_CACHE_BYTES
        {
            let Some(oldest) = state
                .map
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            let removed = state.map.remove(&oldest).unwrap();
            state.bytes -= removed.bytes;
        }
        state.bytes += bytes;
        state.map.insert(
            key,
            CachedGlyphRun {
                glyphs: glyphs.clone(),
                bytes,
                used,
            },
        );
        glyphs
    }

    fn clear(&self) {
        *self.state.lock().unwrap() = GlyphRunState::default();
    }
}

fn glyph_run_bytes(key: &GlyphRunKey, glyphs: &[RasterizedGlyph], capacity: usize) -> usize {
    let masks = glyphs.iter().fold(0usize, |bytes, glyph| {
        let size = match &glyph.mask {
            GlyphMask::Subpixel(mask) => mask.data.capacity(),
            GlyphMask::Color(mask) => mask.data.capacity(),
        };
        bytes.saturating_add(size)
    });
    masks
        .saturating_add(capacity.saturating_mul(std::mem::size_of::<RasterizedGlyph>()))
        .saturating_add(
            key.features
                .capacity()
                .saturating_mul(std::mem::size_of::<crate::scene::TextFeature>()),
        )
        .saturating_add(std::mem::size_of::<GlyphRunKey>() + std::mem::size_of::<CachedGlyphRun>())
}
