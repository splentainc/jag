//! Driver allocations must scale with framebuffer reads, not draw count.
use super::*;
use crate::{Brush, ColorLinPremul, Painter, Rect, Viewport};
use std::sync::Arc;

#[test]
fn ordinary_draws_share_passes_and_backdrops_split_them() {
    let adapter =
        pollster::block_on(wgpu::Instance::default().request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let device = Arc::new(device);
    let mut allocator = RenderAllocator::new(device.clone());
    let mut manager = PassManager::new_with_color_space(
        device.clone(),
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba16Float,
        false,
    );
    let target = crate::gpu_texture::create_texture_2d(
        &device,
        "batched offscreen regression",
        crate::gpu_texture::Texture2dSpec {
            width: 16,
            height: 16,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        },
    );
    for backdrops in [false, true] {
        let mut painter = Painter::begin_frame(Viewport {
            width: 16,
            height: 16,
        });
        for z in 0..100 {
            painter.rect(
                Rect {
                    x: 0.,
                    y: 0.,
                    w: 16.,
                    h: 16.,
                },
                Brush::Solid(ColorLinPremul::from_srgba_u8([255, 0, 0, 128])),
                z,
            );
        }
        let scene =
            crate::upload_display_list_unified(&mut allocator, &queue, painter.display_list())
                .unwrap();
        assert_eq!(scene.transparent_batches.len(), 100);
        let blur = crate::BackdropBlurDraw {
            rect: Rect {
                x: 0.,
                y: 0.,
                w: 16.,
                h: 16.,
            },
            z: 50,
            effects: vec![crate::FilterEffect::Blur(1.)],
            transform: crate::Transform2D::identity(),
            clip: None,
        };
        let mut encoder = device.create_command_encoder(&Default::default());
        crate::gpu_commands::OFFSCREEN_PASSES.with(|count| count.set(0));
        manager.render_unified(
            &mut encoder,
            &mut allocator,
            &crate::gpu_texture::create_default_texture_view(&target),
            16,
            16,
            &scene.gpu_scene,
            &scene.solid_batches,
            &scene.transparent_gpu_scene,
            &scene.transparent_batches,
            &[],
            &[],
            &[],
            if backdrops {
                std::slice::from_ref(&blur)
            } else {
                &[]
            },
            &[],
            wgpu::Color::WHITE,
            false,
            &queue,
            false,
        );
        assert_eq!(
            crate::gpu_commands::OFFSCREEN_PASSES.with(|count| count.get()),
            if backdrops { 2 } else { 1 }
        );
        queue.submit([encoder.finish()]);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    }
}
