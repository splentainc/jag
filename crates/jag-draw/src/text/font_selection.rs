impl JagTextProvider {
    fn face_segments_for_run(
        &self,
        run: &crate::scene::TextRun,
        primary_face: &jag_text::FontFace,
    ) -> Vec<TextFaceSegment> {
        use unicode_segmentation::UnicodeSegmentation;

        let requested_weight = run.weight.clamp(100.0, 900.0).round() as u16;
        let mut segments: Vec<TextFaceSegment> = Vec::new();
        let mut current_start = 0;
        let mut current_face = primary_face.clone();
        let mut current_key = Self::face_key(&current_face);
        let mut initialized = false;

        for (byte_index, grapheme) in run.text.grapheme_indices(true) {
            let face = if grapheme.chars().all(char::is_whitespace) {
                current_face.clone()
            } else if Self::face_supports_grapheme(primary_face, grapheme) {
                if Self::prefers_symbol_fallback(grapheme) {
                    Self::fallback_char_for_grapheme(primary_face, grapheme)
                        .and_then(|ch| {
                            self.resolve_fallback_face_for_char(ch, requested_weight, run.style)
                        })
                        .unwrap_or_else(|| primary_face.clone())
                } else {
                    primary_face.clone()
                }
            } else if let Some(face) = self.family_fallback_for_grapheme(run, grapheme) {
                face
            } else if initialized
                && current_key != Self::face_key(primary_face)
                && Self::face_supports_grapheme(&current_face, grapheme)
            {
                current_face.clone()
            } else {
                Self::fallback_char_for_grapheme(primary_face, grapheme)
                    .and_then(|ch| {
                        self.resolve_fallback_face_for_char(ch, requested_weight, run.style)
                    })
                    .unwrap_or_else(|| primary_face.clone())
            };
            let key = Self::face_key(&face);
            if !initialized {
                current_start = byte_index;
                current_face = face;
                current_key = key;
                initialized = true;
                continue;
            }
            if key != current_key {
                segments.push(TextFaceSegment {
                    start: current_start,
                    end: byte_index,
                    face: current_face,
                });
                current_start = byte_index;
                current_key = key;
            }
            current_face = face;
        }

        if initialized {
            segments.push(TextFaceSegment {
                start: current_start,
                end: run.text.len(),
                face: current_face,
            });
        }
        segments
    }

    fn family_fallback_for_grapheme(
        &self,
        run: &crate::scene::TextRun,
        grapheme: &str,
    ) -> Option<jag_text::FontFace> {
        let weight = run.weight.clamp(100.0, 900.0).round() as u16;
        let italic = matches!(
            run.style,
            crate::scene::FontStyle::Italic | crate::scene::FontStyle::Oblique
        );
        for candidate in parse_font_family_stack(run.family.as_deref()?) {
            let key = Self::cache_key_for(&candidate);
            let cached = self
                .font_cache
                .lock()
                .unwrap()
                .get(&key)
                .and_then(|set| Self::pick_variant(set, weight, italic));
            let face = cached.or_else(|| {
                let set = Self::resolve_family(self.font_db.as_ref()?, &candidate)?;
                let face = Self::pick_variant(&set, weight, italic);
                self.font_cache.lock().unwrap().insert(key, set);
                face
            });
            if let Some(face) = face
                && Self::face_supports_grapheme(&face, grapheme)
            {
                return Some(face);
            }
        }
        None
    }

    /// Select the appropriate font face based on a `TextRun`'s family, weight,
    /// and style.
    ///
    /// When the run specifies a `family` string, parses it as a CSS font-family
    /// stack and walks the candidates in order, resolving each against the system
    /// font database. Resolved fonts are cached for subsequent requests.
    ///
    /// Returns a *cloned* `FontFace` (cheap — inner data is `Arc`).
    fn select_face(&self, run: &crate::scene::TextRun) -> jag_text::FontFace {
        self.select_face_for_style(
            run.family.as_deref(),
            run.weight.clamp(100.0, 900.0).round() as u16,
            run.style,
        )
    }

    fn select_face_for_style(
        &self,
        family: Option<&str>,
        requested_weight: u16,
        style: crate::scene::FontStyle,
    ) -> jag_text::FontFace {
        use crate::scene::FontStyle as SceneFontStyle;
        let is_bold = requested_weight >= 600;
        let is_italic = matches!(style, SceneFontStyle::Italic | SceneFontStyle::Oblique);

        // --- Try to resolve from the font-family string, if present. ---
        if let Some(family_str) = family {
            let candidates = parse_font_family_stack(family_str);

            for candidate in &candidates {
                let key = Self::cache_key_for(candidate);
                // Web fonts are registered independently of a system fontdb.
                if let Some(face) = self
                    .font_cache
                    .lock()
                    .unwrap()
                    .get(&key)
                    .and_then(|set| Self::pick_variant(set, requested_weight, is_italic))
                {
                    return face;
                }
                if let Some(db) = &self.font_db
                    && let Some(set) = Self::resolve_family(db, candidate)
                {
                    let face = Self::pick_variant(&set, requested_weight, is_italic)
                        .unwrap_or_else(|| self.font.clone());
                    self.font_cache.lock().unwrap().insert(key, set);
                    return face;
                }
            }

            // If no fontdb or no candidates matched, check for the legacy
            // "monospace" shorthand before falling through to defaults.
            if family_str.eq_ignore_ascii_case("monospace") {
                if let Some(ref mono) = self.mono_font {
                    return mono.clone();
                }
            }
        }

        // --- Fallback to the pre-loaded defaults. ---
        if is_italic {
            if let Some(ref italic) = self.italic_font {
                return italic.clone();
            }
        }
        if is_bold {
            if let Some(ref bold) = self.bold_font {
                return bold.clone();
            }
        }
        self.font.clone()
    }
    fn browser_face_metrics(face: &jag_text::FontFace, px: f32) -> jag_text::ScaledFontMetrics {
        let metrics = face.scaled_metrics(px);
        #[cfg(target_os = "macos")]
        {
            let mut metrics = metrics;
            // Match Blink/Safari's macOS legacy-font ascent correction. Inspect
            // the selected face, not the CSS alias (or an unavailable family).
            let font = swash::FontRef::from_index(face.borrowed_bytes(), face.index());
            let family = font
                .and_then(|font| {
                    font.localized_strings()
                        .find_by_id(swash::StringId::Family, Some("en"))
                })
                .map(|s| s.to_string());
            if matches!(family.as_deref(), Some("Helvetica" | "Times" | "Courier")) {
                metrics.ascent +=
                    ((metrics.ascent.round() + metrics.descent.round()) * 0.15).round();
            }
            metrics
        }
        #[cfg(not(target_os = "macos"))]
        metrics
    }
}
