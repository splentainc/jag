#[cfg(all(test, target_os = "macos"))]
mod family_resolution_tests {
    use super::*;

    #[test]
    fn browser_sans_serif_uses_helvetica_without_changing_system_ui() {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        db.load_font_file("/System/Library/Fonts/SFNS.ttf").unwrap();
        let resolve = |candidate| JagTextProvider::resolve_family(&db, &candidate).unwrap();
        let helvetica = resolve(FontFamilyCandidate::Name("Helvetica".into()));
        let sans = resolve(FontFamilyCandidate::Generic(GenericFamily::SansSerif));
        for (expected, actual) in [
            (&helvetica.upright_faces, &sans.upright_faces),
            (&helvetica.italic_faces, &sans.italic_faces),
        ] {
            assert_eq!(expected.len(), actual.len());
            for ((ew, ef), (aw, af)) in expected.iter().zip(actual) {
                assert_eq!(ew, aw);
                assert_eq!(ef.index(), af.index());
                assert!(
                    ef.as_bytes() == af.as_bytes(),
                    "sans-serif must use browser font metrics"
                );
            }
        }
        let ui = resolve(FontFamilyCandidate::Generic(GenericFamily::SystemUi));
        assert!(ui.upright_faces[0].1.as_bytes() != sans.upright_faces[0].1.as_bytes());
    }
    #[test]
    fn styled_metrics_match_the_painted_face_before_any_text_is_shaped() {
        let provider = JagTextProvider::from_system_fonts(SubpixelOrientation::RGB).unwrap();
        for family in [Some("Arial"), Some("missing-face, Arial"), None] {
            for style in [
                crate::scene::FontStyle::Normal,
                crate::scene::FontStyle::Italic,
            ] {
                provider.font_cache.lock().unwrap().clear();
                let cold = provider
                    .line_metrics_for_style(11.2, family, 700, style)
                    .unwrap();
                let run = crate::scene::TextRun {
                    features: Vec::new(),
                    source: None,
                    spacing: None,
                    rtl: None,
                    text: "Toggle".into(),
                    pos: [0., 0.],
                    size: 11.2,
                    logical_size: 11.2,
                    color: crate::scene::ColorLinPremul::rgba(0, 0, 0, 255),
                    weight: 700.,
                    style,
                    family: family.map(str::to_owned),
                };
                let face = provider.select_face(&run);
                let expected = face.scaled_metrics(11.2);
                let warm = provider
                    .line_metrics_for_style(11.2, family, 700, style)
                    .unwrap();
                assert_eq!(
                    (cold.ascent, cold.descent, cold.line_gap),
                    (expected.ascent, expected.descent, expected.line_gap),
                    "{family:?} {style:?}"
                );
                assert_eq!(
                    (cold.ascent, cold.descent, cold.line_gap),
                    (warm.ascent, warm.descent, warm.line_gap)
                );
            }
        }
    }
    #[test]
    fn mac_generic_sans_metrics_match_chrome_at_fractional_button_size() {
        let provider = JagTextProvider::from_system_fonts(SubpixelOrientation::RGB).unwrap();
        for family in ["sans-serif", "Helvetica", "missing-face, sans-serif"] {
            let metrics = provider
                .line_metrics_for_style(11.2, Some(family), 700, crate::scene::FontStyle::Normal)
                .unwrap();
            // Independent Chrome Canvas fontBoundingBox measurements, and its
            // native-font ascent correction, at the Wikipedia toggle size.
            assert_eq!(
                (metrics.ascent.round(), metrics.descent.round()),
                (11., 3.),
                "{family}"
            );
        }
    }
}
