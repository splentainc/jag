impl JagTextProvider {
    fn swash_glyph_mask(
        &self,
        antialiasing: TextAntialiasing,
        content: swash::scale::image::Content,
        width: u32,
        height: u32,
        data: &[u8],
    ) -> GlyphMask {
        use swash::scale::image::Content;

        match content {
            Content::Mask => {
                let mask = match antialiasing {
                    TextAntialiasing::Subpixel => {
                        grayscale_to_subpixel_rgb(width, height, data, self.orientation)
                    }
                    TextAntialiasing::Grayscale => grayscale_to_rgb_equal(width, height, data),
                };
                GlyphMask::Subpixel(mask)
            }
            Content::SubpixelMask => {
                let data = match antialiasing {
                    // Swash already produced true per-channel coverage. Applying
                    // the grayscale/browser strengthening curve here widens
                    // partial edge pixels, making low-DPI text look heavy and
                    // smudged. Preserve the native LCD mask unchanged.
                    TextAntialiasing::Subpixel => data.to_vec(),
                    TextAntialiasing::Grayscale => strengthen_grayscale_subpixel_rgba8(data),
                };
                GlyphMask::Subpixel(SubpixelMask {
                    width,
                    height,
                    format: MaskFormat::Rgba8,
                    data,
                })
            }
            Content::Color => GlyphMask::Color(ColorMask {
                width,
                height,
                data: data.to_vec(),
            }),
        }
    }

    fn browser_stem_embolden_px(
        &self,
        run: &crate::scene::TextRun,
        antialiasing: TextAntialiasing,
    ) -> f32 {
        if !matches!(antialiasing, TextAntialiasing::Grayscale) {
            return 0.0;
        }

        // CoreText/Chromium grayscale text retains slightly more outline ink
        // than Swash at mobile sizes. Apply the measured 0.1 CSS-pixel stem
        // darkening in physical pixels, without changing advances, CSS weight,
        // or authored color. Clamp malformed scale ratios defensively.
        const STEM_DARKENING_CSS_PX: f32 = 0.1;
        let device_scale = if run.logical_size > 0.0 {
            (run.size / run.logical_size).clamp(1.0, 4.0)
        } else {
            1.0
        };
        STEM_DARKENING_CSS_PX * device_scale
    }

    /// Load a `FontFace` from a `fontdb` face entry.
    fn load_face_from_db(face: &fontdb::FaceInfo) -> Option<jag_text::FontFace> {
        use fontdb::Source;
        let bytes: Vec<u8> = match &face.source {
            Source::File(path) => std::fs::read(path).ok()?,
            Source::Binary(data) => data.as_ref().as_ref().to_vec(),
            Source::SharedFile(_, data) => data.as_ref().as_ref().to_vec(),
        };
        jag_text::FontFace::from_vec(bytes, face.index as usize).ok()
    }

    /// Register a web font from raw TTF/OTF bytes.
    ///
    /// **Idempotent**: returns `Ok(false)` if a font with the same
    /// family + weight + style is already registered.
    ///
    /// **Thread-safe**: font_cache is Mutex-protected.
    ///
    /// The `data` must be raw TTF or OTF — WOFF/WOFF2 must be decompressed
    /// before calling this method.
    pub fn register_web_font(
        &self,
        family: &str,
        data: Vec<u8>,
        weight: u16,
        style: crate::scene::FontStyle,
    ) -> anyhow::Result<bool> {
        let cache_key = family.to_lowercase();
        let is_italic = matches!(
            style,
            crate::scene::FontStyle::Italic | crate::scene::FontStyle::Oblique
        );

        // Idempotent fast-path checks before parsing font bytes.
        {
            let cache = self.font_cache.lock().unwrap();
            if let Some(set) = cache.get(&cache_key) {
                let faces = if is_italic {
                    &set.italic_faces
                } else {
                    &set.upright_faces
                };
                if faces.iter().any(|(w, _)| *w == weight) {
                    return Ok(false);
                }
            }
        }

        // Validate and create FontFace from raw TTF/OTF bytes
        let face = jag_text::FontFace::from_vec(data, 0)
            .map_err(|e| anyhow::anyhow!("invalid font data for '{}': {}", family, e))?;

        // Insert into our font cache
        let mut cache = self.font_cache.lock().unwrap();
        if let Some(set) = cache.get_mut(&cache_key) {
            let faces = if is_italic {
                &mut set.italic_faces
            } else {
                &mut set.upright_faces
            };
            Self::insert_weighted_face(faces, weight, face);
        } else {
            let mut set = CachedFontSet {
                upright_faces: Vec::new(),
                italic_faces: Vec::new(),
            };
            if is_italic {
                set.italic_faces.push((weight, face));
            } else {
                set.upright_faces.push((weight, face));
            }
            cache.insert(cache_key, set);
        }

        // Flush the global glyph rasterization cache so the next frame
        // re-rasterizes text using the newly registered web font face
        // instead of returning stale bitmaps from the old fallback font.
        invalidate_glyph_run_cache();

        // Bump generation so text-width caches (keyed by cache_tag)
        // are invalidated — re-layout will re-measure with the new font.
        self.font_generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        Ok(true)
    }
}
