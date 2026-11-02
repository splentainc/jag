impl TextProvider for JagTextProvider {
    fn cache_tag(&self) -> u64 {
        self.font_generation
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn rasterize_run(&self, run: &crate::scene::TextRun) -> Vec<RasterizedGlyph> {
        use swash::FontRef;
        use swash::scale::{Render, ScaleContext, Source, StrikeWith};

        let size = run.size.max(1.0);
        let antialiasing = self.antialiasing.for_run(run);
        let face = self.select_face(run);
        let mut segments = self.face_segments_for_run(run, &face);
        if run.rtl == Some(true) {
            segments.reverse();
        }

        // Build variation settings for variable fonts (wght, opsz, ital, slnt).
        let requested_weight = run.weight.clamp(100.0, 900.0);
        let raw_variations = Self::build_variations(requested_weight, run, run.style);
        let mut renderer = Render::new(&[
            Source::Outline,
            Source::Bitmap(StrikeWith::BestFit),
            Source::ColorBitmap(StrikeWith::BestFit),
        ]);
        if matches!(antialiasing, TextAntialiasing::Subpixel) {
            renderer.format(swash::zeno::Format::Subpixel);
        }
        let stem_embolden = self.browser_stem_embolden_px(run, antialiasing);
        if stem_embolden > 0.0 {
            renderer.embolden(stem_embolden);
        }

        // Get emoji font bytes if available (we'll create FontRef per-use due to lifetime constraints)
        let emoji_bytes = self.emoji_font.as_ref().map(|f| f.as_bytes());

        let mut out = Vec::new();
        let mut pen_x: f32 = 0.0;

        for segment in segments {
            let text = &run.text[segment.start..segment.end];
            if text.is_empty() {
                continue;
            }
            let segment_x = pen_x;
            let mut segment_advance = 0.0;

            let shaped = {
                use jag_text::shaping::hb_tag_from_bytes;
                let hb_vars: Vec<_> = raw_variations
                    .iter()
                    .map(|(tag, val)| (hb_tag_from_bytes(tag), *val))
                    .collect();
                shape_styled_segment(run, text, &segment.face, &hb_vars)
            };

            let font_bytes = segment.face.as_bytes();
            let font_ref = FontRef::from_index(&font_bytes, segment.face.index())
                .expect("jag-text FontFace bytes should be a valid swash FontRef");
            let norm_coords: Vec<swash::NormalizedCoord> = if raw_variations.is_empty() {
                Vec::new()
            } else {
                font_ref
                    .variations()
                    .normalized_coords(&raw_variations)
                    .collect()
            };

            let mut ctx = ScaleContext::new();
            let mut builder = ctx.builder(font_ref).size(size).hint(true);
            if !norm_coords.is_empty() {
                builder = builder.normalized_coords(&norm_coords);
            }
            let mut scaler = builder.build();
            let symbol_advance_floor = Self::browser_symbol_advance_floor(text, size);
            let shaped_advance = shaped.width;
            let glyph_x_adjust = symbol_advance_floor
                .map(|floor| ((floor - shaped_advance).max(0.0)) * 0.5)
                .unwrap_or(0.0);

            for idx in 0..shaped.glyphs.len() {
                let cluster = shaped.clusters[idx] as usize;
                let end = shaped
                    .clusters
                    .iter()
                    .map(|&byte| byte as usize)
                    .filter(|&byte| byte > cluster)
                    .min()
                    .unwrap_or(text.len());
                let source = Some(segment.start + cluster..segment.start + end);
                let glyph_id = shaped.glyphs[idx];
                let advance = shaped.advances[idx];
                let glyph_pos = shaped.positions[idx];

                if glyph_id == 0 {
                    let cluster_byte = shaped.clusters[idx] as usize;
                    let emoji_rendered = emoji_bytes.as_ref().and_then(|eb| {
                        let ch = text[cluster_byte..].chars().next()?;
                        let emoji_font_ref = FontRef::from_index(eb, 0)?;
                        let emoji_gid = emoji_font_ref.charmap().map(ch);
                        if emoji_gid == 0 {
                            return None;
                        }
                        let mut emoji_ctx = ScaleContext::new();
                        let mut emoji_scaler = emoji_ctx
                            .builder(emoji_font_ref)
                            .size(size)
                            .hint(false)
                            .build();
                        let emoji_renderer = Render::new(&[
                            Source::ColorOutline(0),
                            Source::ColorBitmap(StrikeWith::BestFit),
                            Source::Bitmap(StrikeWith::BestFit),
                            Source::Outline,
                        ]);
                        let img = emoji_renderer.render(&mut emoji_scaler, emoji_gid)?;
                        let w = img.placement.width;
                        let h = img.placement.height;
                        if w == 0 || h == 0 {
                            return None;
                        }
                        let mask =
                            self.swash_glyph_mask(antialiasing, img.content, w, h, &img.data);
                        let ox = segment_x
                            + glyph_x_adjust
                            + glyph_pos.x_offset
                            + img.placement.left as f32;
                        let oy = glyph_pos.y_offset - img.placement.top as f32;
                        out.push(RasterizedGlyph {
                            source: source.clone(),
                            offset: [ox, oy],
                            mask,
                        });
                        Some(w as f32)
                    });

                    if let Some(emoji_width) = emoji_rendered {
                        segment_advance += emoji_width;
                    } else {
                        segment_advance += size * 0.5;
                    }
                    continue;
                }

                if let Some(img) = renderer.render(&mut scaler, glyph_id) {
                    let w = img.placement.width;
                    let h = img.placement.height;
                    if w > 0 && h > 0 {
                        let mask =
                            self.swash_glyph_mask(antialiasing, img.content, w, h, &img.data);

                        let ox = segment_x
                            + glyph_x_adjust
                            + glyph_pos.x_offset
                            + img.placement.left as f32;
                        let oy = glyph_pos.y_offset - img.placement.top as f32;
                        out.push(RasterizedGlyph {
                            source: source.clone(),
                            offset: [ox, oy],
                            mask,
                        });
                    }
                }

                segment_advance += advance;
            }
            if let Some(floor) = symbol_advance_floor {
                segment_advance = segment_advance.max(floor);
            }
            pen_x = segment_x + segment_advance;
        }

        out
    }

    fn shape_paragraph(&self, text: &str, size_px: f32) -> Option<ShapedParagraph> {
        // Use jag-text layout to compute glyph advances; this matches the
        // shaping used for cursor movement and selection and avoids width
        // drift when centering text.
        let layout = self.layout_paragraph(text, size_px, None);
        let mut glyphs = Vec::new();
        for line in layout.lines() {
            for run in &line.runs {
                for (idx, adv) in run.advances.iter().enumerate() {
                    glyphs.push(ShapedGlyph {
                        cluster: run.clusters.get(idx).copied().unwrap_or(0),
                        x_advance: *adv,
                    });
                }
            }
        }
        Some(ShapedParagraph { glyphs })
    }

    fn shape_styled_run(&self, run: &crate::scene::TextRun) -> Option<ShapedParagraph> {
        use jag_text::shaping::hb_tag_from_bytes;
        let face = self.select_face(run);
        let variations = Self::build_variations(run.weight.clamp(100.0, 900.0), run, run.style);
        let variations: Vec<_> = variations
            .iter()
            .map(|(tag, value)| (hb_tag_from_bytes(tag), *value))
            .collect();
        let mut glyphs = Vec::new();
        let mut segments = self.face_segments_for_run(run, &face);
        if run.rtl == Some(true) {
            segments.reverse();
        }
        for segment in segments {
            let text = &run.text[segment.start..segment.end];
            let shaped = shape_styled_segment(run, text, &segment.face, &variations);
            let extra = Self::browser_symbol_advance_floor(text, run.size.max(1.0))
                .map_or(0.0, |floor| (floor - shaped.width).max(0.0));
            for (index, (&cluster, &advance)) in
                shaped.clusters.iter().zip(&shaped.advances).enumerate()
            {
                glyphs.push(ShapedGlyph {
                    cluster: segment.start as u32 + cluster,
                    x_advance: advance + if index == 0 { extra } else { 0.0 },
                });
            }
        }
        Some(ShapedParagraph { glyphs })
    }

    fn line_metrics(&self, px: f32) -> Option<LineMetrics> {
        let m = self.font.scaled_metrics(px.max(1.0));
        Some(LineMetrics {
            ascent: m.ascent,
            descent: m.descent,
            line_gap: m.line_gap,
        })
    }

    fn line_metrics_for_style(
        &self,
        px: f32,
        family: Option<&str>,
        weight: u16,
        style: crate::scene::FontStyle,
    ) -> Option<LineMetrics> {
        // Try to find the specific font face from the cache (web fonts, system fonts).
        if let Some(family_str) = family {
            let is_italic = matches!(
                style,
                crate::scene::FontStyle::Italic | crate::scene::FontStyle::Oblique
            );
            let candidates = parse_font_family_stack(family_str);
            let cache = self.font_cache.lock().unwrap();
            for candidate in &candidates {
                let key = Self::cache_key_for(candidate);
                if let Some(set) = cache.get(&key) {
                    if let Some(face) = Self::pick_variant(set, weight, is_italic) {
                        let m = face.scaled_metrics(px.max(1.0));
                        return Some(LineMetrics {
                            ascent: m.ascent,
                            descent: m.descent,
                            line_gap: m.line_gap,
                        });
                    }
                }
            }
        }
        // Fall back to the default font
        self.line_metrics(px)
    }

    fn measure_run(&self, run: &crate::scene::TextRun) -> f32 {
        let size = run.size.max(1.0);
        let face = self.select_face(run);
        let mut segments = self.face_segments_for_run(run, &face);
        if run.rtl == Some(true) {
            segments.reverse();
        }

        // Shape with HarfBuzz — returns the same advances used by rasterize_run,
        // so measurement and rendering always agree.
        let requested_weight = run.weight.clamp(100.0, 900.0);
        let raw_variations = Self::build_variations(requested_weight, run, run.style);
        let mut width = 0.0;
        for segment in segments {
            let text = &run.text[segment.start..segment.end];
            if text.is_empty() {
                continue;
            }
            let shaped = {
                use jag_text::shaping::hb_tag_from_bytes;
                let hb_vars: Vec<_> = raw_variations
                    .iter()
                    .map(|(tag, val)| (hb_tag_from_bytes(tag), *val))
                    .collect();
                shape_styled_segment(run, text, &segment.face, &hb_vars)
            };
            width += Self::browser_symbol_advance_floor(text, size)
                .map(|floor| shaped.width.max(floor))
                .unwrap_or(shaped.width);
        }
        if std::env::var("JAG_TEXT_DEBUG_FAMILY").is_ok()
            && (run.text.contains("Z-Ordering")
                || run.text.contains("Hit Testing")
                || run.text.contains("Depth Buffer")
                || run.text.contains(" System Fonts")
                || run.text.contains(" Opacity")
                || run.text.contains(" Text Runs")
                || run.text.contains(" Inline Block"))
        {
            eprintln!(
                "[TEXT] measure_run text={:?} size={} weight={} width={}",
                run.text, run.size, run.weight, width
            );
        }
        width
    }

    fn register_web_font(
        &self,
        family: &str,
        data: Vec<u8>,
        weight: u16,
        style: crate::scene::FontStyle,
    ) -> anyhow::Result<bool> {
        // Delegate to the concrete JagTextProvider implementation.
        JagTextProvider::register_web_font(self, family, data, weight, style)
    }
}
