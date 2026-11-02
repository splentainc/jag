//! Spatial CSS contexts resolve into textured planes before ordinary composition.
use super::{JagSurface, opacity::LayerGeometry};
use anyhow::Result;
use jag_draw::{
    Command, ExternalTextureId, Rect, SurfaceEffect, TextProvider, Transform2D, Viewport,
    spatial::{self, Matrix4, SpatialLayer},
};
use std::sync::Arc;
fn contains_spatial(commands: &[Command]) -> bool {
    commands.iter().any(|c| match c {
        Command::DrawSpatialLayer(_) => true,
        Command::DrawScrollScene { scene, .. } => contains_spatial(&scene.commands),
        _ => false,
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
    ) -> Result<()> {
        let mut layer = layer.clone();
        layer.commands = expand_retained(&layer.commands);
        let matrix = spatial::multiply(parent, Matrix4::from_array(layer.matrix));
        if layer.backface_hidden && spatial::back_facing(matrix) {
            return Ok(());
        }
        if !layer.preserve_3d {
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
            let commands = self.flatten_spatial_layers(&commands, viewport, provider)?;
            if let Some((id, bounds)) = self.spatial_raster(commands, viewport, provider)? {
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
        if let Some((id, bounds)) = self.spatial_raster(own, viewport, provider)? {
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
        for command in &layer.commands {
            if let Command::DrawSpatialLayer(child) = command {
                self.spatial_planes(child, child_matrix, viewport, provider, planes, true)?;
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
        let mut result = Vec::new();
        let commands = expand_retained(commands);
        for command in &commands {
            let Command::DrawSpatialLayer(layer) = command else {
                result.push(command.clone());
                continue;
            };
            let mut planes = Vec::new();
            self.spatial_planes(
                layer,
                Matrix4::identity(),
                viewport,
                provider,
                &mut planes,
                false,
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
            if let Some((texture_id, rect)) = self.spatial_raster(projected, viewport, provider)? {
                result.push(Command::DrawExternalTexture {
                    rect,
                    texture_id,
                    z: layer.z,
                    transform: Transform2D::identity(),
                    opacity: 1.,
                    premultiplied: true,
                    rounded_clip: None,
                    clip: layer.clip,
                });
            }
        }
        Ok(result)
    }
}
