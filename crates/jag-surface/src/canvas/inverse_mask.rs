//! Border-box knockout for CSS outer shadows, including transparent owners.
use super::{Canvas, GeneratedMaskTexture};
use jag_draw::{ExternalTextureId, FilterEffect, MaskEffect, MaskMode, Rect, RoundedRect};

impl Canvas {
    /// Begin an alpha mask covering `bounds` except the owning rounded border box.
    /// Both rectangles are local; rasterization applies the current affine transform.
    pub fn push_outer_shadow_mask(&mut self, bounds: Rect, owner: RoundedRect) -> bool {
        let transform = self.current_transform().m;
        let Some(inverse) = super::masks::inverse_transform(transform) else {
            return false;
        };
        let mapped = super::masks::transformed_bounds(bounds, transform);
        let width = (mapped.w * self.dpi_scale.max(1.0)).ceil().max(1.0) as u32;
        let height = (mapped.h * self.dpi_scale.max(1.0)).ceil().max(1.0) as u32;
        let pixels = outer_shadow_mask_pixels(mapped, width, height, inverse, owner);
        let id = ExternalTextureId(self.next_generated_mask_texture_id);
        self.next_generated_mask_texture_id = self.next_generated_mask_texture_id.wrapping_add(1);
        self.generated_mask_textures.push(GeneratedMaskTexture {
            id,
            width,
            height,
            pixels,
        });
        self.push_filter(FilterEffect::Mask(MaskEffect {
            texture_id: id,
            mode: MaskMode::Alpha,
            rect: mapped,
            mapping: None,
        }));
        true
    }
}
/// Alpha is the fraction of 2x2 samples outside `owner`; rgb is opaque white.
///
/// With an axis-aligned transform a sample's local x depends only on its column
/// and its local y only on its row, so rows clear of every rounded corner reduce
/// to a per-column lookup. Other transforms test every sample.
pub(crate) fn outer_shadow_mask_pixels(
    mapped: Rect,
    width: u32,
    height: u32,
    inverse: [f32; 6],
    owner: RoundedRect,
) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let [a, b, c, d, e, f] = inverse;
    let world_x = |x: usize, sx: usize| {
        mapped.x + (x as f32 + (sx as f32 + 0.5) / 2.0) * mapped.w / width as f32
    };
    let world_y = |y: usize, sy: usize| {
        mapped.y + (height as f32 - y as f32 - (sy as f32 + 0.5) / 2.0) * mapped.h / height as f32
    };
    let local = |wx: f32, wy: f32| [a * wx + c * wy + e, b * wx + d * wy + f];
    let axis_aligned = b == 0.0 && c == 0.0;
    let r = owner.rect;
    let corner_reach = [owner.radii.tl, owner.radii.tr, owner.radii.br, owner.radii.bl]
        .into_iter()
        .map(|radius| radius.min(r.w.min(r.h) / 2.0).max(0.0))
        .fold(0.0f32, f32::max);
    let column_inside: Vec<[bool; 2]> = if axis_aligned {
        (0..w)
            .map(|x| {
                [0, 1].map(|sx| {
                    let lx = local(world_x(x, sx), 0.0)[0] - r.x;
                    !(lx < 0.0 || lx >= r.w)
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    // Alpha of a row whose samples all use the column test: two sample rows per column pair.
    let clean_row_alpha: Vec<u8> = column_inside
        .iter()
        .map(|inside| {
            let outside: u32 = inside.iter().map(|&i| u32::from(!i)).sum::<u32>() * 2;
            (outside * 255 / 4) as u8
        })
        .collect();
    let mut pixels = vec![255u8; w * h * 4];
    for y in 0..h {
        // `None`: every sample of this row is outside. `Some(true)`: membership is
        // the column test. `Some(false)`: corners may matter, test each sample.
        let rows: [(f32, Option<bool>); 2] = [0, 1].map(|sy| {
            let wy = world_y(y, sy);
            if !axis_aligned {
                return (wy, Some(false));
            }
            let ly = local(0.0, wy)[1] - r.y;
            if ly < 0.0 || ly >= r.h {
                (wy, None)
            } else {
                (wy, Some(ly >= corner_reach && r.h - ly >= corner_reach))
            }
        });
        match (rows[0].1, rows[1].1) {
            // The buffer is already fully outside (alpha 255).
            (None, None) => continue,
            (Some(true), Some(true)) => {
                for (x, alpha) in clean_row_alpha.iter().enumerate() {
                    pixels[(y * w + x) * 4 + 3] = *alpha;
                }
                continue;
            }
            _ => {}
        }
        for x in 0..w {
            let mut outside = 0u32;
            for (wy, kind) in rows {
                for sx in 0..2 {
                    let is_inside = match kind {
                        None => false,
                        Some(true) => column_inside[x][sx],
                        Some(false) => inside(owner, local(world_x(x, sx), wy)),
                    };
                    outside += u32::from(!is_inside);
                }
            }
            pixels[(y * w + x) * 4 + 3] = (outside * 255 / 4) as u8;
        }
    }
    pixels
}

fn inside(shape: RoundedRect, p: [f32; 2]) -> bool {
    let r = shape.rect;
    let x = p[0] - r.x;
    let y = p[1] - r.y;
    if x < 0.0 || y < 0.0 || x >= r.w || y >= r.h {
        return false;
    }
    for (radius, cx, cy) in [
        (shape.radii.tl, 0.0, 0.0),
        (shape.radii.tr, r.w, 0.0),
        (shape.radii.br, r.w, r.h),
        (shape.radii.bl, 0.0, r.h),
    ] {
        let radius = radius.min(r.w.min(r.h) / 2.0).max(0.0);
        let dx = (x - cx).abs();
        let dy = (y - cy).abs();
        if dx < radius && dy < radius {
            return (dx - radius).powi(2) + (dy - radius).powi(2) <= radius * radius;
        }
    }
    true
}
