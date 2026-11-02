use super::*;
impl HitIndex {
    /// Build a hit-test index from the display list. Records each drawable
    /// element with its transform and the active clip stack at that point.
    pub fn build(list: &DisplayList) -> Self {
        let mut items = Vec::new();
        let mut clips: Vec<ClipEntry> = Vec::new();
        let mut tstack: Vec<Transform2D> = vec![Transform2D::identity()];
        let mut next_id: usize = 0;
        let mut retained = Vec::new();

        for cmd in &list.commands {
            match cmd {
                Command::DrawScrollScene {
                    scene,
                    base,
                    inputs,
                } => {
                    retained.push(RetainedHits {
                        scene: scene.clone(),
                        base: *base,
                        inputs: inputs.clone(),
                        clips: clips.clone(),
                        id_offset: next_id,
                        resolved: Arc::new(OnceLock::new()),
                    });
                    next_id += scene.commands.len();
                }
                Command::PushScrollLayer { .. }
                | Command::PopScrollLayer
                | Command::ScrollClipRadii(_)
                | Command::DrawCompositeTile { .. } => {}
                Command::PushClip(ClipRect(rect)) => {
                    clips.push(ClipEntry {
                        rect: *rect,
                        transform: *tstack.last().unwrap(),
                    });
                }
                Command::PopClip => {
                    let _ = clips.pop();
                }
                Command::PushTransform(t) => {
                    tstack.push(*t);
                }
                Command::PopTransform => {
                    let _ = tstack.pop();
                }
                Command::DrawRect {
                    rect, z, transform, ..
                } => {
                    items.push(HitItem {
                        id: next_id,
                        z: *z,
                        kind: HitKind::Rect,
                        transform: *transform,
                        data: HitData::Rect(*rect),
                        clips: clips.clone(),
                        region_id: None,
                    });
                    next_id += 1;
                }
                Command::DrawRoundedRect {
                    rrect,
                    z,
                    transform,
                    ..
                } => {
                    items.push(HitItem {
                        id: next_id,
                        z: *z,
                        kind: HitKind::RoundedRect,
                        transform: *transform,
                        data: HitData::RoundedRect(*rrect),
                        clips: clips.clone(),
                        region_id: None,
                    });
                    next_id += 1;
                }
                Command::DrawEllipse {
                    center,
                    radii,
                    z,
                    transform,
                    ..
                } => {
                    items.push(HitItem {
                        id: next_id,
                        z: *z,
                        kind: HitKind::Ellipse,
                        transform: *transform,
                        data: HitData::Ellipse {
                            center: *center,
                            radii: *radii,
                        },
                        clips: clips.clone(),
                        region_id: None,
                    });
                    next_id += 1;
                }
                Command::StrokeRect {
                    rect,
                    stroke,
                    z,
                    transform,
                    ..
                } => {
                    items.push(HitItem {
                        id: next_id,
                        z: *z,
                        kind: HitKind::StrokeRect,
                        transform: *transform,
                        data: HitData::StrokeRect {
                            rect: *rect,
                            width: stroke.width,
                        },
                        clips: clips.clone(),
                        region_id: None,
                    });
                    next_id += 1;
                }
                Command::StrokeRoundedRect {
                    rrect,
                    stroke,
                    z,
                    transform,
                    ..
                } => {
                    items.push(HitItem {
                        id: next_id,
                        z: *z,
                        kind: HitKind::StrokeRoundedRect,
                        transform: *transform,
                        data: HitData::StrokeRoundedRect {
                            rrect: *rrect,
                            width: stroke.width,
                        },
                        clips: clips.clone(),
                        region_id: None,
                    });
                    next_id += 1;
                }
                Command::DrawText {
                    run, z, transform, ..
                } => {
                    items.push(HitItem {
                        id: next_id,
                        z: *z,
                        kind: HitKind::Text,
                        transform: *transform,
                        data: HitData::Text(run.clone()),
                        clips: clips.clone(),
                        region_id: None,
                    });
                    next_id += 1;
                }
                Command::DrawHyperlink {
                    hyperlink,
                    z,
                    transform,
                    ..
                } => {
                    // Keep hyperlinks above generic per-node hit regions (commonly z + 10)
                    // so inline links remain hoverable/clickable inside text containers.
                    let hit_z = z.saturating_add(11);
                    let text_width = hyperlink.measured_width.unwrap_or_else(|| {
                        // Fallback heuristic (slightly generous). Include a small
                        // boost for heavier font weights.
                        let weight_boost = ((hyperlink.weight - 400.0).max(0.0) / 500.0) * 0.08;
                        let char_width = hyperlink.size * (0.55 + weight_boost);
                        hyperlink.text.chars().count() as f32 * char_width
                    });
                    let text_height = hyperlink.size * 1.2; // Include descenders

                    let rect = Rect {
                        x: hyperlink.pos[0],
                        y: hyperlink.pos[1] - hyperlink.size, // Baseline to top
                        w: text_width,
                        h: text_height,
                    };

                    items.push(HitItem {
                        id: next_id,
                        z: hit_z,
                        kind: HitKind::Hyperlink,
                        transform: *transform,
                        data: HitData::Hyperlink {
                            rect,
                            url: hyperlink.url.clone(),
                        },
                        clips: clips.clone(),
                        region_id: None,
                    });
                    next_id += 1;
                }
                Command::FillPath { .. } => {
                    // Coarse bbox hit: compute path bounding box (approximate using control/end points).
                    if let Command::FillPath {
                        path, z, transform, ..
                    } = cmd
                    {
                        if let Some(rect) = bbox_for_path(path) {
                            items.push(HitItem {
                                id: next_id,
                                z: *z,
                                kind: HitKind::Path,
                                transform: *transform,
                                data: HitData::PathBBox(rect),
                                clips: clips.clone(),
                                region_id: None,
                            });
                            next_id += 1;
                        }
                    }
                }
                Command::StrokePath {
                    path, z, transform, ..
                } => {
                    if let Some(mut rect) = bbox_for_path(path) {
                        // Expand bbox by half of stroke width conservatively
                        if let Command::StrokePath { stroke, .. } = cmd {
                            let w = stroke.width.max(0.0) * 0.5;
                            rect.x -= w;
                            rect.y -= w;
                            rect.w += w * 2.0;
                            rect.h += w * 2.0;
                        }
                        items.push(HitItem {
                            id: next_id,
                            z: *z,
                            kind: HitKind::Path,
                            transform: *transform,
                            data: HitData::PathBBox(rect),
                            clips: clips.clone(),
                            region_id: None,
                        });
                        next_id += 1;
                    }
                }
                Command::BoxShadow {
                    rrect,
                    z,
                    transform,
                    ..
                } => {
                    items.push(HitItem {
                        id: next_id,
                        z: *z,
                        kind: HitKind::BoxShadow,
                        transform: *transform,
                        data: HitData::BoxShadow { rrect: *rrect },
                        clips: clips.clone(),
                        region_id: None,
                    });
                    next_id += 1;
                }
                Command::HitRegionRect {
                    id,
                    rect,
                    z,
                    transform,
                } => {
                    items.push(HitItem {
                        id: next_id,
                        z: *z,
                        kind: HitKind::HitRegion,
                        transform: *transform,
                        data: HitData::Rect(*rect),
                        clips: clips.clone(),
                        region_id: Some(*id),
                    });
                    next_id += 1;
                }
                Command::HitRegionRoundedRect {
                    id,
                    rrect,
                    z,
                    transform,
                } => {
                    items.push(HitItem {
                        id: next_id,
                        z: *z,
                        kind: HitKind::HitRegion,
                        transform: *transform,
                        data: HitData::RoundedRect(*rrect),
                        clips: clips.clone(),
                        region_id: Some(*id),
                    });
                    next_id += 1;
                }
                Command::HitRegionEllipse {
                    id,
                    center,
                    radii,
                    z,
                    transform,
                } => {
                    items.push(HitItem {
                        id: next_id,
                        z: *z,
                        kind: HitKind::HitRegion,
                        transform: *transform,
                        data: HitData::Ellipse {
                            center: *center,
                            radii: *radii,
                        },
                        clips: clips.clone(),
                        region_id: Some(*id),
                    });
                    next_id += 1;
                }
                // Image, SVG, and external texture draws currently do not participate in hit testing.
                Command::DrawImage { .. } => {}
                Command::DrawSvg { .. } => {}
                Command::DrawExternalTexture { .. }
                | Command::DrawProjectedTexture { .. }
                | Command::DrawSpatialLayer(_) => {}
                Command::BackdropFilter(_) => {}
                Command::PushOpacity(_) => {}
                Command::PopOpacity | Command::PushFilter(_) | Command::PopFilter => {}
            }
        }

        // Always include a root viewport hit region to capture scene-surface hits
        // Use minimal z so it doesn't occlude content.
        let root_rect = Rect {
            x: 0.0,
            y: 0.0,
            w: list.viewport.width as f32,
            h: list.viewport.height as f32,
        };
        items.push(HitItem {
            id: next_id,
            z: i32::MIN,
            kind: HitKind::HitRegion,
            transform: Transform2D::identity(),
            data: HitData::Rect(root_rect),
            clips: Vec::new(),
            region_id: Some(u32::MAX),
        });

        let static_signature = if retained.is_empty() {
            Vec::new()
        } else {
            static_hit_signature(list)
        };
        Self {
            items,
            retained,
            static_signature,
            viewport: (list.viewport.width, list.viewport.height),
        }
    }
}
