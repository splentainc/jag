//! Clip stacks for direct draws and retained command streams.
use super::{Canvas, RoundedRectClip, helpers::intersect_rect};
use jag_draw::{Rect, RoundedRect};

impl Canvas {
    // Expose some painter helpers for advanced users
    pub fn push_clip_rect(&mut self, rect: Rect) {
        self.push_clip_rect_inner(rect);
        // No rounded clip for plain rect clips.
        self.rounded_clip_stack.push(None);
    }

    /// Push a rounded-rect clip.  The AABB is used for scissor-based coarse
    /// clipping; the full rounded rect (with per-corner radii) is forwarded
    /// to image draws for SDF-based fragment discard in the shader.
    pub fn push_clip_rounded_rect(&mut self, rrect: RoundedRect) {
        self.push_clip_rect_inner(rrect.rect);
        // Compute device-space rounded clip for the image shader.
        let s = self.dpi_scale;
        let t = self.painter.current_transform();
        let [a, _b, _c, d, e, f] = t.m;
        // Assumes axis-aligned transform (translation + uniform scale).
        let sx = a.abs() * s;
        let sy = d.abs() * s;
        let dev_rect = Rect {
            x: (rrect.rect.x * a + e) * s,
            y: (rrect.rect.y * d + f) * s,
            w: rrect.rect.w * sx,
            h: rrect.rect.h * sy,
        };
        let scale_r = sx.min(sy); // uniform radius scale
        let dev_radii = [
            rrect.radii.tl * scale_r,
            rrect.radii.tr * scale_r,
            rrect.radii.br * scale_r,
            rrect.radii.bl * scale_r,
        ];
        self.rounded_clip_stack.push(Some(RoundedRectClip {
            rect: dev_rect,
            radii: dev_radii,
        }));
        self.painter
            .extend_commands(&[jag_draw::Command::ScrollClipRadii(dev_radii)]);
    }

    fn push_clip_rect_inner(&mut self, rect: Rect) {
        // Forward to Painter to keep display list behavior.
        self.painter.push_clip_rect(rect);

        // Compute device-space clip rect based on current transform and dpi.
        let t = self.painter.current_transform();
        let [a, b, c, d, e, f] = t.m;

        let x0 = rect.x;
        let y0 = rect.y;
        let x1 = rect.x + rect.w;
        let y1 = rect.y + rect.h;

        let p0 = [a * x0 + c * y0 + e, b * x0 + d * y0 + f];
        let p1 = [a * x1 + c * y0 + e, b * x1 + d * y0 + f];
        let p2 = [a * x0 + c * y1 + e, b * x0 + d * y1 + f];
        let p3 = [a * x1 + c * y1 + e, b * x1 + d * y1 + f];

        let min_x = p0[0].min(p1[0]).min(p2[0]).min(p3[0]) * self.dpi_scale;
        let max_x = p0[0].max(p1[0]).max(p2[0]).max(p3[0]) * self.dpi_scale;
        let min_y = p0[1].min(p1[1]).min(p2[1]).min(p3[1]) * self.dpi_scale;
        let max_y = p0[1].max(p1[1]).max(p2[1]).max(p3[1]) * self.dpi_scale;

        let new_clip = Rect {
            x: min_x,
            y: min_y,
            w: (max_x - min_x).max(0.0),
            h: (max_y - min_y).max(0.0),
        };

        let merged = match self.clip_stack.last().cloned().unwrap_or(None) {
            None => Some(new_clip),
            Some(prev) => Some(intersect_rect(prev, new_clip).unwrap_or(Rect {
                x: prev.x,
                y: prev.y,
                w: 0.0,
                h: 0.0,
            })),
        };
        self.clip_stack.push(merged);
    }

    pub fn pop_clip(&mut self) {
        self.painter.pop_clip();
        if self.clip_stack.len() > 1 {
            self.clip_stack.pop();
        }
        if self.rounded_clip_stack.len() > 1 {
            self.rounded_clip_stack.pop();
        }
    }
}
