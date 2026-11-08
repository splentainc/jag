//! Preserve the opaque destination before compositing analytic shadows.
use super::{PassManager, PassTargets};
use crate::{
    allocator::RenderAllocator, gpu_commands::CommandEncoderExt as _,
    gpu_transfer::create_transient_upload,
};
impl PassManager {
    pub(super) fn record_offscreen_shadows(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        allocator: &mut RenderAllocator,
        targets: &PassTargets,
        width: u32,
        height: u32,
    ) {
        // Analytic box shadows, composited in sRGB (gamma) space to match
        // Chrome. GPU fixed-function blend only works in the offscreen's stored
        // LINEAR space, so instead we snapshot the destination, do the sRGB
        // "over" in-shader (`SHADOW_INSTANCE_COMPOSITE_WGSL`), and write the
        // result with REPLACE blend. This runs after the opaque solids (so the
        // snapshot and depth test see them) and before the transparent
        // interleave. Skipped entirely when there are no shadows.
        //
        // Limitation: all overlapping shadows read the same pre-shadow
        // snapshot, so they do not accumulate against each other. Accepted.
        if !self.shadow_instances.is_empty() && self.gamma_blend {
            self.record_blended_shadows(encoder, targets);
        } else if !self.shadow_instances.is_empty() {
            let shadow_buf = create_transient_upload(
                &self.device,
                "shadow-instances",
                bytemuck::cast_slice(&self.shadow_instances),
                wgpu::BufferUsages::VERTEX,
            );
            let shadow_vp_bg = crate::gpu_bindings::create_bind_group(
                &self.device,
                "shadow-composite-vp-bg-offscreen",
                self.shadow_composite.viewport_bgl(),
                &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.vp_buffer.as_entire_binding(),
                }],
            );

            // Snapshot of the offscreen scene target (pre-shadow). Same size +
            // format as the scene target; only needs sampling + copy-dst.
            let snapshot = allocator.allocate_texture(crate::allocator::TexKey {
                width: width.max(1),
                height: height.max(1),
                format: self.offscreen_format,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            });
            crate::gpu_texture::copy_texture_2d(
                encoder,
                &targets.color.texture,
                &snapshot.texture,
                [width.max(1), height.max(1)],
            );
            let shadow_dst_bg = crate::gpu_bindings::create_bind_group(
                &self.device,
                "shadow-composite-dst-bg",
                self.shadow_composite.dst_bgl(),
                &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&snapshot.view),
                }],
            );

            {
                let mut shadow_pass =
                    encoder.begin_semantic_render_pass(crate::gpu_commands::RenderPassDescriptor {
                        label: Some("unified-offscreen-shadow-composite-pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &targets.color.view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: self.depth_view(),
                            // Depth is read-only here (pipeline depth_write=false);
                            // load + store keeps the opaque depth intact for the
                            // transparent interleave that follows.
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        occlusion_query_set: None,
                        timestamp_writes: None,
                    });
                self.shadow_composite.record(
                    &mut shadow_pass,
                    &shadow_vp_bg,
                    &shadow_dst_bg,
                    &shadow_buf,
                    self.shadow_instances.len() as u32,
                );
            }
            allocator.release_texture(snapshot);
        }
    }
}
