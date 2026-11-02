/// Select glyphs by source identity. Only a glyph shared by multiple source
/// ranges (a ligature) is clipped; ordinary italic overhang remains intact.
fn slice_glyphs(
    glyphs: &[RasterizedGlyph],
    slice: &crate::scene::TextSourceSlice,
    size: f32,
) -> Vec<RasterizedGlyph> {
    glyphs
        .iter()
        .filter_map(|glyph| {
            let source = glyph.source.as_ref()?;
            if source.start >= slice.bytes.end || source.end <= slice.bytes.start {
                return None;
            }
            if source.start >= slice.bytes.start && source.end <= slice.bytes.end {
                return Some(glyph.clone());
            }
            let (width, height, data, bpp) = match &glyph.mask {
                GlyphMask::Subpixel(m) => (m.width, m.height, &m.data, m.bytes_per_pixel()),
                GlyphMask::Color(m) => (m.width, m.height, &m.data, m.bytes_per_pixel()),
            };
            let left = slice.clip_start.map_or(0, |x| {
                ((x * size - glyph.offset[0])
                    .round()
                    .clamp(0.0, width as f32)) as u32
            });
            let right = slice.clip_end.map_or(width, |x| {
                ((x * size - glyph.offset[0])
                    .round()
                    .clamp(0.0, width as f32)) as u32
            });
            if left >= right {
                return None;
            }
            if left == 0 && right == width {
                return Some(glyph.clone());
            }
            let mut cropped = Vec::with_capacity(((right - left) * height) as usize * bpp);
            for row in 0..height {
                cropped.extend_from_slice(
                    &data[((row * width + left) as usize * bpp)
                        ..((row * width + right) as usize * bpp)],
                );
            }
            let mask = match &glyph.mask {
                GlyphMask::Subpixel(m) => GlyphMask::Subpixel(SubpixelMask {
                    width: right - left,
                    height,
                    format: m.format,
                    data: cropped,
                }),
                GlyphMask::Color(_) => GlyphMask::Color(ColorMask {
                    width: right - left,
                    height,
                    data: cropped,
                }),
            };
            Some(RasterizedGlyph {
                source: glyph.source.clone(),
                offset: [glyph.offset[0] + left as f32, glyph.offset[1]],
                mask,
            })
        })
        .collect()
}

#[cfg(test)]
mod source_slice_tests {
    use super::*;

    fn mask(source: core::ops::Range<usize>, x: f32, width: u32) -> RasterizedGlyph {
        RasterizedGlyph {
            source: Some(source),
            offset: [x, 0.0],
            mask: GlyphMask::Color(ColorMask {
                width,
                height: 1,
                data: vec![255; width as usize * 4],
            }),
        }
    }
    #[test]
    fn source_selection_keeps_italic_overhang_and_excludes_neighbor_ink() {
        let glyphs = vec![mask(0..1, 0.0, 7), mask(1..2, 4.0, 6)];
        let selected = slice_glyphs(
            &glyphs,
            &crate::scene::TextSourceSlice {
                bytes: 0..1,
                clip_start: None,
                clip_end: None,
            },
            1.0,
        );
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].mask.width(), 7);
        assert_eq!(selected[0].source, Some(0..1));
    }
    #[test]
    fn shared_ligature_source_ranges_partition_mask_columns_once() {
        let glyphs = vec![mask(0..3, 0.0, 10)];
        let mut next = 0.0;
        for index in 0..3 {
            let selected = slice_glyphs(
                &glyphs,
                &crate::scene::TextSourceSlice {
                    bytes: index..index + 1,
                    clip_start: (index > 0).then_some(index as f32 / 3.0),
                    clip_end: (index < 2).then_some((index + 1) as f32 / 3.0),
                },
                10.0,
            );
            assert_eq!(selected[0].offset[0], next);
            next += selected[0].mask.width() as f32;
        }
        assert_eq!(next, 10.0);
    }
}
