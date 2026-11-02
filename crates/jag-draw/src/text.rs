//! Text rendering providers for jag-draw.
//!
//! The primary provider is [`JagTextProvider`] which uses:
//! - `harfrust` for text shaping (HarfBuzz implementation)
//! - `swash` for glyph rasterization
//! - `fontdb` for font discovery and fallback
//!
//! This provides high-quality text rendering with:
//! - Proper kerning and ligatures
//! - Subpixel RGB rendering
//! - BiDi support
//! - Complex script support
//!
//! # Example
//! ```no_run
//! use jag_draw::{
//!     JagTextProvider, SubpixelOrientation, TextRun, ColorLinPremul, TextProvider, FontStyle,
//! };
//!
//! let provider = JagTextProvider::from_system_fonts(SubpixelOrientation::RGB)
//!     .expect("Failed to load fonts");
//!
//! let run = TextRun {
//!     source: None,
//!     spacing: None,
//!     rtl: None,
//!     text: "Hello, world!".to_string(),
//!     pos: [0.0, 0.0],
//!     size: 16.0,
//!     logical_size: 16.0,
//!     color: ColorLinPremul::rgba(255, 255, 255, 255),
//!     weight: 400.0,
//!     style: FontStyle::Normal,
//!     family: None,
//! };
//!
//! let glyphs = provider.rasterize_run(&run);
//! ```

use std::hash::Hash;

/// LCD subpixel orientation along X axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubpixelOrientation {
    RGB,
    BGR,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextAntialiasing {
    Subpixel,
    Grayscale,
}

impl TextAntialiasing {
    const SUBPIXEL_MAX_DEVICE_SCALE: f32 = 1.5;

    fn for_target_os(target_os: &str) -> Self {
        if target_os == "ios" {
            Self::Grayscale
        } else {
            Self::Subpixel
        }
    }

    fn platform_default() -> Self {
        Self::for_target_os(std::env::consts::OS)
    }

    fn for_run(self, run: &crate::scene::TextRun) -> Self {
        if matches!(self, Self::Grayscale) {
            return Self::Grayscale;
        }
        let device_scale = if run.logical_size.is_finite() && run.logical_size > 0.0 {
            (run.size / run.logical_size).max(1.0)
        } else {
            1.0
        };
        if device_scale < Self::SUBPIXEL_MAX_DEVICE_SCALE {
            Self::Subpixel
        } else {
            Self::Grayscale
        }
    }
}

/// Storage format for a subpixel coverage mask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskFormat {
    Rgba8,
    Rgba16,
}

/// Subpixel mask in RGB coverage format stored in RGBA (A is unused).
/// Supports 8-bit or 16-bit per-channel storage.
#[derive(Clone, Debug)]
pub struct SubpixelMask {
    pub width: u32,
    pub height: u32,
    pub format: MaskFormat,
    /// Pixel data, row-major. For Rgba8, 4 bytes/pixel. For Rgba16, 8 bytes/pixel (little-endian u16s).
    pub data: Vec<u8>,
}

impl SubpixelMask {
    pub fn bytes_per_pixel(&self) -> usize {
        match self.format {
            MaskFormat::Rgba8 => 4,
            MaskFormat::Rgba16 => 8,
        }
    }
}

/// Color emoji mask in full RGBA format (premultiplied alpha).
/// Used for color emoji glyphs that have embedded color information.
#[derive(Clone, Debug)]
pub struct ColorMask {
    pub width: u32,
    pub height: u32,
    /// RGBA8 pixel data, row-major, premultiplied alpha.
    pub data: Vec<u8>,
}

impl ColorMask {
    pub fn bytes_per_pixel(&self) -> usize {
        4
    }
}

/// Glyph mask that can be either subpixel (for text) or color (for emoji).
#[derive(Clone, Debug)]
pub enum GlyphMask {
    /// RGB subpixel coverage mask for regular text rendering
    Subpixel(SubpixelMask),
    /// Full RGBA color mask for color emoji
    Color(ColorMask),
}

impl GlyphMask {
    pub fn width(&self) -> u32 {
        match self {
            GlyphMask::Subpixel(m) => m.width,
            GlyphMask::Color(m) => m.width,
        }
    }

    pub fn height(&self) -> u32 {
        match self {
            GlyphMask::Subpixel(m) => m.height,
            GlyphMask::Color(m) => m.height,
        }
    }

    pub fn is_color(&self) -> bool {
        matches!(self, GlyphMask::Color(_))
    }
}

/// GPU-ready batch of glyph masks with positions and color.
/// This is the canonical representation used when sending text to the GPU.
#[derive(Clone, Debug)]
pub struct GlyphBatch {
    pub glyphs: Vec<(SubpixelMask, [f32; 2], crate::scene::ColorLinPremul)>,
}

impl GlyphBatch {
    pub fn new() -> Self {
        Self { glyphs: Vec::new() }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            glyphs: Vec::with_capacity(cap),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    pub fn len(&self) -> usize {
        self.glyphs.len()
    }
}

/// Whether glyph coverage feeds sRGB-space blending (see
/// `jag_shaders::with_blend_space`). Process-wide because the glyph cache is.
static GAMMA_BLEND_COVERAGE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Select the blend space glyph masks are rasterized for, clearing cached
/// glyphs when it changes. Called by `PassManager::new`.
pub(crate) fn set_gamma_blend_coverage(gamma_blend: bool) {
    let previous = GAMMA_BLEND_COVERAGE.swap(gamma_blend, std::sync::atomic::Ordering::Relaxed);
    if previous != gamma_blend {
        invalidate_glyph_run_cache();
    }
}

// These complete-item files share the provider facade and its private font state.
include!("text/support.rs");
include!("text/providers.rs");
include!("text/constructors.rs");
include!("text/raster_masks.rs");
include!("text/family_resolution.rs");
include!("text/fallback.rs");
include!("text/font_selection.rs");
include!("text/variations.rs");
include!("text/jag_provider.rs");
include!("text/tests.rs");

include!("text/source_slices.rs");

include!("text/styled_shaping.rs");
