impl JagTextProvider {
    fn fallback_emoji_font(&self) -> Option<&jag_text::FontFace> {
        self.emoji_font.get_or_init(|| {
            use fontdb::{Family, Query, Stretch, Style, Weight};
            let db = self.font_db.as_ref()?;
            let id = db.query(&Query {
                families: &[
                    Family::Name("Apple Color Emoji"),
                    Family::Name("Segoe UI Emoji"),
                    Family::Name("Noto Color Emoji"),
                ],
                weight: Weight::NORMAL,
                stretch: Stretch::Normal,
                style: Style::Normal,
                ..Query::default()
            })?;
            Self::load_face_from_db(db.face(id)?)
        }).as_ref()
    }
}

#[cfg(all(test, target_os = "macos"))]
mod lazy_emoji_tests {
    use super::*;
    #[test]
    fn ordinary_text_does_not_load_the_emoji_file_and_fallback_remains_available() {
        let provider = JagTextProvider::from_system_fonts(SubpixelOrientation::RGB).unwrap();
        assert!(provider.emoji_font.get().is_none());
        let run = crate::scene::TextRun {
            text: "Software still starts with you".into(),
            pos: [0., 0.], size: 16., logical_size: 16.,
            color: crate::scene::ColorLinPremul::rgba(0, 0, 0, 255),
            weight: 400., style: crate::scene::FontStyle::Normal,
            family: None, features: Vec::new(), source: None, spacing: None, rtl: None,
        };
        assert!(provider.measure_run(&run) > 0.);
        assert!(!provider.rasterize_run(&run).is_empty());
        assert!(provider.emoji_font.get().is_none());
        let face = provider.fallback_emoji_font().unwrap();
        let bytes = face.as_bytes();
        let font = swash::FontRef::from_index(&bytes, face.index()).unwrap();
        assert_ne!(font.charmap().map('😀'), 0);
        assert!(std::ptr::eq(face, provider.fallback_emoji_font().unwrap()));
    }
}
