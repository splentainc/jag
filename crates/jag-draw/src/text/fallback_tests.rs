#[cfg(all(test, target_os = "macos"))]
mod fallback_coverage_tests {
    use super::*;

    fn provider_with_unlisted_script_font() -> JagTextProvider {
        let primary = std::fs::read("/System/Library/Fonts/Supplemental/Arial.ttf").unwrap();
        let mut provider = JagTextProvider::from_bytes(&primary, SubpixelOrientation::RGB).unwrap();
        let mut db = fontdb::Database::new();
        db.load_font_file("/System/Library/Fonts/Supplemental/Myanmar Sangam MN.ttc")
            .unwrap();
        provider.font_db = Some(db);
        provider
    }

    #[test]
    fn coverage_fallback_discovers_an_installed_script_outside_the_family_hints() {
        let provider = provider_with_unlisted_script_font();
        let face = provider
            .resolve_fallback_face_for_char('မ', 400, crate::scene::FontStyle::Normal)
            .expect("installed Myanmar coverage must be found without a family-name hint");
        assert!(JagTextProvider::face_supports_grapheme(&face, "မြန်"));
        let cached = provider
            .resolve_fallback_face_for_char('မ', 400, crate::scene::FontStyle::Normal)
            .unwrap();
        assert_eq!(
            JagTextProvider::face_key(&face),
            JagTextProvider::face_key(&cached)
        );
    }

    #[test]
    fn myanmar_language_label_has_real_glyphs_and_ink() {
        let provider = provider_with_unlisted_script_font();
        let run = crate::scene::TextRun {
            features: Vec::new(),
            source: None,
            spacing: None,
            rtl: None,
            text: "မြန်မာဘာသာ".to_string(),
            pos: [0.0, 0.0],
            size: 18.0,
            logical_size: 18.0,
            color: crate::scene::ColorLinPremul::rgba(54, 102, 204, 255),
            weight: 400.0,
            style: crate::scene::FontStyle::Normal,
            family: Some("Arial, sans-serif".to_string()),
        };
        let primary = provider.select_face(&run);
        let segments = provider.face_segments_for_run(&run, &primary);
        assert_eq!(
            segments.len(),
            1,
            "keep the script's clusters in one shaping context"
        );
        assert!(JagTextProvider::face_supports_grapheme(
            &segments[0].face,
            &run.text
        ));
        let shaped = shape_styled_segment(&run, &run.text, &segments[0].face, &[]);
        assert!(
            shaped.glyphs.iter().all(|&glyph| glyph != 0),
            "no missing glyph boxes"
        );
        assert!(provider.measure_run(&run) > 0.0);
        let glyphs = provider.rasterize_run(&run);
        assert!(
            !glyphs.is_empty(),
            "the language label must paint ink, not a blank advance"
        );
        assert!(glyphs.iter().any(|glyph| match &glyph.mask {
            GlyphMask::Subpixel(mask) => mask.data.iter().any(|&value| value != 0),
            GlyphMask::Color(mask) => mask.data.iter().any(|&value| value != 0),
        }));
    }
}
