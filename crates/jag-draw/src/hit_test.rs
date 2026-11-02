use crate::display_list::{Command, DisplayList};
use crate::scene::*;
use std::sync::{Arc, OnceLock};

/// Result of a hit test for a single topmost element.
#[derive(Clone, Debug)]
pub struct HitResult {
    /// Increasing draw-order identifier within the display list.
    pub id: usize,
    /// Z layering value; larger is visually on top.
    pub z: i32,
    /// Kind of primitive that was hit.
    pub kind: HitKind,
    /// Geometry of the hit shape.
    pub shape: HitShape,
    /// The local-to-world transform at draw time.
    pub transform: Transform2D,
    /// If this hit corresponds to a hit region, returns the user-specified region id.
    pub region_id: Option<u32>,
    /// Local point within the hit item's local space (after inverse transform), relative to the shape's origin.
    pub local_pos: Option<[f32; 2]>,
    /// Normalized coordinates within the shape's bounding box when applicable ([0,1] range).
    pub local_uv: Option<[f32; 2]>,
}

#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum HitKind {
    Rect,
    RoundedRect,
    Ellipse,
    Text,
    StrokeRect,
    StrokeRoundedRect,
    Path,
    BoxShadow,
    HitRegion,
    Hyperlink,
}

/// Public geometry snapshot of a hit element.
#[derive(Clone, Debug, PartialEq)]
pub enum HitShape {
    Rect(Rect),
    RoundedRect(RoundedRect),
    Ellipse { center: [f32; 2], radii: [f32; 2] },
    StrokeRect { rect: Rect, width: f32 },
    StrokeRoundedRect { rrect: RoundedRect, width: f32 },
    PathBBox { rect: Rect },
    Text,
    BoxShadow { rrect: RoundedRect },
    Hyperlink { rect: Rect, url: String },
}

/// Preprocessed hit-test item built from a display list command.
#[derive(Clone, Debug)]
struct HitItem {
    id: usize,
    z: i32,
    kind: HitKind,
    transform: Transform2D,
    data: HitData,
    clips: Vec<ClipEntry>,
    region_id: Option<u32>,
}

#[derive(Clone, Debug)]
enum HitData {
    Rect(Rect),
    RoundedRect(RoundedRect),
    Ellipse { center: [f32; 2], radii: [f32; 2] },
    StrokeRect { rect: Rect, width: f32 },
    StrokeRoundedRect { rrect: RoundedRect, width: f32 },
    PathBBox(Rect),
    Text(TextRun),
    BoxShadow { rrect: RoundedRect },
    Hyperlink { rect: Rect, url: String },
}

#[derive(Clone, Debug)]
struct ClipEntry {
    rect: Rect,
    transform: Transform2D,
}

/// Spatial index for hit testing. Currently implemented as a flat list while
/// preserving z-order and clip stacks. Can be upgraded to a tree later.
#[derive(Default, Clone)]
pub struct HitIndex {
    items: Vec<HitItem>,
    retained: Vec<RetainedHits>,
    static_signature: Vec<Command>,
    viewport: (u32, u32),
}

#[derive(Clone)]
struct RetainedHits {
    scene: Arc<crate::ScrollScene>,
    base: Transform2D,
    inputs: Arc<crate::ScrollInputs>,
    clips: Vec<ClipEntry>,
    id_offset: usize,
    resolved: Arc<OnceLock<HitIndex>>,
}

impl HitIndex {
    /// Update committed-scene properties without rebuilding static geometry.
    /// The signature includes transformed clips and each drawable's complete
    /// transform, so chrome, resize and overlay changes still force a rebuild.
    pub fn update_retained_scenes(&mut self, list: &DisplayList) -> bool {
        if self.retained.is_empty()
            || self.viewport != (list.viewport.width, list.viewport.height)
            || self.static_signature != static_hit_signature(list)
        {
            return false;
        }
        let scenes: Vec<_> = list
            .commands
            .iter()
            .filter_map(|c| match c {
                Command::DrawScrollScene {
                    scene,
                    base,
                    inputs,
                } => Some((scene, base, inputs)),
                _ => None,
            })
            .collect();
        if scenes.len() != self.retained.len()
            || scenes
                .iter()
                .zip(&self.retained)
                .any(|((scene, _, _), old)| scene.id != old.scene.id)
        {
            return false;
        }
        for ((_, base, inputs), old) in scenes.into_iter().zip(&mut self.retained) {
            if old.base != *base || old.inputs != *inputs {
                old.base = *base;
                old.inputs = inputs.clone();
                old.resolved = Arc::new(OnceLock::new());
            }
        }
        true
    }

    /// Return the topmost element at the given device-space position.
    pub fn topmost_at(&self, pos: [f32; 2]) -> Option<HitResult> {
        let mut best: Option<HitItem> = None;
        for it in &self.items {
            if !passes_clip(it, pos) {
                continue;
            }
            if hit_item_contains(it, pos) {
                best = match best {
                    None => Some(it.clone()),
                    Some(ref cur) => {
                        if it.z > cur.z || (it.z == cur.z && it.id > cur.id) {
                            Some(it.clone())
                        } else {
                            Some(cur.clone())
                        }
                    }
                };
            }
        }
        let mut result = best.map(|it| {
            let (local_pos, local_uv) = compute_locals(&it, pos);
            HitResult {
                id: it.id,
                z: it.z,
                kind: it.kind,
                shape: match &it.data {
                    HitData::Rect(r) => HitShape::Rect(*r),
                    HitData::RoundedRect(rr) => HitShape::RoundedRect(*rr),
                    HitData::Ellipse { center, radii } => HitShape::Ellipse {
                        center: *center,
                        radii: *radii,
                    },
                    HitData::StrokeRect { rect, width } => HitShape::StrokeRect {
                        rect: *rect,
                        width: *width,
                    },
                    HitData::StrokeRoundedRect { rrect, width } => HitShape::StrokeRoundedRect {
                        rrect: *rrect,
                        width: *width,
                    },
                    HitData::PathBBox(r) => HitShape::PathBBox { rect: *r },
                    HitData::Text(_) => HitShape::Text,
                    HitData::BoxShadow { rrect } => HitShape::BoxShadow { rrect: *rrect },
                    HitData::Hyperlink { rect, url } => HitShape::Hyperlink {
                        rect: *rect,
                        url: url.clone(),
                    },
                },
                transform: it.transform,
                region_id: it.region_id,
                local_pos,
                local_uv,
            }
        });
        for retained in &self.retained {
            if !retained
                .clips
                .iter()
                .all(|clip| point_in_rect_local(pos, &clip.transform, clip.rect))
            {
                continue;
            }
            let index = retained.resolved.get_or_init(|| {
                HitIndex::build(&DisplayList {
                    viewport: crate::Viewport {
                        width: self.viewport.0,
                        height: self.viewport.1,
                    },
                    commands: retained.scene.resolve(retained.base, &retained.inputs),
                })
            });
            if let Some(mut hit) = index.topmost_at(pos) {
                if hit.region_id == Some(u32::MAX) {
                    continue;
                }
                hit.id += retained.id_offset;
                if result
                    .as_ref()
                    .is_none_or(|old| hit.z > old.z || (hit.z == old.z && hit.id > old.id))
                {
                    result = Some(hit);
                }
            }
        }
        result
    }
}

#[path = "hit_test_build.rs"]
mod build;
#[path = "hit_test_geometry.rs"]
mod geometry;
pub use geometry::hit_test;
use geometry::*;
#[cfg(test)]
#[path = "hit_test_tests.rs"]
mod tests;
