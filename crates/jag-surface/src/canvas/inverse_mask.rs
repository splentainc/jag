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
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        for y in 0..height {
            for x in 0..width {
                let mut outside = 0u32;
                for sy in 0..2 {
                    for sx in 0..2 {
                        let world = [
                            mapped.x
                                + (x as f32 + (sx as f32 + 0.5) / 2.0) * mapped.w / width as f32,
                            mapped.y
                                + (height as f32 - y as f32 - (sy as f32 + 0.5) / 2.0) * mapped.h
                                    / height as f32,
                        ];
                        let [a, b, c, d, e, f] = inverse;
                        let p = [
                            a * world[0] + c * world[1] + e,
                            b * world[0] + d * world[1] + f,
                        ];
                        if !inside(owner, p) {
                            outside += 1;
                        }
                    }
                }
                let i = (y as usize * width as usize + x as usize) * 4;
                pixels[i..i + 3].fill(255);
                pixels[i + 3] = (outside * 255 / 4) as u8;
            }
        }
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
