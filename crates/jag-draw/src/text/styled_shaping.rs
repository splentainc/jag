fn shape_styled_segment(
    run: &crate::scene::TextRun,
    text: &str,
    face: &jag_text::FontFace,
    variations: &[(harfrust::Tag, f32)],
) -> jag_text::shaping::ShapedRun {
    use jag_text::shaping::{TextShaper, hb_tag_from_bytes};
    use unicode_segmentation::UnicodeSegmentation;
    let spacing = run.spacing.unwrap_or(crate::scene::TextSpacing {
        letter: 0.0,
        word: 0.0,
    });
    let features = if spacing.letter != 0.0 {
        vec![
            harfrust::Feature::new(hb_tag_from_bytes(b"liga"), 0, ..),
            harfrust::Feature::new(hb_tag_from_bytes(b"clig"), 0, ..),
        ]
    } else {
        Vec::new()
    };
    let mut shaped = TextShaper::shape_with_features(
        text,
        0..text.len(),
        face,
        0,
        run.size.max(1.0),
        variations,
        run.rtl,
        &features,
    );
    if spacing.letter == 0.0 && spacing.word == 0.0 {
        return shaped;
    }
    let cursive = matches!(shaped.script, jag_text::shaping::Script::Arabic)
        || matches!(shaped.script, jag_text::shaping::Script::Other(tag) if [*b"Syrc", *b"Mand", *b"Mong", *b"Nkoo", *b"Phag", *b"Rohg"].contains(&tag));
    let mut boundaries: Vec<_> = shaped
        .clusters
        .iter()
        .map(|&c| c as usize)
        .chain([text.len()])
        .collect();
    boundaries.sort_unstable();
    boundaries.dedup();
    let mut extra = 0.0;
    for i in 0..shaped.glyphs.len() {
        shaped.positions[i].x_offset += extra;
        let cluster = shaped.clusters[i] as usize;
        if shaped
            .clusters
            .get(i + 1)
            .is_some_and(|&next| next as usize == cluster)
        {
            continue;
        }
        let end = *boundaries.iter().find(|&&end| end > cluster).unwrap();
        let unit = &text[cluster..end];
        let letters = if cursive
            && !unit.chars().all(char::is_whitespace)
            && text
                .get(end..)
                .is_some_and(|tail| tail.chars().next().is_some_and(|ch| !ch.is_whitespace()))
        {
            0
        } else {
            1
        };
        let words = unit
            .graphemes(true)
            .filter(|g| g.chars().all(|ch| matches!(ch, ' ' | '\t' | '\u{a0}')))
            .count();
        let advance = (spacing.letter * letters as f32 + spacing.word * words as f32) * run.size;
        shaped.advances[i] += advance;
        extra += advance;
    }
    shaped.width += extra;
    shaped
}
