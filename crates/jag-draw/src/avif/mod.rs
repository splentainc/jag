//! Still AVIF images: container parsing (`avif-parse`), AV1 decoding (`rav1d`)
//! and conversion to straight-alpha RGBA8.
//!
//! Not handled, and reported as errors rather than drawn wrong: image grids,
//! animated sequences, HDR transfer functions, and files that carry rotation,
//! mirror or clean-aperture transforms. Colour primaries are not converted;
//! samples are treated as sRGB like the other raster formats here.

mod av1;
mod color;

use av1::Cicp;

/// ISOBMFF brands that mark a still AVIF file.
const AVIF_BRAND: &[u8; 4] = b"avif";

/// Child boxes of `data`, as `(type, payload)`. Stops at the first box whose
/// size does not fit.
fn boxes(mut data: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8])> {
    std::iter::from_fn(move || {
        let size = u32::from_be_bytes(data.get(..4)?.try_into().ok()?) as usize;
        let kind: [u8; 4] = data.get(4..8)?.try_into().ok()?;
        // ISO/IEC 14496-12 4.2: size 1 means a 64-bit size follows the type,
        // size 0 means the box runs to the end of its container.
        let (header, size) = match size {
            0 => (8, data.len()),
            1 => (
                16,
                usize::try_from(u64::from_be_bytes(data.get(8..16)?.try_into().ok()?)).ok()?,
            ),
            size => (8, size),
        };
        let payload = data.get(header..size)?;
        data = &data[size..];
        Some((kind, payload))
    })
}

fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(data)
        .find(|(k, _)| k == kind)
        .map(|(_, payload)| payload)
}

/// Whether `bytes` start with an ISOBMFF `ftyp` box naming the `avif` brand.
pub fn is_avif(bytes: &[u8]) -> bool {
    let Some((kind, payload)) = boxes(bytes).next() else {
        return false;
    };
    // `ftyp` payload: major brand, minor version, then compatible brands.
    kind == *b"ftyp"
        && payload
            .chunks_exact(4)
            .enumerate()
            .any(|(i, brand)| i != 1 && brand == AVIF_BRAND)
}

/// Properties in the file's `ipco` box that change how the image is shown.
struct Properties {
    /// The container's `colr`/`nclx` colour description, if it has one.
    cicp: Option<Cicp>,
}

fn properties(bytes: &[u8]) -> Result<Properties, String> {
    // `meta` is a full box: 4 bytes of version and flags precede its children.
    let ipco = child(bytes, b"meta")
        .and_then(|meta| meta.get(4..))
        .and_then(|meta| child(meta, b"iprp"))
        .and_then(|iprp| child(iprp, b"ipco"))
        .ok_or("AVIF has no item property box")?;
    let mut cicp = None;
    for (kind, payload) in boxes(ipco) {
        match &kind {
            b"irot" | b"imir" | b"clap" => {
                return Err(format!(
                    "AVIF {} transform is not supported",
                    String::from_utf8_lossy(&kind)
                ));
            }
            // ISO/IEC 14496-12 12.1.5: `nclx` carries primaries, transfer and
            // matrix as 16-bit codes, then the full-range flag in the top bit.
            b"colr" if payload.get(..4) == Some(b"nclx") => {
                let field = |at: usize| {
                    payload
                        .get(at..at + 2)
                        .map(|b| u32::from(u16::from_be_bytes([b[0], b[1]])))
                };
                let found = Cicp {
                    transfer: field(6).ok_or("truncated AVIF colr box")?,
                    matrix: field(8).ok_or("truncated AVIF colr box")?,
                    full_range: payload.get(10).ok_or("truncated AVIF colr box")? & 0x80 != 0,
                };
                if cicp.is_some_and(|earlier| earlier != found) {
                    return Err("AVIF items with differing colour descriptions".into());
                }
                cicp = Some(found);
            }
            _ => {}
        }
    }
    Ok(Properties { cicp })
}

/// Pixel size of a still AVIF, read from its AV1 sequence header.
pub fn avif_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let data = avif_parse::read_avif(&mut &bytes[..]).ok()?;
    let meta = data.primary_item_metadata().ok()?;
    Some((meta.max_frame_width.get(), meta.max_frame_height.get()))
}

/// Decode a still AVIF to straight-alpha RGBA8.
pub fn decode_avif(bytes: &[u8]) -> Result<image::RgbaImage, String> {
    let properties = properties(bytes)?;
    let data =
        avif_parse::read_avif(&mut &bytes[..]).map_err(|e| format!("AVIF container: {e}"))?;
    let color = av1::decode_frame(&data.primary_item)?;
    let alpha = data
        .alpha_item
        .as_deref()
        .map(av1::decode_frame)
        .transpose()?;
    // AVIF 4.2.3.1 (via ISO/IEC 23000-22): a `colr` box overrides the
    // colour description in the AV1 bitstream.
    let cicp = properties.cicp.unwrap_or(color.cicp);
    color::to_rgba(&color, cicp, alpha.as_ref(), data.premultiplied_alpha)
}
