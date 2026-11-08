use super::inverse_mask::outer_shadow_mask_pixels;
use jag_draw::{Rect, RoundedRadii, RoundedRect};

/// The original per-sample rasterizer, kept as the reference the fast path must match.
fn reference(
    mapped: Rect,
    width: u32,
    height: u32,
    inverse: [f32; 6],
    owner: RoundedRect,
) -> Vec<u8> {
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height {
        for x in 0..width {
            let mut outside = 0u32;
            for sy in 0..2 {
                for sx in 0..2 {
                    let world = [
                        mapped.x + (x as f32 + (sx as f32 + 0.5) / 2.0) * mapped.w / width as f32,
                        mapped.y
                            + (height as f32 - y as f32 - (sy as f32 + 0.5) / 2.0) * mapped.h
                                / height as f32,
                    ];
                    let [a, b, c, d, e, f] = inverse;
                    let p = [
                        a * world[0] + c * world[1] + e,
                        b * world[0] + d * world[1] + f,
                    ];
                    if !contains(owner, p) {
                        outside += 1;
                    }
                }
            }
            let i = (y as usize * width as usize + x as usize) * 4;
            pixels[i..i + 3].fill(255);
            pixels[i + 3] = (outside * 255 / 4) as u8;
        }
    }
    pixels
}

fn contains(shape: RoundedRect, p: [f32; 2]) -> bool {
    let r = shape.rect;
    let (x, y) = (p[0] - r.x, p[1] - r.y);
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
        let (dx, dy) = ((x - cx).abs(), (y - cy).abs());
        if dx < radius && dy < radius {
            return (dx - radius).powi(2) + (dy - radius).powi(2) <= radius * radius;
        }
    }
    true
}

fn owner(rect: Rect, tl: f32, tr: f32, br: f32, bl: f32) -> RoundedRect {
    RoundedRect {
        rect,
        radii: RoundedRadii { tl, tr, br, bl },
    }
}

#[test]
fn fast_mask_matches_reference_for_axis_aligned_and_rotated_transforms() {
    let bounds = Rect {
        x: 10.0,
        y: 20.0,
        w: 97.0,
        h: 61.0,
    };
    let shape = owner(
        Rect {
            x: 22.0,
            y: 31.3,
            w: 70.5,
            h: 38.2,
        },
        14.0,
        3.0,
        0.0,
        40.0,
    );
    let cases: [[f32; 6]; 4] = [
        [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, -1.0, 0.0, 120.0],
        [0.5, 0.0, 0.0, 0.5, -3.25, 7.5],
        [0.9, 0.3, -0.3, 0.9, 0.0, 0.0],
    ];
    for inverse in cases {
        let (width, height) = (97u32, 61u32);
        assert_eq!(
            outer_shadow_mask_pixels(bounds, width, height, inverse, shape),
            reference(bounds, width, height, inverse, shape),
            "inverse {inverse:?}"
        );
    }
}
