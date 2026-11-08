impl JagTextProvider {
    pub fn from_bytes(bytes: &[u8], orientation: SubpixelOrientation) -> anyhow::Result<Self> {
        let font = jag_text::FontFace::from_vec(bytes.to_vec(), 0)?;
        Ok(Self {
            font,
            bold_font: None,
            italic_font: None,
            mono_font: None,
            emoji_font: None.into(),
            orientation,
            antialiasing: TextAntialiasing::platform_default(),
            font_db: None,
            font_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
            char_fallback_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
            font_generation: std::sync::atomic::AtomicU64::new(0),
        })
    }

    /// Create a provider with a primary font and an emoji font for fallback.
    pub fn from_bytes_with_emoji(
        bytes: &[u8],
        emoji_bytes: &[u8],
        orientation: SubpixelOrientation,
    ) -> anyhow::Result<Self> {
        let font = jag_text::FontFace::from_vec(bytes.to_vec(), 0)?;
        let emoji_font = jag_text::FontFace::from_vec(emoji_bytes.to_vec(), 0)?;
        Ok(Self {
            font,
            bold_font: None,
            italic_font: None,
            mono_font: None,
            emoji_font: Some(emoji_font).into(),
            orientation,
            antialiasing: TextAntialiasing::platform_default(),
            font_db: None,
            font_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
            char_fallback_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
            font_generation: std::sync::atomic::AtomicU64::new(0),
        })
    }

    /// Construct from a reasonable system sans-serif font using `fontdb`.
    /// Loads the system emoji fallback only when missing-glyph rendering needs it.
    pub fn from_system_fonts(orientation: SubpixelOrientation) -> anyhow::Result<Self> {
        use fontdb::{Database, Family, Query, Stretch, Style, Weight};

        let mut db = Database::new();
        db.load_system_fonts();

        // macOS: Apple's SF system font (SFNS.ttf) is hidden from standard
        // font enumeration but is readable on disk. Load it explicitly so
        // `from_system_fonts` matches Chrome's default sans-serif.
        #[cfg(target_os = "macos")]
        {
            let sfns_path = std::path::Path::new("/System/Library/Fonts/SFNS.ttf");
            if sfns_path.exists() {
                db.load_font_file(sfns_path).ok();
            }
            let sfns_italic = std::path::Path::new("/System/Library/Fonts/SFNSItalic.ttf");
            if sfns_italic.exists() {
                db.load_font_file(sfns_italic).ok();
            }
            let sfns_mono = std::path::Path::new("/System/Library/Fonts/SFNSMono.ttf");
            if sfns_mono.exists() {
                db.load_font_file(sfns_mono).ok();
            }
            let sfns_mono_italic = std::path::Path::new("/System/Library/Fonts/SFNSMonoItalic.ttf");
            if sfns_mono_italic.exists() {
                db.load_font_file(sfns_mono_italic).ok();
            }
        }

        // Load primary text font (regular weight)
        let id = db
            .query(&Query {
                families: &[
                    // Prefer the modern macOS UI font (SF Pro / .AppleSystemUIFont)
                    // so HarfBuzz advances match Chrome's `system-ui` resolution.
                    // Fall back to the legacy SFNS.ttf (registered as `System
                    // Font` after explicit load) only on older macOS where the
                    // newer faces aren't enumerable in fontdb.
                    Family::Name(".AppleSystemUIFont".into()),
                    Family::Name("SF Pro Text".into()),
                    Family::Name(".SF NS Text".into()),
                    Family::Name("System Font".into()),
                    // Windows system UI font
                    Family::Name("Segoe UI".into()),
                    // Generic fallbacks
                    Family::SansSerif,
                    Family::Name("Arial".into()),
                    Family::Name("Helvetica Neue".into()),
                    // Linux: fontdb does not apply fontconfig aliases, so
                    // `Family::SansSerif` ("Arial") misses on Debian.
                    Family::Name("Noto Sans".into()),
                    Family::Name("DejaVu Sans".into()),
                    Family::Name("Liberation Sans".into()),
                ],
                weight: Weight::NORMAL,
                stretch: Stretch::Normal,
                style: Style::Normal,
                ..Query::default()
            })
            .ok_or_else(|| anyhow::anyhow!("no suitable system font found for jag-text"))?;

        let face = db
            .face(id)
            .ok_or_else(|| anyhow::anyhow!("fontdb face missing for system font id"))?;

        let font = Self::load_face_from_db(face)
            .ok_or_else(|| anyhow::anyhow!("failed to load system font"))?;

        // Try to load a matching bold face from the same family (if available).
        let primary_family = face.families.first().map(|(name, _lang)| name.clone());
        let bold_font = primary_family
            .as_deref()
            .and_then(|family_name| {
                db.query(&Query {
                    families: &[Family::Name(family_name)],
                    weight: Weight::BOLD,
                    stretch: Stretch::Normal,
                    style: Style::Normal,
                    ..Query::default()
                })
            })
            .and_then(|bold_id| db.face(bold_id))
            .and_then(Self::load_face_from_db);

        // Try to load a matching italic face from the same family (if available).
        let italic_font = primary_family
            .as_deref()
            .and_then(|family_name| {
                db.query(&Query {
                    families: &[Family::Name(family_name)],
                    weight: Weight::NORMAL,
                    stretch: Stretch::Normal,
                    style: Style::Italic,
                    ..Query::default()
                })
            })
            .and_then(|italic_id| db.face(italic_id))
            .and_then(Self::load_face_from_db);

        // Try to load a monospace font for code spans
        let mono_font = db
            .query(&Query {
                families: &[
                    // macOS
                    Family::Name("SF Mono".into()),
                    Family::Name(".SF NS Mono".into()),
                    Family::Name("Menlo".into()),
                    Family::Name("Monaco".into()),
                    // Windows
                    Family::Name("Cascadia Code".into()),
                    Family::Name("Consolas".into()),
                    // Linux
                    Family::Name("DejaVu Sans Mono".into()),
                    Family::Name("Liberation Mono".into()),
                    Family::Monospace,
                ],
                weight: Weight::NORMAL,
                stretch: Stretch::Normal,
                style: Style::Normal,
                ..Query::default()
            })
            .and_then(|mono_id| db.face(mono_id))
            .and_then(Self::load_face_from_db);

        Ok(Self {
            font,
            bold_font,
            italic_font,
            mono_font,
            emoji_font: std::sync::OnceLock::new(),
            orientation,
            antialiasing: TextAntialiasing::platform_default(),
            font_db: Some(db),
            font_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
            char_fallback_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
            font_generation: std::sync::atomic::AtomicU64::new(0),
        })
    }
}
