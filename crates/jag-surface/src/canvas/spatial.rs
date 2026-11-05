//! Capture balanced CSS planes without routing text/images through side channels.
use super::{Canvas, RoundedRectClip};
use jag_draw::{
    Command, Transform2D,
    spatial::{self, Matrix4, SpatialLayer},
};
use std::sync::Arc;
#[derive(Clone)]
pub(crate) struct SpatialCapture {
    start: usize,
    ambient_opacity: f32,
    opacity: f32,
    matrix: [f32; 16],
    perspective: [f32; 16],
    preserve: bool,
    backface: bool,
    clips: Vec<Option<jag_draw::Rect>>,
    rounded_clips: Vec<Option<RoundedRectClip>>,
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
        // The plane is recorded in its own coordinate system. Ambient clips
        // belong to the final projected plane, not to its unprojected ink.
        // Keeping a viewport clip here discards artwork below the viewport
        // before the scroll/3D transform can bring it into view.
        let clips = std::mem::replace(&mut self.clip_stack, vec![None]);
        let rounded_clips = std::mem::replace(&mut self.rounded_clip_stack, vec![None]);
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
            clips,
            rounded_clips,
        });
    }
    pub fn pop_spatial_layer(&mut self) {
        let capture = self.spatial_layers.pop().expect("balanced spatial scopes");
        self.pop_opacity();
        let commands =
            self.display_list().commands[capture.start + 1..self.command_count() - 1].to_vec();
        self.painter.truncate_commands(capture.start);
        self.pop_transform();
        self.clip_stack = capture.clips;
        self.rounded_clip_stack = capture.rounded_clips;
        // An empty child plane must not introduce a synthetic z=0 into its
        // parent's paint order. Invisible hover/reveal children otherwise
        // move the entire 3D context behind the document background.
        let Some(z) = commands.iter().filter_map(Command::z_index).min() else {
            return;
        };
        self.extend_commands(&[Command::DrawSpatialLayer(Arc::new(SpatialLayer {
            clip: self.clip_stack.last().copied().flatten(),
            rounded_clip: self
                .rounded_clip_stack
                .iter()
                .rev()
                .find_map(|c| *c)
                .map(|c| jag_draw::RoundedRectClipGpu {
                    rect: [c.rect.x, c.rect.y, c.rect.w, c.rect.h],
                    radii: c.radii,
                }),
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
