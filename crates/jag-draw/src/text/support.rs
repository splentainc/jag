fn strengthen_coverage(c: f32) -> f32 {
    let coverage = c.clamp(0.0, 1.0);
    if coverage <= 0.0
        || coverage >= 1.0
        || GAMMA_BLEND_COVERAGE.load(std::sync::atomic::Ordering::Relaxed)
    {
        return coverage;
    }

    // Browser text edges are visually closer to sRGB/display-space alpha
    // compositing. On a linear-blend target the text shader blends
    // premultiplied linear color, so a
    // geometric 50% coverage mask becomes perceptually too light on white.
    // Convert the coverage so linear blending produces approximately the same
    // sRGB result while preserving empty/full pixels and glyph bounds.
    1.0 - srgb_to_linear(1.0 - coverage)
}

fn srgb_to_linear(v: f32) -> f32 {
    let value = v.clamp(0.0, 1.0);
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn strengthen_coverage_u8(v: u8) -> u8 {
    let c = strengthen_coverage(v as f32 / 255.0);
    (c * 255.0 + 0.5) as u8
}

fn strengthen_coverage_u16(v: u16) -> u16 {
    let c = strengthen_coverage(v as f32 / 65535.0);
    (c * 65535.0 + 0.5) as u16
}

fn strengthen_subpixel_rgba8(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    for px in out.chunks_exact_mut(4) {
        px[0] = strengthen_coverage_u8(px[0]);
        px[1] = strengthen_coverage_u8(px[1]);
        px[2] = strengthen_coverage_u8(px[2]);
    }
    out
}

fn strengthen_grayscale_subpixel_rgba8(data: &[u8]) -> Vec<u8> {
    let mut out = strengthen_subpixel_rgba8(data);
    for px in out.chunks_exact_mut(4) {
        let coverage = ((u16::from(px[0]) + u16::from(px[1]) + u16::from(px[2])) / 3) as u8;
        px[0] = coverage;
        px[1] = coverage;
        px[2] = coverage;
    }
    out
}

/// Simple global cache for glyph runs keyed by (text, size, weight, style,
/// family, provider pointer). Used by direct text rendering paths (e.g.,
/// jag-surface Canvas) to avoid re-shaping and re-rasterizing identical
/// text on every frame.
#[derive(Hash, Eq, PartialEq, Clone, Debug)]
struct GlyphRunKey {
    source: Option<(usize, usize, Option<u32>, Option<u32>)>,
    features: Vec<crate::scene::TextFeature>,
    spacing: Option<[u32; 2]>,
    rtl: Option<bool>,
    text_hash: u64,
    size_bits: u32,
    logical_size_bits: u32,
    weight_bits: u32,
    style_bits: u8,
    family_hash: u64,
    provider_id: usize,
}

static GLYPH_RUN_CACHE: std::sync::OnceLock<GlyphRunCache> = std::sync::OnceLock::new();

fn global_glyph_run_cache() -> &'static GlyphRunCache {
    GLYPH_RUN_CACHE.get_or_init(|| GlyphRunCache::new(2048))
}

/// Invalidate all cached glyph rasterizations.
///
/// Must be called after web fonts are registered so that text re-renders
/// using the newly available font faces instead of stale cached bitmaps.
pub fn invalidate_glyph_run_cache() {
    global_glyph_run_cache().clear();
}

/// Convert an 8-bit grayscale coverage mask to an RGB subpixel mask.
/// Uses a gentle subpixel shift for improved clarity on small text.
pub fn grayscale_to_subpixel_rgb(
    width: u32,
    height: u32,
    gray: &[u8],
    orientation: SubpixelOrientation,
) -> SubpixelMask {
    let w = width as usize;
    let h = height as usize;
    assert_eq!(gray.len(), w * h);
    let mut out = vec![0u8; w * h * 4];

    // Gentle subpixel rendering: slight horizontal shift per channel
    // Much lighter than the original 3-tap kernel to avoid blurring
    for y in 0..h {
        for x in 0..w {
            let c0 = strengthen_coverage(gray[y * w + x] as f32 / 255.0);
            let cl = if x > 0 {
                strengthen_coverage(gray[y * w + (x - 1)] as f32 / 255.0)
            } else {
                c0
            };
            let cr = if x + 1 < w {
                strengthen_coverage(gray[y * w + (x + 1)] as f32 / 255.0)
            } else {
                c0
            };

            // Very light blending (10% neighbor influence instead of 33%)
            let sample_left = 0.9 * c0 + 0.1 * cl;
            let sample_center = c0;
            let sample_right = 0.9 * c0 + 0.1 * cr;

            let (r_cov, g_cov, b_cov) = match orientation {
                SubpixelOrientation::RGB => (sample_left, sample_center, sample_right),
                SubpixelOrientation::BGR => (sample_right, sample_center, sample_left),
            };

            let i = (y * w + x) * 4;
            out[i + 0] = (r_cov * 255.0 + 0.5) as u8;
            out[i + 1] = (g_cov * 255.0 + 0.5) as u8;
            out[i + 2] = (b_cov * 255.0 + 0.5) as u8;
            out[i + 3] = 0u8; // alpha unused; output premul alpha computed in shader
        }
    }
    SubpixelMask {
        width,
        height,
        format: MaskFormat::Rgba8,
        data: out,
    }
}

/// Convert an 8-bit grayscale coverage mask to an RGB mask with equal channels (grayscale AA).
pub fn grayscale_to_rgb_equal(width: u32, height: u32, gray: &[u8]) -> SubpixelMask {
    let w = width as usize;
    let h = height as usize;
    assert_eq!(gray.len(), w * h);
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let g = strengthen_coverage_u8(gray[y * w + x]);
            let i = (y * w + x) * 4;
            out[i + 0] = g;
            out[i + 1] = g;
            out[i + 2] = g;
            out[i + 3] = 0u8;
        }
    }
    SubpixelMask {
        width,
        height,
        format: MaskFormat::Rgba8,
        data: out,
    }
}

/// 16-bit variants for higher precision masks. Channels are u16 in [0..65535],
/// packed little-endian into the data buffer. Alpha is unused.
pub fn grayscale_to_subpixel_rgb16(
    width: u32,
    height: u32,
    gray: &[u8],
    orientation: SubpixelOrientation,
) -> SubpixelMask {
    let w = width as usize;
    let h = height as usize;
    assert_eq!(gray.len(), w * h);
    let mut out = vec![0u8; w * h * 8];
    for y in 0..h {
        for x in 0..w {
            let c0 = strengthen_coverage(gray[y * w + x] as f32 / 255.0);
            let cl = if x > 0 {
                strengthen_coverage(gray[y * w + (x - 1)] as f32 / 255.0)
            } else {
                c0
            };
            let cr = if x + 1 < w {
                strengthen_coverage(gray[y * w + (x + 1)] as f32 / 255.0)
            } else {
                c0
            };
            let sample_left = (2.0 / 3.0) * c0 + (1.0 / 3.0) * cl;
            let sample_center = c0;
            let sample_right = (2.0 / 3.0) * c0 + (1.0 / 3.0) * cr;
            let (r_cov, g_cov, b_cov) = match orientation {
                SubpixelOrientation::RGB => (sample_left, sample_center, sample_right),
                SubpixelOrientation::BGR => (sample_right, sample_center, sample_left),
            };
            let (r, g, b) = match orientation {
                SubpixelOrientation::RGB => (r_cov, g_cov, b_cov),
                SubpixelOrientation::BGR => (b_cov, g_cov, r_cov),
            };
            let i = (y * w + x) * 8;
            let write_u16 = |buf: &mut [u8], idx: usize, v: u16| {
                let b = v.to_le_bytes();
                buf[idx] = b[0];
                buf[idx + 1] = b[1];
            };
            write_u16(&mut out, i + 0, (r * 65535.0 + 0.5) as u16);
            write_u16(&mut out, i + 2, (g * 65535.0 + 0.5) as u16);
            write_u16(&mut out, i + 4, (b * 65535.0 + 0.5) as u16);
            write_u16(&mut out, i + 6, 0u16);
        }
    }
    SubpixelMask {
        width,
        height,
        format: MaskFormat::Rgba16,
        data: out,
    }
}

pub fn grayscale_to_rgb_equal16(width: u32, height: u32, gray: &[u8]) -> SubpixelMask {
    let w = width as usize;
    let h = height as usize;
    assert_eq!(gray.len(), w * h);
    let mut out = vec![0u8; w * h * 8];
    for y in 0..h {
        for x in 0..w {
            let g = strengthen_coverage_u16((gray[y * w + x] as u16) * 257); // 255->65535 scale
            let i = (y * w + x) * 8;
            let b = g.to_le_bytes();
            out[i + 0] = b[0];
            out[i + 1] = b[1];
            out[i + 2] = b[0];
            out[i + 3] = b[1];
            out[i + 4] = b[0];
            out[i + 5] = b[1];
            out[i + 6] = 0;
            out[i + 7] = 0;
        }
    }
    SubpixelMask {
        width,
        height,
        format: MaskFormat::Rgba16,
        data: out,
    }
}

// Optional provider that consumes a patched fontdue fork emitting RGB masks directly.
// Behind a feature flag so it doesn't affect default builds.
#[cfg(feature = "fontdue-rgb-patch")]
pub struct PatchedFontdueProvider {
    font: fontdue_rgb::Font,
}

#[cfg(feature = "fontdue-rgb-patch")]
impl PatchedFontdueProvider {
    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        let font = fontdue_rgb::Font::from_bytes(bytes, fontdue_rgb::FontSettings::default())?;
        Ok(Self { font })
    }
}

#[cfg(feature = "fontdue-rgb-patch")]
impl TextProvider for PatchedFontdueProvider {
    fn rasterize_run(&self, run: &crate::scene::TextRun) -> Vec<RasterizedGlyph> {
        use fontdue_rgb::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
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
            // Patched fontdue returns RGB masks directly (u8 or u16). Prefer 16-bit when available.
            let mask = if let Some((w, h, data16)) = self
                .font
                .rasterize_rgb16_indexed(g.key.glyph_index, g.key.px)
            {
                GlyphMask::Subpixel(SubpixelMask {
                    width: w as u32,
                    height: h as u32,
                    format: MaskFormat::Rgba16,
                    data: data16,
                })
            } else {
                let (w, h, data8) = self
                    .font
                    .rasterize_rgb8_indexed(g.key.glyph_index, g.key.px);
                GlyphMask::Subpixel(SubpixelMask {
                    width: w as u32,
                    height: h as u32,
                    format: MaskFormat::Rgba8,
                    data: data8,
                })
            };
            out.push(RasterizedGlyph {
                source: None,
                offset: [g.x, g.y],
                mask,
            });
        }
        out
    }
}

/// A glyph with its top-left offset relative to the run origin and a mask (subpixel or color).
#[derive(Clone, Debug)]
pub struct RasterizedGlyph {
    pub source: Option<core::ops::Range<usize>>,
    pub offset: [f32; 2],
    pub mask: GlyphMask,
}
