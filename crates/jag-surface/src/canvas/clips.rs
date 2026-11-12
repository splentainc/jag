//! Clip stacks for direct draws and retained command streams.
use super::{Canvas, RoundedRectClip, helpers::intersect_rect};
use jag_draw::{Rect, RoundedRect, Transform2D};

/// Whether `transform` turns axis-aligned rects into non-axis-aligned ones.
pub(crate) fn is_rotated(transform: Transform2D) -> bool {
    let [_, b, c, _, _, _] = transform.m;
    b != 0.0 || c != 0.0
}

/// Shader clip for `rect` with per-corner `radii` (tl, tr, br, bl), both in
/// the local space of `transform`, where one logical unit is `scale` device
/// pixels. `None` when the transform is singular.
pub(crate) fn shader_clip(
    rect: Rect,
    radii: [f32; 4],
    transform: Transform2D,
    scale: f32,
) -> Option<RoundedRectClip> {
    let [a, b, c, d, e, f] = transform.m;
    if !is_rotated(transform) {
        // Translation and scale only: the clip is a device-space rect.
        let sx = a.abs() * scale;
        let sy = d.abs() * scale;
        let radius_scale = sx.min(sy);
        return Some(RoundedRectClip {
            rect: Rect {
                x: (rect.x * a + e).min((rect.x + rect.w) * a + e) * scale,
                y: (rect.y * d + f).min((rect.y + rect.h) * d + f) * scale,
                w: rect.w * sx,
                h: rect.h * sy,
            },
            radii: radii.map(|r| r * radius_scale),
            from_device: jag_draw::RoundedRectClipGpu::DEVICE_ALIGNED,
        });
    }
    // Clip space is the local space scaled to device pixels along each local
    // axis, so radii and edge antialiasing stay in pixels.
    let sx = (a * a + b * b).sqrt() * scale;
    let sy = (c * c + d * d).sqrt() * scale;
    let det = a * d - b * c;
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    // Inverse of the local-to-logical transform.
    let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
    let (ie, i_f) = ((c * f - d * e) / det, (b * e - a * f) / det);
    let radius_scale = sx.min(sy);
    Some(RoundedRectClip {
        rect: Rect {
            x: rect.x * sx,
            y: rect.y * sy,
            w: rect.w * sx,
            h: rect.h * sy,
        },
        radii: radii.map(|r| r * radius_scale),
        // device px -> logical (divide by scale) -> local -> clip space.
        from_device: [
            ia * sx / scale,
            ib * sy / scale,
            ic * sx / scale,
            id * sy / scale,
            ie * sx,
            i_f * sy,
        ],
    })
}

impl Canvas {
    // Expose some painter helpers for advanced users
    pub fn push_clip_rect(&mut self, rect: Rect) {
        self.push_clip_rect_inner(rect);
        // An axis-aligned rect is fully described by the scissor. Under
        // rotation the scissor is only its bounding box, so the exact rect
        // goes to the shader as a zero-radius rounded clip. The shader takes
        // one clip, and an enclosing rounded clip keeps that slot.
        let transform = self.painter.current_transform();
        let enclosed = self.rounded_clip_stack.iter().any(Option::is_some);
        let rounded = (is_rotated(transform) && !enclosed)
            .then(|| shader_clip(rect, [0.0; 4], transform, self.dpi_scale))
            .flatten();
        self.rounded_clip_stack.push(rounded);
        if rounded.is_some() {
            self.painter
                .extend_commands(&[jag_draw::Command::ScrollClipRadii([0.0; 4])]);
        }
    }

    /// Push a rounded-rect clip.  The AABB is used for scissor-based coarse
    /// clipping; the full rounded rect (with per-corner radii) is forwarded
    /// to image draws for SDF-based fragment discard in the shader.
    pub fn push_clip_rounded_rect(&mut self, rrect: RoundedRect) {
        self.push_clip_rect_inner(rrect.rect);
        let radii = [
            rrect.radii.tl,
            rrect.radii.tr,
            rrect.radii.br,
            rrect.radii.bl,
        ];
        let clip = shader_clip(
            rrect.rect,
            radii,
            self.painter.current_transform(),
            self.dpi_scale,
        );
        let dev_radii = clip.map_or([0.0; 4], |clip| clip.radii);
        self.rounded_clip_stack.push(clip);
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

#[cfg(test)]
#[path = "clips_tests.rs"]
mod tests;
