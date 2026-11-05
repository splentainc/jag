//! Convert tiny-skia's sRGB-premultiplied pixels for linear GPU blending.
//! Each channel depends only on its byte and alpha. Reuse the exact conversion
//! instead of repeating three transfer functions for every SVG frame pixel.
use std::sync::OnceLock;

fn reference(channel: u8, alpha: u8) -> u8 {
    let pixel = tiny_skia::PremultipliedColorU8::from_rgba(channel, 0, 0, alpha).unwrap();
    let straight = pixel.demultiply();
    let linear = crate::ColorLinPremul::from_srgba_u8([straight.red(), 0, 0, alpha]);
    let encoded: palette::Srgb<f32> =
        palette::Srgb::from_linear(palette::LinSrgb::new(linear.r, 0., 0.));
    (encoded.red * 255.).round() as u8
}

pub(crate) fn rgba(pixels: &[tiny_skia::PremultipliedColorU8]) -> Vec<u8> {
    static TABLE: OnceLock<Box<[u8; 65536]>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = Box::new([0; 65536]);
        for alpha in 1..255u8 {
            for channel in 0..=alpha {
                table[alpha as usize * 256 + channel as usize] = reference(channel, alpha);
            }
        }
        table
    });
    let mut out = Vec::with_capacity(pixels.len() * 4);
    for pixel in pixels {
        let a = pixel.alpha();
        let color = if a == 255 {
            [pixel.red(), pixel.green(), pixel.blue(), a]
        } else if a == 0 {
            [0; 4]
        } else {
            let offset = a as usize * 256;
            [
                table[offset + pixel.red() as usize],
                table[offset + pixel.green() as usize],
                table[offset + pixel.blue() as usize],
                a,
            ]
        };
        out.extend_from_slice(&color);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn svg_pixel_table_matches_all_valid_channel_alpha_pairs() {
        for alpha in 0..=255u8 {
            let pixels: Vec<_> = (0..=alpha)
                .map(|channel| {
                    tiny_skia::PremultipliedColorU8::from_rgba(channel, channel, channel, alpha)
                        .unwrap()
                })
                .collect();
            let converted = rgba(&pixels);
            for channel in 0..=alpha {
                let value = reference(channel, alpha);
                assert_eq!(
                    &converted[channel as usize * 4..][..4],
                    &[value, value, value, alpha]
                );
            }
        }
    }
}
