#[cfg(all(test, target_os = "macos"))]
mod family_resolution_tests {
    use super::*;

    #[test]
    fn browser_sans_serif_uses_arial_without_changing_system_ui() {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        db.load_font_file("/System/Library/Fonts/SFNS.ttf").unwrap();
        let resolve = |candidate| JagTextProvider::resolve_family(&db, &candidate).unwrap();
        let arial = resolve(FontFamilyCandidate::Name("Arial".into()));
        let sans = resolve(FontFamilyCandidate::Generic(GenericFamily::SansSerif));
        for (expected, actual) in [
            (&arial.upright_faces, &sans.upright_faces),
            (&arial.italic_faces, &sans.italic_faces),
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
}
