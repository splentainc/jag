use super::*;
fn unbounded_geometry(bounds: Rect, scale: f32) -> Option<LayerGeometry> {
    if ![bounds.x, bounds.y, bounds.w, bounds.h, scale]
        .iter()
        .all(|v| v.is_finite())
        || scale <= 0.
        || bounds.w <= 0.
        || bounds.h <= 0.
    {
        return None;
    }
    let x = (bounds.x * scale).floor() / scale;
    let y = (bounds.y * scale).floor() / scale;
    let w = ((bounds.x + bounds.w - x) * scale).ceil() as u32;
    let h = ((bounds.y + bounds.h - y) * scale).ceil() as u32;
    Some(LayerGeometry {
        origin: [x, y],
        logical_size: [w as f32 / scale, h as f32 / scale],
        pixel_size: [w, h],
    })
}
impl JagSurface {
    pub(in crate::surface) fn flatten_effect_groups(
        &mut self,
        commands: &[Command],
        viewport: Viewport,
        provider: Option<&Arc<dyn jag_draw::TextProvider + Send + Sync>>,
    ) -> Result<Vec<Command>> {
        self.flatten_effect_groups_inner(commands, viewport, provider, false)
    }
    pub(in crate::surface) fn flatten_unbounded_effect_groups(
        &mut self,
        commands: &[Command],
        viewport: Viewport,
        provider: Option<&Arc<dyn jag_draw::TextProvider + Send + Sync>>,
    ) -> Result<Vec<Command>> {
        self.flatten_effect_groups_inner(commands, viewport, provider, true)
    }
    fn flatten_effect_groups_inner(
        &mut self,
        commands: &[Command],
        viewport: Viewport,
        text_provider: Option<&Arc<dyn jag_draw::TextProvider + Send + Sync>>,
        unbounded: bool,
    ) -> Result<Vec<Command>> {
        let plan = jag_draw::build_compositor_plan(&DisplayList {
            viewport,
            commands: commands.to_vec(),
        })?;
        let mut out: Vec<Command> = Vec::new();
        let mut i = 0usize;
        while i < commands.len() {
            match commands[i] {
                Command::PushOpacity(_) | Command::PushFilter(_) => {
                    let surface = plan
                        .surfaces
                        .iter()
                        .find(|surface| surface.parent.is_none() && surface.commands.start == i + 1)
                        .expect("validated compositor plan must own each root opacity scope");
                    let mut raw_group = commands[surface.commands.clone()].to_vec();
                    // Draws store complete transforms, but clip commands store
                    // local rects. Preserve the transform active at scope entry
                    // when replaying an isolated group (including nested ones).
                    raw_group.insert(0, Command::PushTransform(surface.inherited_transform));
                    raw_group.push(Command::PopTransform);
                    if let Some(clip) = surface.inherited_clip {
                        raw_group.insert(0, Command::PushClip(jag_draw::ClipRect(clip)));
                        raw_group.push(Command::PopClip);
                    }
                    let flattened_group = self.flatten_effect_groups_inner(
                        &raw_group,
                        viewport,
                        text_provider,
                        unbounded,
                    )?;

                    // Preserve hit-only regions outside the composited layer.
                    for cmd in flattened_group.iter() {
                        match cmd {
                            Command::HitRegionRect { .. }
                            | Command::HitRegionRoundedRect { .. }
                            | Command::HitRegionEllipse { .. } => out.push(cmd.clone()),
                            _ => {}
                        }
                    }

                    let layer_opacity = match &surface.effect {
                        jag_draw::SurfaceEffect::Opacity(opacity) => *opacity,
                        jag_draw::SurfaceEffect::Blur(_) => 1.0,
                        jag_draw::SurfaceEffect::ColorMatrix(_) => 1.0,
                        jag_draw::SurfaceEffect::DropShadow(_) => 1.0,
                        jag_draw::SurfaceEffect::Mask(_) => 1.0,
                        jag_draw::SurfaceEffect::MaskGroup(_) => 1.0,
                    };
                    if layer_opacity > 0.0
                        && let Some(z) = Self::effect_group_z(&flattened_group)
                        && let Some(bounds) = surface.bounds
                    {
                        // DrawExternalTexture coordinates are interpreted in logical units
                        // by PassManager when logical pixel mode is enabled.
                        let logical_scale = jag_draw::logical_multiplier(
                            self.logical_pixels,
                            self.dpi_scale,
                            self.ui_scale,
                        );
                        if let Some(geometry) = if unbounded {
                            unbounded_geometry(bounds, logical_scale)
                        } else {
                            layer_geometry(bounds, viewport, logical_scale)
                        } {
                            let tex_id = self.render_effect_group_layer(
                                geometry,
                                flattened_group,
                                surface.effect.clone(),
                                text_provider,
                                logical_scale,
                                None,
                            )?;
                            out.push(Command::DrawExternalTexture {
                                rect: Rect {
                                    x: geometry.origin[0],
                                    y: geometry.origin[1],
                                    w: geometry.logical_size[0],
                                    h: geometry.logical_size[1],
                                },
                                texture_id: tex_id,
                                z,
                                transform: Transform2D::identity(),
                                opacity: layer_opacity,
                                premultiplied: true,
                                rounded_clip: None,
                                clip: None,
                            });
                        }
                    }
                    i = surface.commands.end + 1;
                }
                Command::PopOpacity | Command::PopFilter => {
                    // Ignore unmatched pops.
                    i += 1;
                }
                _ => {
                    out.push(commands[i].clone());
                    i += 1;
                }
            }
        }
        Ok(out)
    }
}
