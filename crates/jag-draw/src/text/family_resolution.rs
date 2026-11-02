impl JagTextProvider {
    /// Resolve a single font family candidate against the `fontdb` database,
    /// loading regular + bold + italic variants. Returns `None` if the family
    /// is not installed.

    fn resolve_family(
        db: &fontdb::Database,
        candidate: &FontFamilyCandidate,
    ) -> Option<CachedFontSet> {
        use fontdb::{Family, Query, Stretch, Style, Weight};

        let families: Vec<Family<'_>> = match candidate {
            FontFamilyCandidate::Name(name) => vec![Family::Name(name.as_str())],
            FontFamilyCandidate::Generic(g) => match g {
                GenericFamily::Serif => vec![
                    Family::Serif,
                    Family::Name("Georgia"),
                    Family::Name("Times New Roman"),
                    Family::Name("Times"),
                ],
                GenericFamily::SansSerif => vec![
                    Family::Name("System Font"),
                    Family::Name("SF Pro Text"),
                    Family::Name("Segoe UI"),
                    Family::SansSerif,
                    Family::Name("Arial"),
                    Family::Name("Helvetica Neue"),
                    Family::Name("Noto Sans"),
                    Family::Name("DejaVu Sans"),
                    Family::Name("Liberation Sans"),
                ],
                GenericFamily::Monospace => vec![
                    Family::Name("SF Mono"),
                    Family::Name(".SF NS Mono"),
                    Family::Name("Menlo"),
                    Family::Name("Monaco"),
                    Family::Name("Cascadia Code"),
                    Family::Name("Consolas"),
                    Family::Name("DejaVu Sans Mono"),
                    Family::Monospace,
                ],
                GenericFamily::SystemUi => vec![
                    // Match Chrome's `system-ui` on macOS — it routes through
                    // CoreText's `.AppleSystemUIFont` virtual font, landing on
                    // SF Pro Text/Display. The legacy SFNS.ttf (registered as
                    // `System Font` after explicit load) has wider advances
                    // and used to be first here; keep it as a fallback for
                    // older macOS where SF Pro isn't enumerable in fontdb.
                    Family::Name(".AppleSystemUIFont"),
                    Family::Name("SF Pro Text"),
                    Family::Name(".SF NS Text"),
                    Family::Name("SF Pro Display"),
                    Family::Name(".SF NS Display"),
                    Family::Name("System Font"),
                    Family::Name("Segoe UI"),
                    Family::Name("system-ui"),
                    Family::Name("-apple-system"),
                    Family::Name("BlinkMacSystemFont"),
                    Family::SansSerif,
                    Family::Name("Arial"),
                    Family::Name("Helvetica Neue"),
                    Family::Name("Noto Sans"),
                    Family::Name("DejaVu Sans"),
                    Family::Name("Liberation Sans"),
                ],
                GenericFamily::Cursive => vec![
                    Family::Cursive,
                    Family::Name("Snell Roundhand"),
                    Family::Name("Comic Sans MS"),
                ],
                GenericFamily::Fantasy => vec![
                    Family::Fantasy,
                    Family::Name("Papyrus"),
                    Family::Name("Impact"),
                ],
            },
        };

        let regular_id = db.query(&Query {
            families: &families,
            weight: Weight::NORMAL,
            stretch: Stretch::Normal,
            style: Style::Normal,
        })?;

        let regular_face = db.face(regular_id)?;
        let regular = Self::load_face_from_db(regular_face)?;

        if std::env::var("JAG_TEXT_DEBUG_FAMILY").is_ok() {
            let resolved = regular_face
                .families
                .first()
                .map(|(name, _)| name.as_str())
                .unwrap_or("<unknown>");
            eprintln!("[TEXT] resolve_family {:?} -> {}", candidate, resolved);
        }

        // Resolve bold + italic from the same resolved family name.
        let resolved_family = regular_face.families.first().map(|(name, _)| name.as_str());

        let bold = resolved_family.and_then(|fam| {
            let id = db.query(&Query {
                families: &[Family::Name(fam)],
                weight: Weight::BOLD,
                stretch: Stretch::Normal,
                style: Style::Normal,
            })?;
            Self::load_face_from_db(db.face(id)?)
        });
        let italic = resolved_family.and_then(|fam| {
            let id = db.query(&Query {
                families: &[Family::Name(fam)],
                weight: Weight::NORMAL,
                stretch: Stretch::Normal,
                style: Style::Italic,
            })?;
            Self::load_face_from_db(db.face(id)?)
        });

        let mut set = CachedFontSet {
            upright_faces: vec![(400, regular)],
            italic_faces: Vec::new(),
        };
        if let Some(bold_face) = bold {
            Self::insert_weighted_face(&mut set.upright_faces, 700, bold_face);
        }
        if let Some(italic_face) = italic {
            Self::insert_weighted_face(&mut set.italic_faces, 400, italic_face);
        }

        Some(set)
    }

    /// Cache key for a font family candidate (lowercase for case-insensitive matching).
    fn cache_key_for(candidate: &FontFamilyCandidate) -> String {
        match candidate {
            FontFamilyCandidate::Name(n) => n.to_ascii_lowercase(),
            FontFamilyCandidate::Generic(g) => match g {
                GenericFamily::Serif => "__generic_serif__".to_string(),
                GenericFamily::SansSerif => "__generic_sans-serif__".to_string(),
                GenericFamily::Monospace => "__generic_monospace__".to_string(),
                GenericFamily::SystemUi => "__generic_system-ui__".to_string(),
                GenericFamily::Cursive => "__generic_cursive__".to_string(),
                GenericFamily::Fantasy => "__generic_fantasy__".to_string(),
            },
        }
    }

    fn insert_weighted_face(
        faces: &mut Vec<(u16, jag_text::FontFace)>,
        weight: u16,
        face: jag_text::FontFace,
    ) {
        if let Some(pos) = faces.iter().position(|(w, _)| *w == weight) {
            faces[pos] = (weight, face);
        } else {
            faces.push((weight, face));
        }
        faces.sort_by_key(|(w, _)| *w);
    }

    fn pick_closest_weighted_face(
        faces: &[(u16, jag_text::FontFace)],
        requested_weight: u16,
    ) -> Option<jag_text::FontFace> {
        let mut best: Option<(u16, &jag_text::FontFace)> = None;
        for (weight, face) in faces {
            match best {
                None => best = Some((*weight, face)),
                Some((best_weight, _)) => {
                    let best_dist = (i32::from(best_weight) - i32::from(requested_weight)).abs();
                    let new_dist = (i32::from(*weight) - i32::from(requested_weight)).abs();
                    if new_dist < best_dist || (new_dist == best_dist && *weight > best_weight) {
                        best = Some((*weight, face));
                    }
                }
            }
        }
        best.map(|(_, face)| face.clone())
    }
}
