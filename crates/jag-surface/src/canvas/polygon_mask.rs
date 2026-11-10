//! Antialiased polygon scopes reuse the generated-mask compositor and cache.
use super::{Canvas, GeneratedMaskTexture};
use jag_draw::{ExternalTextureId, FilterEffect, MaskEffect, MaskMode, MaskTextureMapping, Rect};

impl Canvas {
    /// Clip all ink in a scope to a local-space polygon, including concave paths.
    pub fn push_polygon_mask(&mut self, bounds: Rect, points: &[[f32; 2]], evenodd: bool) -> bool {
        let transform = self.current_transform().m;
        let Some(inverse) = super::masks::inverse_transform(transform) else {
            return false;
        };
        let scale = super::mask_cache::raster_scale(transform, self.dpi_scale);
        let Some([width, height]) =
            super::mask_size::raster_size([bounds.w, bounds.h], scale, self.max_mask_dimension)
        else {
            return false;
        };
        let local: Vec<_> = points
            .iter()
            .map(|p| [p[0] - bounds.x, p[1] - bounds.y])
            .collect();
        let mut key = vec![
            2,
            width,
            height,
            u32::from(evenodd),
            bounds.w.to_bits(),
            bounds.h.to_bits(),
        ];
        key.extend(local.iter().flatten().map(|v| v.to_bits()));
        let pixels = super::mask_cache::pixels(key, || {
            raster([bounds.w, bounds.h], &local, evenodd, width, height)
        });
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
            rect: super::masks::transformed_bounds(bounds, transform),
            mapping: Some(MaskTextureMapping {
                inverse_transform: inverse,
                paint_rect: bounds,
                tile_rect: bounds,
                tile_step: [bounds.w, bounds.h],
                repeat_axes: [false; 2],
                flip_y: false,
            }),
        }));
        true
    }
}
fn inside(points: &[[f32; 2]], x: f32, y: f32, evenodd: bool) -> bool {
    let mut winding = 0i32;
    for (a, b) in points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
    {
        let cross = (b[0] - a[0]) * (y - a[1]) - (x - a[0]) * (b[1] - a[1]);
        if a[1] <= y && b[1] > y && cross > 0. {
            winding += 1;
        }
        if a[1] > y && b[1] <= y && cross < 0. {
            winding -= 1;
        }
    }
    if evenodd {
        winding % 2 != 0
    } else {
        winding != 0
    }
}
fn raster(size: [f32; 2], points: &[[f32; 2]], evenodd: bool, width: u32, height: u32) -> Vec<u8> {
    let mut pixels = vec![255; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let mut coverage = 0u32;
            for sy in 0..4 {
                for sx in 0..4 {
                    let px = (x as f32 + (sx as f32 + 0.5) / 4.) * size[0] / width as f32;
                    let py = (height as f32 - y as f32 - (sy as f32 + 0.5) / 4.) * size[1]
                        / height as f32;
                    coverage += u32::from(inside(points, px, py, evenodd));
                }
            }
            pixels[((y * width + x) * 4 + 3) as usize] = (coverage * 255 / 16) as u8;
        }
    }
    pixels
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concave_star_preserves_center_and_cuts_out_corners() {
        let points = [
            [6., 0.],
            [4.31, 3.98],
            [0., 4.34],
            [3.28, 7.18],
            [2.29, 11.4],
            [6., 9.16],
            [9.71, 11.4],
            [8.72, 7.18],
            [12., 4.34],
            [7.69, 3.98],
        ];
        let p = raster([12., 12.], &points, false, 24, 24);
        let alpha = |x: usize, y: usize| p[(y * 24 + x) * 4 + 3];
        assert_eq!(alpha(12, 12), 255);
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(23, 23), 0);
        assert!(p.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255));
    }
    #[test]
    fn fill_rules_and_degenerate_polygons_keep_correct_alpha() {
        let twice = [
            [0., 0.],
            [10., 0.],
            [10., 10.],
            [0., 10.],
            [0., 0.],
            [10., 0.],
            [10., 10.],
            [0., 10.],
        ];
        assert!(inside(&twice, 5., 5., false));
        assert!(!inside(&twice, 5., 5., true));
        assert!(
            raster([10., 10.], &[], false, 10, 10)
                .chunks_exact(4)
                .all(|p| p[3] == 0)
        );
    }
}
