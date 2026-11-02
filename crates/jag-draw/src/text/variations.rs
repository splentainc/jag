impl JagTextProvider {
    /// Build font variation axis settings for variable fonts.
    ///
    /// Sets `wght` (weight), `opsz` (optical size), `ital` (italic toggle),
    /// and `slnt` (slant angle). For static fonts these are silently ignored
    /// by both HarfBuzz and Swash.
    ///
    /// `logical_size_px` is the CSS font-size in logical px (before DPI
    /// scaling).  The `opsz` axis must use this value — not the physical
    /// pixel size — because the optical-size axis controls stroke weight
    /// and contrast for readability at a given *reading* size.  Using the
    /// physical size (e.g. 32 at 2× retina for a 16px font) selects the
    /// "display" design that is too thin for body text.  Chrome uses the
    /// CSS font-size for `opsz`, and matching this is critical for weight
    /// parity.
    ///
    /// Returns `(4-byte ASCII tag, value)` pairs usable by both harfrust
    /// (via `Tag::new`) and swash (via string conversion).

    fn build_variations(
        weight: f32,
        run: &crate::scene::TextRun,
        style: crate::scene::FontStyle,
    ) -> Vec<([u8; 4], f32)> {
        // Use CSS logical size for opsz; fall back to physical size for
        // callers that don't set logical_size (Direct IR mode, tests).
        let logical_size_px = if run.logical_size > 0.0 {
            run.logical_size
        } else {
            run.size
        };
        let is_italic = matches!(
            style,
            crate::scene::FontStyle::Italic | crate::scene::FontStyle::Oblique
        );
        // For `system-ui`, Chrome / macOS CoreText returns the system font
        // configured at the "Text" optical design (~opsz 19–20), NOT at the
        // literal CSS px size. Passing the raw CSS px (e.g. 16) clamps to
        // SFNS's axis minimum of 17 and produces ~3.7% wider advances than
        // Chrome. Apply this floor ONLY when the CSS family list hints at
        // system-ui so other fonts (web fonts, named families, the default
        // fallback used by tests with `family: None`) keep their existing
        // opsz semantics.
        const SYSTEM_UI_OPSZ_FLOOR: f32 = 20.0;
        let is_system_ui_request = run
            .family
            .as_deref()
            .map(|f| {
                let lower = f.to_ascii_lowercase();
                lower.contains("system-ui") || lower.contains("-apple-system")
            })
            .unwrap_or(false);
        let opsz = if is_system_ui_request {
            logical_size_px.max(SYSTEM_UI_OPSZ_FLOOR).clamp(17.0, 96.0)
        } else {
            logical_size_px.clamp(17.0, 96.0)
        };
        vec![
            (*b"wght", weight),
            (*b"opsz", opsz),
            // Italic toggle: 0 = upright, 1 = italic (CSS Fonts §4.9).
            (*b"ital", if is_italic { 1.0 } else { 0.0 }),
            // Slant angle: 0 = upright, -12 = standard CSS oblique (CSS Fonts §4.8).
            (*b"slnt", if is_italic { -12.0 } else { 0.0 }),
        ]
    }

    /// Pick the best weight/style variant from a cached font set.
    ///
    /// When `italic = true`, tries italic faces first, then falls back to
    /// upright (browsers render upright when no italic is available).
    /// When `italic = false`, only returns upright faces — never returns an
    /// italic face for non-italic requests. This matches browser behavior:
    /// if a font family has only italic faces, the browser skips it and
    /// tries the next family in the CSS font-family stack.
    fn pick_variant(
        set: &CachedFontSet,
        requested_weight: u16,
        italic: bool,
    ) -> Option<jag_text::FontFace> {
        if std::env::var("JAG_TEXT_DEBUG_FAMILY").is_ok() {
            eprintln!(
                "[TEXT] pick_variant weight={} italic={} upright={} italic_faces={}",
                requested_weight,
                italic,
                set.upright_faces.len(),
                set.italic_faces.len()
            );
        }

        if italic {
            if let Some(face) =
                Self::pick_closest_weighted_face(&set.italic_faces, requested_weight)
            {
                return Some(face);
            }
        }

        if let Some(face) = Self::pick_closest_weighted_face(&set.upright_faces, requested_weight) {
            return Some(face);
        }

        // For italic requests, fall back to upright (standard browser behavior).
        // For upright requests, do NOT fall back to italic — return None so
        // select_face tries the next font-family candidate.
        if italic {
            if let Some(face) =
                Self::pick_closest_weighted_face(&set.italic_faces, requested_weight)
            {
                return Some(face);
            }
        }

        None
    }

    /// Layout a paragraph using jag-text's `TextLayout` with optional width-based wrapping.
    ///
    /// This exposes jag-text's multi-line layout (including per-line baselines) so that
    /// callers can build GPU-ready glyph batches without relying on `PassManager`
    /// baseline heuristics.
    pub fn layout_paragraph(
        &self,
        text: &str,
        size_px: f32,
        max_width: Option<f32>,
    ) -> jag_text::layout::TextLayout {
        use jag_text::layout::{TextLayout, WrapMode};

        let wrap = if max_width.is_some() {
            WrapMode::BreakWord
        } else {
            WrapMode::NoWrap
        };

        TextLayout::with_wrap(
            text.to_string(),
            &self.font,
            size_px.max(1.0),
            max_width,
            wrap,
        )
    }
}
