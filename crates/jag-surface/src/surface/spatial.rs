//! Spatial CSS contexts resolve into textured planes before ordinary composition.
use super::{JagSurface, opacity::LayerGeometry};
use anyhow::Result;
use jag_draw::{
    Command, ExternalTextureId, Rect, SurfaceEffect, TextProvider, Transform2D, Viewport,
    spatial::{self, Matrix4, SpatialLayer},
};
use std::sync::Arc;
#[path = "spatial_visibility.rs"]
mod visibility;
fn contains_spatial(commands: &[Command]) -> bool {
    commands.iter().any(|c| match c {
        Command::DrawSpatialLayer(_) => true,
        Command::DrawScrollScene { scene, .. } => contains_spatial(&scene.commands),
        _ => false,
    })
}
fn preserves_plane(matrix: [f32; 16]) -> bool {
    // A local z=0 point must stay at z=0. Test authored coefficients exactly:
    // even a small genuine translateZ/rotation still needs spatial depth.
    matrix[2] == 0. && matrix[6] == 0. && matrix[14] == 0.
}
fn coplanar_contents(commands: &[Command]) -> bool {
    commands.iter().all(|command| match command {
        Command::DrawSpatialLayer(child) => {
            preserves_plane(child.matrix)
                // Backface visibility needs the accumulated spatial matrix.
                && !child.backface_hidden
                && (!child.preserve_3d
                    || (preserves_plane(child.perspective) && coplanar_contents(&child.commands)))
        }
        Command::DrawScrollScene { scene, .. } => coplanar_contents(&scene.commands),
        Command::DrawProjectedTexture { quad, .. } => quad.iter().all(|p| p[2] == 0.),
        _ => true,
    })
}
fn expand_retained(commands: &[Command]) -> Vec<Command> {
    commands
        .iter()
        .flat_map(|c| match c {
            Command::DrawScrollScene {
                scene,
                base,
                inputs,
            } if contains_spatial(&scene.commands) => {
                expand_retained(&scene.resolve(*base, inputs))
            }
            _ => vec![c.clone()],
        })
        .collect()
}
struct Plane {
    opacity: f32,
    id: ExternalTextureId,
    quad: [[f32; 4]; 4],
    bounds: Rect,
}
fn union(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Rect {
        x,
        y,
        w: (a.x + a.w).max(b.x + b.w) - x,
        h: (a.y + a.h).max(b.y + b.h) - y,
    }
}
impl JagSurface {
    fn spatial_raster(
        &mut self,
        commands: Vec<Command>,
        viewport: Viewport,
        provider: Option<&Arc<dyn TextProvider + Send + Sync>>,
        key: &str,
    ) -> Result<Option<(ExternalTextureId, Rect)>> {
        let commands = self.flatten_scroll_layers(&commands, viewport, provider)?;
        let commands = self.flatten_unbounded_effect_groups(&commands, viewport, provider)?;
        let Some(bounds) = commands
            .iter()
            .filter_map(jag_draw::command_bounds)
            .reduce(union)
        else {
            return Ok(None);
        };
        let scale =
            jag_draw::logical_multiplier(self.logical_pixels, self.dpi_scale, self.ui_scale);
        let x = (bounds.x * scale).floor() / scale;
        let y = (bounds.y * scale).floor() / scale;
        let w = ((bounds.x + bounds.w - x) * scale).ceil().max(1.) as u32;
        let h = ((bounds.y + bounds.h - y) * scale).ceil().max(1.) as u32;
        anyhow::ensure!(
            w <= self.device.limits().max_texture_dimension_2d
                && h <= self.device.limits().max_texture_dimension_2d,
            "spatial layer exceeds device texture limits: {w}x{h}"
        );
        let geometry = LayerGeometry {
            origin: [x, y],
            logical_size: [w as f32 / scale, h as f32 / scale],
            pixel_size: [w, h],
        };
        // Small animated details must not rerasterize every glyph, border and
        // background of an otherwise stable card. Cache its local ink in tiles
        // before projecting the complete plane. Its own bounds, not the window
        // clip, define visibility in these unprojected coordinates.
        let commands = if self.retained_layers.is_enabled()
            && commands
                .iter()
                .filter(|c| {
                    matches!(
                        c,
                        Command::DrawText { .. }
                            | Command::DrawRect { .. }
                            | Command::DrawRoundedRect { .. }
                            | Command::StrokeRoundedRect { .. }
                    )
                })
                .count()
                > 4
        {
            let mut tiled = Vec::with_capacity(commands.len() + 2);
            tiled.push(Command::PushScrollLayer {
                key: key.into(),
                origin: [0., 0.],
                binding: jag_draw::ScrollBinding {
                    key: key.into(),
                    value: [0., 0.],
                    factor: [0., 0.],
                    basis: [1., 0., 0., 1.],
                },
            });
            tiled.extend(commands);
            tiled.push(Command::PopScrollLayer);
            self.flatten_scroll_layers_in_rect(&tiled, bounds, scale, provider)?
        } else {
            commands
        };
        let id = self.render_effect_group_layer(
            geometry,
            commands,
            SurfaceEffect::Opacity(1.),
            provider,
            scale,
            None,
        )?;
        Ok(Some((
            id,
            Rect {
                x,
                y,
                w: geometry.logical_size[0],
                h: geometry.logical_size[1],
            },
        )))
    }
    fn spatial_planes(
        &mut self,
        layer: &SpatialLayer,
        parent: Matrix4,
        viewport: Viewport,
        provider: Option<&Arc<dyn TextProvider + Send + Sync>>,
        planes: &mut Vec<Plane>,
        apply_opacity: bool,
        visibility_transform: Option<Matrix4>,
        key: &str,
    ) -> Result<()> {
        let mut layer = layer.clone();
        layer.commands = expand_retained(&layer.commands);
        let matrix = spatial::multiply(parent, Matrix4::from_array(layer.matrix));
        if layer.backface_hidden && spatial::back_facing(matrix) {
            return Ok(());
        }
        // Coplanar contents have CSS paint order, not competing GPU depths.
        // Compose them before applying the shared projective transform. Separate
        // tilted quads round the same depth differently and flicker through one
        // another, particularly for a filtered aura with different bounds.
        if !layer.preserve_3d
            || (preserves_plane(layer.perspective) && coplanar_contents(&layer.commands))
        {
            let commands: Vec<_> = layer
                .commands
                .iter()
                .map(|command| {
                    if let Command::DrawSpatialLayer(child) = command {
                        let mut child = (**child).clone();
                        child.matrix = spatial::multiply(
                            Matrix4::from_array(layer.perspective),
                            Matrix4::from_array(child.matrix),
                        )
                        .to_array();
                        Command::DrawSpatialLayer(Arc::new(child))
                    } else {
                        command.clone()
                    }
                })
                .collect();
            // A parent filter can make an offscreen child contribute visible
            // pixels. Cull that group only after applying its filter bounds.
            let child_visibility = visibility_transform
                .filter(|_| !visibility::expands(&commands))
                .map(|outer| spatial::multiply(outer, matrix));
            let commands = self.flatten_spatial_layers_in(
                &commands,
                viewport,
                provider,
                child_visibility,
                key,
            )?;
            if !visibility::visible(
                &commands,
                visibility_transform.map(|outer| spatial::multiply(outer, matrix)),
                viewport,
                jag_draw::logical_multiplier(self.logical_pixels, self.dpi_scale, self.ui_scale),
            )? {
                return Ok(());
            }
            if let Some((id, bounds)) =
                self.spatial_raster(commands, viewport, provider, &format!("{key}/flat"))?
            {
                let quad = spatial::quad(matrix, bounds);
                if let Some(bounds) = spatial::bounds(&quad) {
                    planes.push(Plane {
                        id,
                        quad,
                        bounds,
                        opacity: if apply_opacity { layer.opacity } else { 1. },
                    });
                }
            }
            return Ok(());
        }
        // A preserve-3D element's own paints form a plane; child spatial owners
        // remain in this context until an explicit flat/grouping boundary.
        let own: Vec<_> = layer
            .commands
            .iter()
            .filter(|c| !matches!(c, Command::DrawSpatialLayer(_)))
            .cloned()
            .collect();
        if visibility::visible(
            &own,
            visibility_transform.map(|outer| spatial::multiply(outer, matrix)),
            viewport,
            jag_draw::logical_multiplier(self.logical_pixels, self.dpi_scale, self.ui_scale),
        )? && let Some((id, bounds)) =
            self.spatial_raster(own, viewport, provider, &format!("{key}/own"))?
        {
            let quad = spatial::quad(matrix, bounds);
            if let Some(bounds) = spatial::bounds(&quad) {
                planes.push(Plane {
                    id,
                    quad,
                    bounds,
                    opacity: if apply_opacity { layer.opacity } else { 1. },
                });
            }
        }
        let child_matrix = spatial::multiply(matrix, Matrix4::from_array(layer.perspective));
        for (index, command) in layer.commands.iter().enumerate() {
            if let Command::DrawSpatialLayer(child) = command {
                self.spatial_planes(
                    child,
                    child_matrix,
                    viewport,
                    provider,
                    planes,
                    true,
                    visibility_transform,
                    &format!("{key}/{index}"),
                )?;
            }
        }
        Ok(())
    }
    pub(super) fn flatten_spatial_layers(
        &mut self,
        commands: &[Command],
        viewport: Viewport,
        provider: Option<&Arc<dyn TextProvider + Send + Sync>>,
    ) -> Result<Vec<Command>> {
        self.flatten_spatial_layers_in(
            commands,
            viewport,
            provider,
            Some(Matrix4::identity()),
            "spatial",
        )
    }
    fn flatten_spatial_layers_in(
        &mut self,
        commands: &[Command],
        viewport: Viewport,
        provider: Option<&Arc<dyn TextProvider + Send + Sync>>,
        visibility_transform: Option<Matrix4>,
        parent_key: &str,
    ) -> Result<Vec<Command>> {
        let mut result = Vec::new();
        let commands = expand_retained(commands);
        for (index, command) in commands.iter().enumerate() {
            let Command::DrawSpatialLayer(layer) = command else {
                result.push(command.clone());
                continue;
            };
            let mut planes = Vec::new();
            let key = format!("{parent_key}/{index}");
            self.spatial_planes(
                layer,
                Matrix4::identity(),
                viewport,
                provider,
                &mut planes,
                false,
                visibility_transform,
                &key,
            )?;
            planes.sort_by(|a, b| {
                let depth = |p: &Plane| {
                    let points = spatial::camera_clip(&p.quad);
                    points.iter().map(|p| p[2] / p[3]).sum::<f32>() / points.len().max(1) as f32
                };
                depth(a).total_cmp(&depth(b))
            });
            let depths: Vec<f32> = planes
                .iter()
                .flat_map(|p| spatial::camera_clip(&p.quad))
                .map(|p| p[2] / p[3])
                .filter(|z| z.is_finite())
                .collect();
            let min = depths.iter().copied().fold(f32::INFINITY, f32::min);
            let max = depths.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let middle = (min + max) * 0.5;
            let span = (max - min).max(1.);
            let projected: Vec<_> = planes
                .into_iter()
                .map(|p| Command::DrawProjectedTexture {
                    opacity: p.opacity,
                    texture_id: p.id,
                    bounds: p.bounds,
                    z: 0,
                    quad: p
                        .quad
                        .map(|v| [v[0], v[1], -(v[2] - middle * v[3]) / span * 0.8, v[3]]),
                })
                .collect();
            if let Some((texture_id, rect)) =
                self.spatial_raster(projected, viewport, provider, &format!("{key}/composite"))?
            {
                result.push(Command::DrawExternalTexture {
                    rect,
                    texture_id,
                    z: layer.z,
                    transform: Transform2D::identity(),
                    opacity: 1.,
                    premultiplied: true,
                    rounded_clip: layer.rounded_clip,
                    clip: layer.clip,
                });
            }
        }
        Ok(result)
    }
}
