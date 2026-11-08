impl JagTextProvider {
    fn face_key(face: &jag_text::FontFace) -> (usize, usize) {
        let bytes = face.borrowed_bytes();
        (bytes.as_ptr() as usize, face.index())
    }

    fn face_supports_char(face: &jag_text::FontFace, ch: char) -> bool {
        if ch.is_control() {
            return true;
        }
        let bytes = face.borrowed_bytes();
        swash::FontRef::from_index(bytes, face.index())
            .map(|font| font.charmap().map(ch) != 0)
            .unwrap_or(false)
    }

    fn face_supports_grapheme(face: &jag_text::FontFace, grapheme: &str) -> bool {
        grapheme
            .chars()
            .all(|ch| ch.is_whitespace() || Self::face_supports_char(face, ch))
    }

    fn fallback_char_for_grapheme(
        primary_face: &jag_text::FontFace,
        grapheme: &str,
    ) -> Option<char> {
        grapheme
            .chars()
            .find(|&ch| {
                !ch.is_whitespace()
                    && !ch.is_control()
                    && !Self::face_supports_char(primary_face, ch)
            })
            .or_else(|| {
                grapheme
                    .chars()
                    .find(|&ch| !ch.is_whitespace() && !ch.is_control())
            })
    }

    fn fallback_family_names_for_char(ch: char) -> &'static [&'static str] {
        match ch as u32 {
            0x2190..=0x21ff | 0x25a0..=0x25ff | 0x2600..=0x26ff => &[
                "Apple Symbols",
                "Segoe UI Symbol",
                "Arial Unicode MS",
                "Noto Sans Symbols",
                "Noto Sans Symbols 2",
                "DejaVu Sans",
            ],
            0x0900..=0x097f => &[
                "Noto Sans Devanagari",
                "Devanagari Sangam MN",
                "Kohinoor Devanagari",
                "Nirmala UI",
                "Mangal",
            ],
            0x0980..=0x09ff => &[
                "Noto Sans Bengali",
                "Bangla Sangam MN",
                "Kohinoor Bangla",
                "Nirmala UI",
                "Vrinda",
            ],
            0x0a00..=0x0a7f => &[
                "Noto Sans Gurmukhi",
                "Gurmukhi MN",
                "Gurmukhi Sangam MN",
                "Nirmala UI",
                "Raavi",
            ],
            0x0a80..=0x0aff => &[
                "Noto Sans Gujarati",
                "Gujarati Sangam MN",
                "Kohinoor Gujarati",
                "Nirmala UI",
                "Shruti",
            ],
            0x0b80..=0x0bff => &[
                "Noto Sans Tamil",
                "Tamil Sangam MN",
                "Tamil MN",
                "Nirmala UI",
                "Latha",
            ],
            0x0c00..=0x0c7f => &[
                "Noto Sans Telugu",
                "Telugu Sangam MN",
                "Kohinoor Telugu",
                "Nirmala UI",
                "Gautami",
            ],
            0x0c80..=0x0cff => &[
                "Noto Sans Kannada",
                "Kannada Sangam MN",
                "Kohinoor Kannada",
                "Nirmala UI",
                "Tunga",
            ],
            0x0d00..=0x0d7f => &[
                "Noto Sans Malayalam",
                "Malayalam Sangam MN",
                "Malayalam MN",
                "Nirmala UI",
                "Kartika",
            ],
            0x0600..=0x06ff => &[
                "Noto Sans Arabic",
                "Geeza Pro",
                "Arial",
                "Segoe UI",
                "Tahoma",
            ],
            0x0590..=0x05ff => &[
                "Noto Sans Hebrew",
                "Arial Hebrew",
                "New Peninim MT",
                "Segoe UI",
                "Arial",
            ],
            0x4e00..=0x9fff | 0x3400..=0x4dbf => &[
                "Noto Sans CJK SC",
                "PingFang SC",
                "Hiragino Sans GB",
                "Microsoft YaHei",
                "SimSun",
            ],
            0x3040..=0x30ff => &[
                "Noto Sans CJK JP",
                "Hiragino Sans",
                "Hiragino Kaku Gothic ProN",
                "Yu Gothic",
                "Meiryo",
            ],
            0xac00..=0xd7af => &[
                "Noto Sans CJK KR",
                "Apple SD Gothic Neo",
                "Malgun Gothic",
                "NanumGothic",
            ],
            _ => &[
                "Arial Unicode MS",
                "Noto Sans",
                "DejaVu Sans",
                "Segoe UI",
                "Arial",
            ],
        }
    }

    fn prefers_symbol_fallback(grapheme: &str) -> bool {
        grapheme.chars().any(|ch| {
            matches!(
                ch as u32,
                0x2190..=0x21ff | 0x25a0..=0x25ff | 0x2600..=0x26ff
            )
        })
    }

    fn browser_symbol_advance_floor(text: &str, size: f32) -> Option<f32> {
        let mut chars = text.chars();
        let ch = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        matches!(
            ch as u32,
            0x2190..=0x21ff | 0x25a0..=0x25ff | 0x2600..=0x26ff
        )
        .then_some(size.max(1.0))
    }

    fn resolve_fallback_face_for_char(
        &self,
        ch: char,
        requested_weight: u16,
        style: crate::scene::FontStyle,
    ) -> Option<jag_text::FontFace> {
        if let Some(cached) = self.char_fallback_cache.lock().unwrap().get(&ch).cloned() {
            return cached;
        }

        let mut resolved = None;
        if let Some(db) = &self.font_db {
            use fontdb::{Family, Query, Stretch, Style, Weight};

            let requested_style = if matches!(
                style,
                crate::scene::FontStyle::Italic | crate::scene::FontStyle::Oblique
            ) {
                Style::Italic
            } else {
                Style::Normal
            };

            for family in Self::fallback_family_names_for_char(ch) {
                let id = db.query(&Query {
                    families: &[Family::Name(family)],
                    weight: Weight(requested_weight),
                    stretch: Stretch::Normal,
                    style: requested_style,
                });
                if let Some(face) = id
                    .and_then(|id| db.face(id))
                    .and_then(Self::load_face_from_db)
                    .filter(|face| Self::face_supports_char(face, ch))
                {
                    resolved = Some(face);
                    break;
                }
            }
            if resolved.is_none() {
                // Family hints accelerate common scripts; they must not be a
                // coverage whitelist. Discover installed faces for other scripts
                // before falling back to the primary face's missing glyph.
                let mut faces: Vec<_> = db
                    .faces()
                    .filter(|face| !face.post_script_name.eq_ignore_ascii_case("LastResort"))
                    .collect();
                faces.sort_by_key(|face| {
                    (
                        face.style != requested_style,
                        face.weight.0.abs_diff(requested_weight),
                        face.post_script_name.as_str(),
                        face.index,
                    )
                });
                resolved = faces.into_iter().find_map(|face| {
                    let supports = db
                        .with_face_data(face.id, |bytes, index| {
                            swash::FontRef::from_index(bytes, index as usize)
                                .is_some_and(|font| font.charmap().map(ch) != 0)
                        })
                        .unwrap_or(false);
                    supports.then(|| Self::load_face_from_db(face)).flatten()
                });
            }
        }

        self.char_fallback_cache
            .lock()
            .unwrap()
            .insert(ch, resolved.clone());
        resolved
    }
}
