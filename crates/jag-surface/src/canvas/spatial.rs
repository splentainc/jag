//! Capture balanced CSS planes without routing text/images through side channels.
use super::Canvas;
use jag_draw::{
    Command, Transform2D,
    spatial::{self, Matrix4, SpatialLayer},
};
use std::sync::Arc;
pub(crate) struct SpatialCapture {
    start: usize,
    ambient_opacity: f32,
    opacity: f32,
    matrix: [f32; 16],
    perspective: [f32; 16],
    preserve: bool,
    backface: bool,
}
impl Canvas {
    pub fn push_spatial_layer(
        &mut self,
        matrix: [f32; 16],
        perspective: [f32; 16],
        preserve: bool,
        backface: bool,
    ) {
        let ambient_opacity = self.current_opacity();
        let opacity = self
            .spatial_layers
            .last()
            .map(|p| {
                if p.ambient_opacity == 0. {
                    0.
                } else {
                    ambient_opacity / p.ambient_opacity
                }
            })
            .unwrap_or(1.);
        let base = self.current_transform();
        let world = spatial::multiply(spatial::affine(base), Matrix4::from_array(matrix));
        let [a, b, c, d, e, f] = base.m;
        let det = a * d - b * c;
        let inverse = if det != 0. {
            Transform2D {
                m: [
                    d / det,
                    -b / det,
                    -c / det,
                    a / det,
                    (c * f - d * e) / det,
                    (b * e - a * f) / det,
                ],
            }
        } else {
            Transform2D::identity()
        };
        self.push_transform(inverse);
        let start = self.command_count();
        self.push_opacity(1.);
        self.spatial_layers.push(SpatialCapture {
            ambient_opacity,
            opacity,
            start,
            matrix: world.to_array(),
            perspective,
            preserve,
            backface,
        });
    }
    pub fn pop_spatial_layer(&mut self) {
        let capture = self.spatial_layers.pop().expect("balanced spatial scopes");
        self.pop_opacity();
        let commands =
            self.display_list().commands[capture.start + 1..self.command_count() - 1].to_vec();
        self.painter.truncate_commands(capture.start);
        self.pop_transform();
        let z = commands
            .iter()
            .filter_map(Command::z_index)
            .min()
            .unwrap_or(0);
        self.extend_commands(&[Command::DrawSpatialLayer(Arc::new(SpatialLayer {
            clip: self.clip_stack.last().copied().flatten(),
            opacity: capture.opacity,
            matrix: capture.matrix,
            perspective: capture.perspective,
            preserve_3d: capture.preserve,
            backface_hidden: capture.backface,
            commands,
            z,
        }))]);
    }
}
