/// Minimal shaped glyph information for paragraph-level wrapping.
#[derive(Clone, Debug)]
pub struct ShapedGlyph {
    /// Glyph's starting UTF-8 byte index in the source text (Harfbuzz cluster).
    pub cluster: u32,
    /// Advance width in pixels.
    pub x_advance: f32,
}

/// Shaped paragraph representation for efficient wrapping.
#[derive(Clone, Debug)]
pub struct ShapedParagraph {
    pub glyphs: Vec<ShapedGlyph>,
}

/// Text provider interface. Implementations convert a `TextRun` into positioned glyph masks.
pub trait TextProvider: Send + Sync {
    fn rasterize_run(&self, run: &crate::scene::TextRun) -> Vec<RasterizedGlyph>;

    /// Optional paragraph shaping hook for advanced wrappers.
    ///
    /// Implementors that can expose shaping results should
    /// return glyphs with cluster indices and advances. The default implementation
    /// returns `None`, in which case callers must fall back to approximate methods.
    fn shape_paragraph(&self, _text: &str, _px: f32) -> Option<ShapedParagraph> {
        None
    }

    /// Shape with exactly the font selection and variations used by rasterize_run.
    /// Providers exposing this hook must retain UTF-8 source ranges on glyph masks
    /// so paint can select a source slice without shaping it again.
    /// None means this provider cannot expose styled shaping; callers retain
    /// their scalar-measurement path rather than using the wrong default face.
    fn shape_styled_run(&self, _run: &crate::scene::TextRun) -> Option<ShapedParagraph> {
        None
    }

    /// Optional cache tag to distinguish providers in text caches.
    /// The default implementation returns 0, which is sufficient when
    /// a single provider is used with a given PassManager.
    fn cache_tag(&self) -> u64 {
        0
    }

    fn line_metrics(&self, px: f32) -> Option<LineMetrics> {
        let _ = px;
        None
    }

    /// Line metrics for a specific font family + weight + style.
    /// Default falls back to `line_metrics()` (ignoring family/weight/style).
    fn line_metrics_for_style(
        &self,
        px: f32,
        _family: Option<&str>,
        _weight: u16,
        _style: crate::scene::FontStyle,
    ) -> Option<LineMetrics> {
        self.line_metrics(px)
    }

    /// Measure the total advance width of a styled text run (in the same pixel
    /// units as `run.size`).  The default delegates to `shape_paragraph`,
    /// ignoring weight/style/family.  Providers that support multiple font faces
    /// should override this to select the correct face.
    fn measure_run(&self, run: &crate::scene::TextRun) -> f32 {
        if let Some(shaped) = self.shape_paragraph(&run.text, run.size) {
            shaped
                .glyphs
                .iter()
                .map(|g| g.x_advance)
                .sum::<f32>()
                .max(0.0)
        } else {
            run.text.chars().count() as f32 * run.size * 0.55
        }
    }

    /// Register a web font from raw TTF/OTF bytes.
    /// Returns `Ok(true)` if newly registered, `Ok(false)` if already present.
    /// Default implementation returns `Ok(false)` (no-op).
    fn register_web_font(
        &self,
        _family: &str,
        _data: Vec<u8>,
        _weight: u16,
        _style: crate::scene::FontStyle,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }
}

/// Rasterize a text run using a global glyph-run cache.
///
/// This is intended for direct text rendering paths that repeatedly render the
/// same text (e.g., during scrolling) and want to avoid re-shaping and
/// re-rasterizing glyphs every frame. The cache key is based on:
/// - text contents
/// - run size in pixels
/// - the concrete text provider instance
pub fn rasterize_run_cached(
    provider: &dyn TextProvider,
    run: &crate::scene::TextRun,
) -> std::sync::Arc<Vec<RasterizedGlyph>> {
    use crate::scene::FontStyle as SceneFontStyle;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;

    let mut hasher = DefaultHasher::new();
    run.text.hash(&mut hasher);
    let text_hash = hasher.finish();

    // Encode style and family so that bold/italic/monospace runs don't
    // collide in the cache with regular text of the same contents.
    let style_bits: u8 = match run.style {
        SceneFontStyle::Normal => 0,
        SceneFontStyle::Italic => 1,
        SceneFontStyle::Oblique => 2,
    };

    let family_hash: u64 = if let Some(ref family) = run.family {
        let mut fh = DefaultHasher::new();
        family.hash(&mut fh);
        fh.finish()
    } else {
        0
    };
    let size_bits = run.size.to_bits();
    let logical_size_bits = run.logical_size.to_bits();
    let weight_bits = run.weight.to_bits();
    // Use the concrete provider data pointer as a stable identifier for this run.
    let provider_id = (provider as *const dyn TextProvider as *const ()) as usize;
    let key = GlyphRunKey {
        source: run.source.as_ref().map(|source| (
            source.bytes.start, source.bytes.end,
            source.clip_start.map(f32::to_bits), source.clip_end.map(f32::to_bits),
        )),
        features: run.features.clone(),
        spacing: run.spacing.map(|s| [s.letter.to_bits(), s.word.to_bits()]),
        rtl: run.rtl,
        text_hash,
        size_bits,
        logical_size_bits,
        weight_bits,
        style_bits,
        family_hash,
        provider_id,
    };

    let cache = global_glyph_run_cache();
    if let Some(hit) = cache.get(&key) {
        return hit;
    }

    // Source fragments share the paragraph's shaping context. Retain the
    // selected masks too: re-slicing on every paint cloned all mask bytes even
    // when the paragraph glyphs were already cached.
    let glyphs = if let Some(source) = &run.source {
        let mut context = run.clone();
        context.source = None;
        let glyphs = rasterize_run_cached(provider, &context);
        slice_glyphs(&glyphs, source, run.size)
    } else {
        provider.rasterize_run(run)
    };
    cache.insert(key, glyphs)
}

/// LEGACY: Simple fontdue-based provider.
///
/// **NOT RECOMMENDED**: Use [`JagTextProvider`] (harfrust + swash) instead.
/// This provider is kept for compatibility and testing purposes only.
///
/// Limitations:
/// - Basic ASCII-first layout
/// - No advanced shaping features
/// - Lower quality than swash rasterization
pub struct SimpleFontdueProvider {
    font: fontdue::Font,
    orientation: SubpixelOrientation,
}

impl SimpleFontdueProvider {
    pub fn from_bytes(bytes: &[u8], orientation: SubpixelOrientation) -> anyhow::Result<Self> {
        let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(Self { font, orientation })
    }
}

impl TextProvider for SimpleFontdueProvider {
    fn rasterize_run(&self, run: &crate::scene::TextRun) -> Vec<RasterizedGlyph> {
        use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
        let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
        layout.reset(&LayoutSettings {
            x: 0.0,
            y: 0.0,
            ..LayoutSettings::default()
        });
        layout.append(
            &[&self.font],
            &TextStyle::new(&run.text, run.size.max(1.0), 0),
        );

        let mut out = Vec::new();
        for g in layout.glyphs() {
            // Rasterize individual glyph to grayscale
            let (metrics, bitmap) = self.font.rasterize_indexed(g.key.glyph_index, g.key.px);
            if metrics.width == 0 || metrics.height == 0 {
                continue;
            }
            // Convert to subpixel mask
            let mask = GlyphMask::Subpixel(grayscale_to_subpixel_rgb(
                metrics.width as u32,
                metrics.height as u32,
                &bitmap,
                self.orientation,
            ));
            // Layout already provides the glyph's top-left (x, y) in pixel space for the
            // chosen CoordinateSystem. Using those directly avoids double-applying the
            // font bearing which would incorrectly shift glyphs vertically (clipping
            // descenders). We keep offsets relative to the run's origin; PassManager
            // snaps the run once using line metrics.
            let ox = g.x;
            let oy = g.y;
            out.push(RasterizedGlyph {
                source: None,
                offset: [ox, oy],
                mask,
            });
        }
        out
    }
    fn line_metrics(&self, px: f32) -> Option<LineMetrics> {
        self.font.horizontal_line_metrics(px).map(|lm| {
            let ascent = lm.ascent;
            // Fontdue typically reports descent as a negative number; normalize to positive magnitude.
            let descent = lm.descent.abs();
            let line_gap = lm.line_gap.max(0.0);
            LineMetrics {
                ascent,
                descent,
                line_gap,
            }
        })
    }
}

/// LEGACY: Grayscale fontdue provider.
///
/// **NOT RECOMMENDED**: Use [`JagTextProvider`] (harfrust + swash) instead.
/// This provider is kept for compatibility and testing purposes only.
///
/// Replicates grayscale coverage to RGB channels equally (no subpixel rendering).
pub struct GrayscaleFontdueProvider {
    font: fontdue::Font,
}

impl GrayscaleFontdueProvider {
    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(Self { font })
    }
}

impl TextProvider for GrayscaleFontdueProvider {
    fn rasterize_run(&self, run: &crate::scene::TextRun) -> Vec<RasterizedGlyph> {
        use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
        let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
        layout.reset(&LayoutSettings {
            x: 0.0,
            y: 0.0,
            ..LayoutSettings::default()
        });
        layout.append(
            &[&self.font],
            &TextStyle::new(&run.text, run.size.max(1.0), 0),
        );
        let mut out = Vec::new();
        for g in layout.glyphs() {
            let (metrics, bitmap) = self.font.rasterize_indexed(g.key.glyph_index, g.key.px);
            if metrics.width == 0 || metrics.height == 0 {
                continue;
            }
            let mask = GlyphMask::Subpixel(grayscale_to_rgb_equal(
                metrics.width as u32,
                metrics.height as u32,
                &bitmap,
            ));
            // See note above: use layout-provided top-left directly.
            let ox = g.x;
            let oy = g.y;
            out.push(RasterizedGlyph {
                source: None,
                offset: [ox, oy],
                mask,
            });
        }
        out
    }
    fn line_metrics(&self, px: f32) -> Option<LineMetrics> {
        self.font.horizontal_line_metrics(px).map(|lm| {
            let ascent = lm.ascent;
            let descent = lm.descent.abs();
            let line_gap = lm.line_gap.max(0.0);
            LineMetrics {
                ascent,
                descent,
                line_gap,
            }
        })
    }
}

/// Simplified line metrics
#[derive(Clone, Copy, Debug, Default)]
pub struct LineMetrics {
    pub ascent: f32,
    pub descent: f32,
    pub line_gap: f32,
}

// ---------------------------------------------------------------------------
// CSS font-family parser
// ---------------------------------------------------------------------------

/// A single candidate in a parsed CSS font-family stack.
#[derive(Debug, Clone, PartialEq, Eq)]
enum FontFamilyCandidate {
    /// A specific font name, e.g. `"Georgia"`.
    Name(String),
    /// A CSS generic family keyword.
    Generic(GenericFamily),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum GenericFamily {
    Serif,
    SansSerif,
    Monospace,
    SystemUi,
    Cursive,
    Fantasy,
}

/// Parse a CSS font-family value into an ordered list of candidates.
///
/// Handles quoted names (`"Times New Roman"`, `'Georgia'`), unquoted names,
/// generic keywords (`serif`, `sans-serif`, `monospace`, `system-ui`,
/// `cursive`, `fantasy`), and browser aliases (`-apple-system`,
/// `BlinkMacSystemFont`).
fn parse_font_family_stack(css_value: &str) -> Vec<FontFamilyCandidate> {
    let mut result = Vec::new();
    for part in css_value.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Strip surrounding quotes
        let name = if (trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        {
            &trimmed[1..trimmed.len() - 1]
        } else {
            trimmed
        };
        let lower = name.to_ascii_lowercase();
        let candidate = match lower.as_str() {
            "serif" | "ui-serif" => FontFamilyCandidate::Generic(GenericFamily::Serif),
            "sans-serif" | "ui-sans-serif" => {
                FontFamilyCandidate::Generic(GenericFamily::SansSerif)
            }
            "monospace" | "ui-monospace" => FontFamilyCandidate::Generic(GenericFamily::Monospace),
            "system-ui" | "-apple-system" | "blinkmacswissfont" | "blinkmacsystemfont" => {
                FontFamilyCandidate::Generic(GenericFamily::SystemUi)
            }
            "cursive" => FontFamilyCandidate::Generic(GenericFamily::Cursive),
            "fantasy" => FontFamilyCandidate::Generic(GenericFamily::Fantasy),
            _ => FontFamilyCandidate::Name(name.to_string()),
        };
        result.push(candidate);
    }
    result
}

// ---------------------------------------------------------------------------
// Cached font set for a resolved family
// ---------------------------------------------------------------------------

/// Font faces for a single resolved font family (regular + weight/style variants).
#[derive(Clone)]
struct CachedFontSet {
    /// Upright (non-italic) faces keyed by CSS font-weight.
    upright_faces: Vec<(u16, jag_text::FontFace)>,
    /// Italic/oblique faces keyed by CSS font-weight.
    italic_faces: Vec<(u16, jag_text::FontFace)>,
}

#[derive(Clone)]
struct TextFaceSegment {
    start: usize,
    end: usize,
    face: jag_text::FontFace,
}

// ---------------------------------------------------------------------------
// JagTextProvider
// ---------------------------------------------------------------------------

/// Text provider backed by jag-text (HarfBuzz) for shaping and swash for rasterization.
///
/// This uses a primary `jag-text` `FontFace` for text and an optional emoji font
/// for color emoji fallback. Delegates shaping to `TextShaper::shape_ltr`, then
/// rasterizes glyphs via swash bitmap images.
///
/// Supports CSS font-family stack resolution: when a `TextRun` specifies a
/// `family` string (e.g. `"Georgia, 'Times New Roman', serif"`), the provider
/// parses the stack and resolves candidates against the system font database,
/// caching loaded fonts for subsequent requests.
pub struct JagTextProvider {
    /// Primary (regular) text font.
    font: jag_text::FontFace,
    /// Optional bold/semibold face for heavier weights.
    bold_font: Option<jag_text::FontFace>,
    /// Optional italic face for slanted text.
    italic_font: Option<jag_text::FontFace>,
    /// Optional monospace font for code spans.
    mono_font: Option<jag_text::FontFace>,
    /// Optional emoji font for fallback when primary font lacks emoji glyphs.
    emoji_font: std::sync::OnceLock<Option<jag_text::FontFace>>,
    orientation: SubpixelOrientation,
    antialiasing: TextAntialiasing,
    /// System font database kept alive for on-demand font resolution.
    /// `None` when the provider was constructed from raw bytes.
    font_db: Option<fontdb::Database>,
    /// Cache of resolved font families, keyed by lowercase family name or
    /// generic family keyword. Protected by a `Mutex` because the
    /// `TextProvider` trait takes `&self`.
    font_cache: std::sync::Mutex<std::collections::HashMap<String, CachedFontSet>>,
    /// Cache of system fallback faces by source character. The primary CSS
    /// face often resolves to Arial/System UI, which does not cover Indic
    /// scripts used by google.com language links.
    char_fallback_cache:
        std::sync::Mutex<std::collections::HashMap<char, Option<jag_text::FontFace>>>,
    /// Generation counter incremented on each `register_web_font` call.
    /// Used as `cache_tag` to invalidate stale text-width caches after
    /// new fonts are registered.
    font_generation: std::sync::atomic::AtomicU64,
}
