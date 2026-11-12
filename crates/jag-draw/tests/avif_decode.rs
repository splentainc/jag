//! AVIF decoding against libavif's own output (`avifdec -d 8`, PNG files next
//! to each asset). Assets come from `avifenc` runs over `source*.png`.

use jag_draw::{avif_dimensions, decode_avif, is_avif};

fn asset(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/assets/avif/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Largest per-channel difference from libavif's decode of the same file.
fn worst_difference(name: &str) -> u8 {
    let bytes = asset(&format!("{name}.avif"));
    assert!(is_avif(&bytes));
    let decoded = decode_avif(&bytes).unwrap();
    let reference = image::load_from_memory(&asset(&format!("{name}.png")))
        .unwrap()
        .to_rgba8();
    assert_eq!(decoded.dimensions(), reference.dimensions());
    assert_eq!(avif_dimensions(&bytes), Some(reference.dimensions()));
    decoded
        .iter()
        .zip(reference.iter())
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap()
}

#[test]
fn lossless_identity_444_matches_libavif_exactly() {
    assert_eq!(worst_difference("lossless-444"), 0);
}

#[test]
fn monochrome_matches_libavif_exactly() {
    assert_eq!(worst_difference("mono-400"), 0);
}

#[test]
fn subsampled_limited_and_high_bit_depth_frames_match_libavif_within_rounding() {
    for name in ["lossy-420-limited-bt601", "lossy-422-10bit-bt709"] {
        let worst = worst_difference(name);
        assert!(worst <= 1, "{name}: channel off by {worst}");
    }
}

#[test]
fn alpha_item_becomes_the_alpha_channel() {
    assert!(worst_difference("alpha-420") <= 1);
    let decoded = decode_avif(&asset("alpha-420.avif")).unwrap();
    assert_eq!(decoded.get_pixel(0, 0).0[3], 0);
    assert_eq!(decoded.get_pixel(31, 0).0[3], 255);
}

#[test]
fn rotation_is_refused_instead_of_drawn_unrotated() {
    let error = decode_avif(&asset("rotated.avif")).unwrap_err();
    assert!(error.contains("irot"), "{error}");
}

#[test]
fn other_formats_and_truncated_files_are_not_avif() {
    assert!(!is_avif(&asset("source.png")));
    assert!(!is_avif(&asset("lossless-444.avif")[..8]));
}
