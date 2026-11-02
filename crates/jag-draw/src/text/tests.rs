#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_font_family() {
        let result = parse_font_family_stack("Georgia");
        assert_eq!(result, vec![FontFamilyCandidate::Name("Georgia".into())]);
    }

    #[test]
    fn parse_font_stack_with_generic() {
        let result = parse_font_family_stack("Georgia, \"Times New Roman\", Times, serif");
        assert_eq!(
            result,
            vec![
                FontFamilyCandidate::Name("Georgia".into()),
                FontFamilyCandidate::Name("Times New Roman".into()),
                FontFamilyCandidate::Name("Times".into()),
                FontFamilyCandidate::Generic(GenericFamily::Serif),
            ]
        );
    }

    #[test]
    fn parse_sans_serif_stack() {
        let result = parse_font_family_stack(
            "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif",
        );
        assert_eq!(
            result,
            vec![
                FontFamilyCandidate::Generic(GenericFamily::SystemUi),
                FontFamilyCandidate::Generic(GenericFamily::SystemUi),
                FontFamilyCandidate::Name("Segoe UI".into()),
                FontFamilyCandidate::Name("Roboto".into()),
                FontFamilyCandidate::Generic(GenericFamily::SansSerif),
            ]
        );
    }

    #[test]
    fn parse_monospace_stack() {
        let result = parse_font_family_stack("'SF Mono', ui-monospace, monospace");
        assert_eq!(
            result,
            vec![
                FontFamilyCandidate::Name("SF Mono".into()),
                FontFamilyCandidate::Generic(GenericFamily::Monospace),
                FontFamilyCandidate::Generic(GenericFamily::Monospace),
            ]
        );
    }

    #[test]
    fn parse_empty_and_whitespace() {
        assert!(parse_font_family_stack("").is_empty());
        assert!(parse_font_family_stack("  ,  , ").is_empty());
    }

    #[test]
    fn generic_families_case_insensitive() {
        let result = parse_font_family_stack("SERIF, Sans-Serif, MONOSPACE");
        assert_eq!(
            result,
            vec![
                FontFamilyCandidate::Generic(GenericFamily::Serif),
                FontFamilyCandidate::Generic(GenericFamily::SansSerif),
                FontFamilyCandidate::Generic(GenericFamily::Monospace),
            ]
        );
    }

    #[test]
    fn cache_key_case_insensitive() {
        let k1 = JagTextProvider::cache_key_for(&FontFamilyCandidate::Name("Georgia".into()));
        let k2 = JagTextProvider::cache_key_for(&FontFamilyCandidate::Name("georgia".into()));
        assert_eq!(k1, k2);
    }

    #[test]
    fn cache_key_generic_distinct() {
        let serif =
            JagTextProvider::cache_key_for(&FontFamilyCandidate::Generic(GenericFamily::Serif));
        let sans =
            JagTextProvider::cache_key_for(&FontFamilyCandidate::Generic(GenericFamily::SansSerif));
        assert_ne!(serif, sans);
    }

    #[test]
    fn coverage_strengthening_preserves_extremes_and_darkens_edges() {
        assert_eq!(strengthen_coverage_u8(0), 0);
        assert_eq!(strengthen_coverage_u8(255), 255);
        assert!(strengthen_coverage_u8(96) > 96);
        assert!(strengthen_coverage_u8(128) > 190);
        assert!(strengthen_coverage_u8(192) > 192);
    }

    #[test]
    fn grayscale_masks_use_browser_strengthened_coverage() {
        let mask = grayscale_to_rgb_equal(3, 1, &[0, 128, 255]);
        assert_eq!(&mask.data[0..4], &[0, 0, 0, 0]);
        assert!(mask.data[4] > 128);
        assert_eq!(mask.data[4], mask.data[5]);
        assert_eq!(mask.data[5], mask.data[6]);
        assert_eq!(&mask.data[8..12], &[255, 255, 255, 0]);
    }

    #[test]
    fn ios_selects_grayscale_coverage_without_changing_desktop_policy() {
        assert_eq!(
            TextAntialiasing::for_target_os("ios"),
            TextAntialiasing::Grayscale
        );
        assert_eq!(
            TextAntialiasing::for_target_os("macos"),
            TextAntialiasing::Subpixel
        );
        assert_eq!(
            TextAntialiasing::for_target_os("windows"),
            TextAntialiasing::Subpixel
        );
    }

    #[test]
    fn desktop_subpixel_coverage_is_limited_to_low_dpi() {
        let make_run = |device_scale: f32| crate::scene::TextRun {
            source: None,
            spacing: None,
            rtl: None,
            text: "Aa".to_string(),
            pos: [0.0, 0.0],
            size: 14.0 * device_scale,
            logical_size: 14.0,
            color: crate::scene::ColorLinPremul::rgba(10, 10, 10, 255),
            weight: 400.0,
            style: crate::scene::FontStyle::Normal,
            family: None,
        };

        assert_eq!(
            TextAntialiasing::Subpixel.for_run(&make_run(1.0)),
            TextAntialiasing::Subpixel
        );
        assert_eq!(
            TextAntialiasing::Subpixel.for_run(&make_run(1.25)),
            TextAntialiasing::Subpixel
        );
        assert_eq!(
            TextAntialiasing::Subpixel.for_run(&make_run(1.5)),
            TextAntialiasing::Grayscale
        );
        assert_eq!(
            TextAntialiasing::Grayscale.for_run(&make_run(1.0)),
            TextAntialiasing::Grayscale
        );
    }

    #[test]
    fn ios_stem_darkening_tracks_device_scale_without_affecting_desktop() {
        let bytes = include_bytes!("../../../../fonts/Geist/Geist-VariableFont_wght.ttf");
        let mut provider = JagTextProvider::from_bytes(bytes, SubpixelOrientation::RGB)
            .expect("load bundled Geist variable font");
        let mut run = crate::scene::TextRun {
            source: None,
            spacing: None,
            rtl: None,
            text: "Medium".to_string(),
            pos: [0.0, 0.0],
            size: 28.0,
            logical_size: 14.0,
            color: crate::scene::ColorLinPremul::rgba(10, 10, 10, 255),
            weight: 500.0,
            style: crate::scene::FontStyle::Normal,
            family: None,
        };

        assert_eq!(
            provider.browser_stem_embolden_px(&run, TextAntialiasing::Subpixel),
            0.0
        );
        provider.antialiasing = TextAntialiasing::Grayscale;
        assert!(
            (provider.browser_stem_embolden_px(&run, TextAntialiasing::Grayscale) - 0.2).abs()
                < f32::EPSILON
        );
        run.size = 42.0;
        assert!(
            (provider.browser_stem_embolden_px(&run, TextAntialiasing::Grayscale) - 0.3).abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn geist_mobile_specimens_preserve_weight_and_grayscale_at_12_14_16_px() {
        let bytes = include_bytes!("../../../../fonts/Geist/Geist-VariableFont_wght.ttf");
        let mut provider = JagTextProvider::from_bytes(bytes, SubpixelOrientation::RGB)
            .expect("load bundled Geist variable font");

        let baseline = provider.rasterize_run(&crate::scene::TextRun {
            source: None,
            spacing: None,
            rtl: None,
            text: "Hamburgefontsiv 012345".to_string(),
            pos: [0.0, 0.0],
            size: 12.0,
            logical_size: 12.0,
            color: crate::scene::ColorLinPremul::rgba(115, 115, 115, 255),
            weight: 400.0,
            style: crate::scene::FontStyle::Normal,
            family: None,
        });
        assert!(
            baseline.iter().any(|glyph| match &glyph.mask {
                GlyphMask::Subpixel(mask) => mask
                    .data
                    .chunks_exact(4)
                    .any(|pixel| pixel[0] != pixel[1] || pixel[1] != pixel[2]),
                GlyphMask::Color(_) => false,
            }),
            "the previous RGB policy must reproduce channel fringing"
        );

        let high_dpi = provider.rasterize_run(&crate::scene::TextRun {
            source: None,
            spacing: None,
            rtl: None,
            text: "Hamburgefontsiv 012345".to_string(),
            pos: [0.0, 0.0],
            size: 28.0,
            logical_size: 14.0,
            color: crate::scene::ColorLinPremul::rgba(115, 115, 115, 255),
            weight: 400.0,
            style: crate::scene::FontStyle::Normal,
            family: None,
        });
        assert!(high_dpi.iter().all(|glyph| {
            match &glyph.mask {
                GlyphMask::Subpixel(mask) => mask
                    .data
                    .chunks_exact(4)
                    .all(|pixel| pixel[0] == pixel[1] && pixel[1] == pixel[2]),
                GlyphMask::Color(_) => true,
            }
        }));

        provider.antialiasing = TextAntialiasing::Grayscale;

        for device_scale in [2.0, 3.0] {
            for css_size in [12.0, 14.0, 16.0] {
                let mut weight_ink = Vec::new();
                for weight in [400.0, 500.0, 700.0] {
                    let run = crate::scene::TextRun {
                        source: None,
                        spacing: None,
                        rtl: None,
                        text: "Hamburgefontsiv 012345".to_string(),
                        pos: [0.0, 0.0],
                        size: css_size * device_scale,
                        logical_size: css_size,
                        color: crate::scene::ColorLinPremul::rgba(115, 115, 115, 255),
                        weight,
                        style: crate::scene::FontStyle::Normal,
                        family: None,
                    };
                    let glyphs = provider.rasterize_run(&run);
                    assert!(
                        !glyphs.is_empty(),
                        "{weight} weight at {css_size}px @ {device_scale}x must rasterize"
                    );

                    let mut coverage_ink = 0_u64;
                    for glyph in glyphs {
                        let GlyphMask::Subpixel(mask) = glyph.mask else {
                            panic!("plain Geist text must use a coverage mask");
                        };
                        for pixel in mask.data.chunks_exact(4) {
                            assert_eq!(pixel[0], pixel[1], "red/green fringe at weight {weight}");
                            assert_eq!(pixel[1], pixel[2], "green/blue fringe at weight {weight}");
                            assert_eq!(pixel[3], 0, "coverage-mask alpha stays unused");
                            coverage_ink += u64::from(pixel[0]);
                        }
                    }
                    assert!(coverage_ink > 0, "specimen must retain visible ink");
                    weight_ink.push(coverage_ink);
                }
                assert!(
                    weight_ink.windows(2).all(|pair| pair[0] < pair[1]),
                    "400/500/700 coverage must increase at {css_size}px @ {device_scale}x"
                );
            }
        }
    }

    #[test]
    fn grayscale_subpixel_input_preserves_empty_and_full_coverage() {
        let data =
            strengthen_grayscale_subpixel_rgba8(&[0, 0, 0, 0, 255, 255, 255, 0, 64, 128, 192, 0]);
        assert_eq!(&data[0..4], &[0, 0, 0, 0]);
        assert_eq!(&data[4..8], &[255, 255, 255, 0]);
        assert_eq!(data[8], data[9]);
        assert_eq!(data[9], data[10]);
    }

    #[test]
    fn standalone_browser_symbols_floor_advance_to_font_size() {
        assert_eq!(
            JagTextProvider::browser_symbol_advance_floor("▲", 10.0),
            Some(10.0)
        );
        assert_eq!(
            JagTextProvider::browser_symbol_advance_floor("A", 10.0),
            None
        );
        assert_eq!(
            JagTextProvider::browser_symbol_advance_floor("▲▲", 10.0),
            None
        );
    }

    #[test]
    fn system_provider_uses_script_fallback_for_indic_text_when_available() {
        let provider = JagTextProvider::from_system_fonts(SubpixelOrientation::RGB);
        if provider.is_err() {
            return;
        }
        let provider = provider.unwrap();
        let Some(face) =
            provider.resolve_fallback_face_for_char('ह', 400, crate::scene::FontStyle::Normal)
        else {
            return;
        };
        assert!(JagTextProvider::face_supports_char(&face, 'ह'));

        let run = crate::scene::TextRun {
            source: None,
            spacing: None,
            rtl: None,
            text: "हिन्दी".to_string(),
            pos: [0.0, 0.0],
            size: 14.0,
            logical_size: 14.0,
            color: crate::scene::ColorLinPremul::rgba(255, 255, 255, 255),
            weight: 400.0,
            style: crate::scene::FontStyle::Normal,
            family: Some("Arial, sans-serif".to_string()),
        };

        assert!(provider.measure_run(&run) > 0.0);
        assert!(
            !provider.rasterize_run(&run).is_empty(),
            "Indic text should rasterize through a script fallback face instead of becoming empty/tofu"
        );
    }

    #[test]
    fn fallback_segmentation_preserves_indic_graphemes() {
        let provider = JagTextProvider::from_system_fonts(SubpixelOrientation::RGB);
        if provider.is_err() {
            return;
        }
        let provider = provider.unwrap();
        let run = crate::scene::TextRun {
            source: None,
            spacing: None,
            rtl: None,
            text: "हिन्दी".to_string(),
            pos: [0.0, 0.0],
            size: 14.0,
            logical_size: 14.0,
            color: crate::scene::ColorLinPremul::rgba(255, 255, 255, 255),
            weight: 400.0,
            style: crate::scene::FontStyle::Normal,
            family: Some("Arial, sans-serif".to_string()),
        };
        let primary = provider.select_face(&run);
        let segments = provider.face_segments_for_run(&run, &primary);

        assert_eq!(
            segments.len(),
            1,
            "Indic base letters and dependent marks must be shaped in one font segment"
        );
        assert_eq!(segments[0].start, 0);
        assert_eq!(segments[0].end, run.text.len());
    }

    #[test]
    fn system_provider_selects_distinct_face_for_monospace_family() {
        let provider = JagTextProvider::from_system_fonts(SubpixelOrientation::RGB);
        if provider.is_err() {
            // Skip on systems without fonts (CI containers)
            return;
        }
        let provider = provider.unwrap();

        let base_run = crate::scene::TextRun {
            source: None,
            spacing: None,
            rtl: None,
            text: "font-family".to_string(),
            pos: [0.0, 0.0],
            size: 14.0,
            logical_size: 14.0,
            color: crate::scene::ColorLinPremul::rgba(255, 255, 255, 255),
            weight: 400.0,
            style: crate::scene::FontStyle::Normal,
            family: Some(
                "-apple-system, BlinkMacSystemFont, \"Segoe UI\", Roboto, sans-serif".to_string(),
            ),
        };
        let mono_run = crate::scene::TextRun {
            family: Some("\"SF Mono\", \"Fira Code\", \"Cascadia Code\", monospace".to_string()),
            ..base_run.clone()
        };

        let default_face = provider.select_face(&base_run);
        let mono_face = provider.select_face(&mono_run);

        assert!(
            default_face.index() != mono_face.index()
                || default_face.as_bytes() != mono_face.as_bytes(),
            "monospace family should resolve to a different face than the default sans stack"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn system_provider_resolves_hidden_sf_mono_for_explicit_family() {
        let sf_mono_path = std::path::Path::new("/System/Library/Fonts/SFNSMono.ttf");
        if !sf_mono_path.exists() {
            return;
        }

        let provider = JagTextProvider::from_system_fonts(SubpixelOrientation::RGB);
        if provider.is_err() {
            return;
        }
        let provider = provider.unwrap();

        let run = crate::scene::TextRun {
            source: None,
            spacing: None,
            rtl: None,
            text: "mono".to_string(),
            pos: [0.0, 0.0],
            size: 14.0,
            logical_size: 14.0,
            color: crate::scene::ColorLinPremul::rgba(255, 255, 255, 255),
            weight: 400.0,
            style: crate::scene::FontStyle::Normal,
            family: Some("\"SF Mono\", monospace".to_string()),
        };

        let selected = provider.select_face(&run);
        let expected = std::fs::read(sf_mono_path).expect("read SFNSMono.ttf");

        assert_eq!(
            selected.as_bytes().as_ref(),
            expected.as_slice(),
            "explicit SF Mono family should resolve to the hidden SFNSMono font on macOS"
        );
    }

    #[test]
    fn register_web_font_invalid_data_fails() {
        let provider = JagTextProvider::from_system_fonts(SubpixelOrientation::RGB);
        if provider.is_err() {
            // Skip on systems without fonts (CI containers)
            return;
        }
        let provider = provider.unwrap();

        // Invalid font data should return error
        let result = provider.register_web_font(
            "TestFont",
            vec![0, 0, 0, 0],
            400,
            crate::scene::FontStyle::Normal,
        );
        assert!(result.is_err(), "Invalid font data should return error");
    }
}
