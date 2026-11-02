use super::*;
pub(super) fn static_hit_signature(list: &DisplayList) -> Vec<Command> {
    let mut transforms = vec![Transform2D::identity()];
    let mut signature = Vec::new();
    for command in &list.commands {
        match command {
            // Keep the scene's location among clip/overlay commands in the
            // signature; only its live compositor properties may change.
            Command::DrawScrollScene { scene, .. } => signature.push(Command::DrawScrollScene {
                scene: scene.clone(),
                base: Transform2D::identity(),
                inputs: Arc::default(),
            }),
            Command::PushTransform(t) => transforms.push(*t),
            Command::PopTransform => {
                transforms.pop();
            }
            Command::PushClip(_) => {
                signature.push(Command::PushTransform(
                    *transforms.last().unwrap_or(&Transform2D::identity()),
                ));
                signature.push(command.clone());
                signature.push(Command::PopTransform);
            }
            _ => signature.push(command.clone()),
        }
    }
    signature
}

pub(super) fn bbox_for_path(path: &Path) -> Option<Rect> {
    let mut minx = f32::INFINITY;
    let mut miny = f32::INFINITY;
    let mut maxx = f32::NEG_INFINITY;
    let mut maxy = f32::NEG_INFINITY;
    let mut any = false;
    for cmd in &path.cmds {
        match *cmd {
            PathCmd::MoveTo(p) | PathCmd::LineTo(p) => {
                minx = minx.min(p[0]);
                miny = miny.min(p[1]);
                maxx = maxx.max(p[0]);
                maxy = maxy.max(p[1]);
                any = true;
            }
            PathCmd::QuadTo(c, p) => {
                for q in [c, p] {
                    minx = minx.min(q[0]);
                    miny = miny.min(q[1]);
                    maxx = maxx.max(q[0]);
                    maxy = maxy.max(q[1]);
                }
                any = true;
            }
            PathCmd::CubicTo(c1, c2, p) => {
                for q in [c1, c2, p] {
                    minx = minx.min(q[0]);
                    miny = miny.min(q[1]);
                    maxx = maxx.max(q[0]);
                    maxy = maxy.max(q[1]);
                }
                any = true;
            }
            PathCmd::Close => {}
        }
    }
    if any {
        Some(Rect {
            x: minx,
            y: miny,
            w: (maxx - minx).max(0.0),
            h: (maxy - miny).max(0.0),
        })
    } else {
        None
    }
}

pub(super) fn passes_clip(item: &HitItem, world: [f32; 2]) -> bool {
    for c in &item.clips {
        if !point_in_rect_local(world, &c.transform, c.rect) {
            return false;
        }
    }
    true
}

pub(super) fn hit_item_contains(item: &HitItem, world: [f32; 2]) -> bool {
    match &item.data {
        HitData::Rect(r) => point_in_rect_local(world, &item.transform, *r),
        HitData::RoundedRect(r) => point_in_rounded_rect_local(world, &item.transform, *r),
        HitData::Ellipse { center, radii } => {
            point_in_ellipse_local(world, &item.transform, *center, *radii)
        }
        HitData::StrokeRect { rect, width } => {
            point_in_stroke_rect_local(world, &item.transform, *rect, *width)
        }
        HitData::StrokeRoundedRect { rrect, width } => {
            point_in_stroke_rounded_rect_local(world, &item.transform, *rrect, *width)
        }
        HitData::PathBBox(rect) => point_in_rect_local(world, &item.transform, *rect),
        HitData::Text(_run) => false, // Text hit testing not yet implemented
        HitData::BoxShadow { rrect } => point_in_rounded_rect_local(world, &item.transform, *rrect),
        HitData::Hyperlink { rect, .. } => point_in_rect_local(world, &item.transform, *rect),
    }
}

pub(super) fn point_in_rect_local(world: [f32; 2], transform: &Transform2D, rect: Rect) -> bool {
    if let Some(p) = transform.inverse_apply(world) {
        p[0] >= rect.x && p[1] >= rect.y && p[0] <= rect.x + rect.w && p[1] <= rect.y + rect.h
    } else {
        false
    }
}

pub(super) fn point_in_rounded_rect_local(
    world: [f32; 2],
    transform: &Transform2D,
    rrect: RoundedRect,
) -> bool {
    if let Some(p) = transform.inverse_apply(world) {
        let Rect { x, y, w, h } = rrect.rect;
        let tl = rrect.radii.tl.min(w * 0.5).min(h * 0.5);
        let tr = rrect.radii.tr.min(w * 0.5).min(h * 0.5);
        let br = rrect.radii.br.min(w * 0.5).min(h * 0.5);
        let bl = rrect.radii.bl.min(w * 0.5).min(h * 0.5);
        let px = p[0] - x;
        let py = p[1] - y;
        if px < 0.0 || py < 0.0 || px > w || py > h {
            return false;
        }
        // Top-left
        if px < tl && py < tl {
            let dx = tl - px;
            let dy = tl - py;
            return dx * dx + dy * dy <= tl * tl + 1e-5;
        }
        // Top-right
        if px > w - tr && py < tr {
            let dx = px - (w - tr);
            let dy = tr - py;
            return dx * dx + dy * dy <= tr * tr + 1e-5;
        }
        // Bottom-right
        if px > w - br && py > h - br {
            let dx = px - (w - br);
            let dy = py - (h - br);
            return dx * dx + dy * dy <= br * br + 1e-5;
        }
        // Bottom-left
        if px < bl && py > h - bl {
            let dx = bl - px;
            let dy = py - (h - bl);
            return dx * dx + dy * dy <= bl * bl + 1e-5;
        }
        true
    } else {
        false
    }
}

pub(super) fn point_in_ellipse_local(
    world: [f32; 2],
    transform: &Transform2D,
    center: [f32; 2],
    radii: [f32; 2],
) -> bool {
    if let Some(p) = transform.inverse_apply(world) {
        let dx = (p[0] - center[0]) / radii[0].max(1e-6);
        let dy = (p[1] - center[1]) / radii[1].max(1e-6);
        dx * dx + dy * dy <= 1.0 + 1e-5
    } else {
        false
    }
}

pub(super) fn point_in_stroke_rect_local(
    world: [f32; 2],
    transform: &Transform2D,
    rect: Rect,
    width: f32,
) -> bool {
    if let Some(p) = transform.inverse_apply(world) {
        let outer = Rect {
            x: rect.x - width * 0.5,
            y: rect.y - width * 0.5,
            w: rect.w + width,
            h: rect.h + width,
        };
        let inner = Rect {
            x: rect.x + width * 0.5,
            y: rect.y + width * 0.5,
            w: (rect.w - width).max(0.0),
            h: (rect.h - width).max(0.0),
        };
        let in_outer = p[0] >= outer.x
            && p[1] >= outer.y
            && p[0] <= outer.x + outer.w
            && p[1] <= outer.y + outer.h;
        let in_inner = p[0] >= inner.x
            && p[1] >= inner.y
            && p[0] <= inner.x + inner.w
            && p[1] <= inner.y + inner.h;
        in_outer && !in_inner
    } else {
        false
    }
}

pub(super) fn point_in_stroke_rounded_rect_local(
    world: [f32; 2],
    transform: &Transform2D,
    rrect: RoundedRect,
    width: f32,
) -> bool {
    // Approximate by testing ring between rrect and inset rrect.
    if let Some(p) = transform.inverse_apply(world) {
        // Outer check
        let outer_hit = point_in_rounded_rect_untransformed(p, rrect);
        if !outer_hit {
            return false;
        }
        // Inner check (inset by stroke width)
        let inset = width.max(0.0) * 0.5;
        let inner = RoundedRect {
            rect: Rect {
                x: rrect.rect.x + inset,
                y: rrect.rect.y + inset,
                w: (rrect.rect.w - width).max(0.0),
                h: (rrect.rect.h - width).max(0.0),
            },
            radii: RoundedRadii {
                tl: (rrect.radii.tl - inset).max(0.0),
                tr: (rrect.radii.tr - inset).max(0.0),
                br: (rrect.radii.br - inset).max(0.0),
                bl: (rrect.radii.bl - inset).max(0.0),
            },
        };
        let inner_hit = point_in_rounded_rect_untransformed(p, inner);
        return outer_hit && !inner_hit;
    }
    false
}

pub(super) fn point_in_rounded_rect_untransformed(p: [f32; 2], rrect: RoundedRect) -> bool {
    let Rect { x, y, w, h } = rrect.rect;
    let tl = rrect.radii.tl.min(w * 0.5).min(h * 0.5);
    let tr = rrect.radii.tr.min(w * 0.5).min(h * 0.5);
    let br = rrect.radii.br.min(w * 0.5).min(h * 0.5);
    let bl = rrect.radii.bl.min(w * 0.5).min(h * 0.5);
    let px = p[0] - x;
    let py = p[1] - y;
    if px < 0.0 || py < 0.0 || px > w || py > h {
        return false;
    }
    if px < tl && py < tl {
        let dx = tl - px;
        let dy = tl - py;
        return dx * dx + dy * dy <= tl * tl + 1e-5;
    }
    if px > w - tr && py < tr {
        let dx = px - (w - tr);
        let dy = tr - py;
        return dx * dx + dy * dy <= tr * tr + 1e-5;
    }
    if px > w - br && py > h - br {
        let dx = px - (w - br);
        let dy = py - (h - br);
        return dx * dx + dy * dy <= br * br + 1e-5;
    }
    if px < bl && py > h - bl {
        let dx = bl - px;
        let dy = py - (h - bl);
        return dx * dx + dy * dy <= bl * bl + 1e-5;
    }
    true
}

// --- Transform helpers ---
impl Transform2D {
    /// Apply the transform to a point (x, y).
    pub fn apply(&self, p: [f32; 2]) -> [f32; 2] {
        let a = self.m[0];
        let b = self.m[1];
        let c = self.m[2];
        let d = self.m[3];
        let e = self.m[4];
        let f = self.m[5];
        [a * p[0] + c * p[1] + e, b * p[0] + d * p[1] + f]
    }

    /// Apply the inverse transform to a world-space point. Returns None if non-invertible.
    pub fn inverse_apply(&self, p: [f32; 2]) -> Option<[f32; 2]> {
        let a = self.m[0];
        let b = self.m[1];
        let c = self.m[2];
        let d = self.m[3];
        let e = self.m[4];
        let f = self.m[5];
        let det = a * d - b * c;
        if det.abs() < 1e-12 {
            return None;
        }
        let inv_det = 1.0 / det;
        let ia = d * inv_det;
        let ib = -b * inv_det;
        let ic = -c * inv_det;
        let id = a * inv_det;
        // Inverse translation = -inv_linear * [e, f]
        let ie = -(ia * e + ic * f);
        let iff = -(ib * e + id * f);
        Some([ia * p[0] + ic * p[1] + ie, ib * p[0] + id * p[1] + iff])
    }
}

/// Convenience: perform a one-shot hit test on the display list without keeping an index.
pub fn hit_test(list: &DisplayList, pos: [f32; 2]) -> Option<HitResult> {
    HitIndex::build(list).topmost_at(pos)
}

pub(super) fn compute_locals(
    item: &HitItem,
    world: [f32; 2],
) -> (Option<[f32; 2]>, Option<[f32; 2]>) {
    let p = match item.transform.inverse_apply(world) {
        Some(p) => p,
        None => return (None, None),
    };
    match &item.data {
        HitData::Rect(r) => {
            let local = [p[0] - r.x, p[1] - r.y];
            let uv = [
                if r.w.abs() > 1e-6 {
                    (local[0] / r.w).clamp(0.0, 1.0)
                } else {
                    0.0
                },
                if r.h.abs() > 1e-6 {
                    (local[1] / r.h).clamp(0.0, 1.0)
                } else {
                    0.0
                },
            ];
            (Some(local), Some(uv))
        }
        HitData::RoundedRect(rr) => {
            let r = rr.rect;
            let local = [p[0] - r.x, p[1] - r.y];
            let uv = [
                if r.w.abs() > 1e-6 {
                    (local[0] / r.w).clamp(0.0, 1.0)
                } else {
                    0.0
                },
                if r.h.abs() > 1e-6 {
                    (local[1] / r.h).clamp(0.0, 1.0)
                } else {
                    0.0
                },
            ];
            (Some(local), Some(uv))
        }
        HitData::Ellipse { center, radii } => {
            let local = [p[0] - center[0], p[1] - center[1]];
            let uv = [
                0.5 + if radii[0].abs() > 1e-6 {
                    local[0] / (2.0 * radii[0])
                } else {
                    0.0
                },
                0.5 + if radii[1].abs() > 1e-6 {
                    local[1] / (2.0 * radii[1])
                } else {
                    0.0
                },
            ];
            (Some(local), Some(uv))
        }
        HitData::StrokeRect { rect, .. } => {
            let local = [p[0] - rect.x, p[1] - rect.y];
            (Some(local), None)
        }
        HitData::StrokeRoundedRect { rrect, .. } => {
            let r = rrect.rect;
            let local = [p[0] - r.x, p[1] - r.y];
            (Some(local), None)
        }
        HitData::Text(_) => (None, None),
        HitData::PathBBox(r) => {
            let local = [p[0] - r.x, p[1] - r.y];
            let uv = [
                if r.w.abs() > 1e-6 {
                    (local[0] / r.w).clamp(0.0, 1.0)
                } else {
                    0.0
                },
                if r.h.abs() > 1e-6 {
                    (local[1] / r.h).clamp(0.0, 1.0)
                } else {
                    0.0
                },
            ];
            (Some(local), Some(uv))
        }
        HitData::BoxShadow { rrect } => {
            let r = rrect.rect;
            let local = [p[0] - r.x, p[1] - r.y];
            let uv = [
                if r.w.abs() > 1e-6 {
                    (local[0] / r.w).clamp(0.0, 1.0)
                } else {
                    0.0
                },
                if r.h.abs() > 1e-6 {
                    (local[1] / r.h).clamp(0.0, 1.0)
                } else {
                    0.0
                },
            ];
            (Some(local), Some(uv))
        }
        HitData::Hyperlink { rect, .. } => {
            let local = [p[0] - rect.x, p[1] - rect.y];
            let uv = [
                if rect.w.abs() > 1e-6 {
                    (local[0] / rect.w).clamp(0.0, 1.0)
                } else {
                    0.0
                },
                if rect.h.abs() > 1e-6 {
                    (local[1] / rect.h).clamp(0.0, 1.0)
                } else {
                    0.0
                },
            ];
            (Some(local), Some(uv))
        }
    }
}
