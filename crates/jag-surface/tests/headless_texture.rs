//! A headless frame can be rendered into a texture the caller owns.
use jag_draw::{ColorLinPremul, wgpu};
use jag_surface::JagSurface;
use std::sync::Arc;

fn gpu() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
    let adapter =
        pollster::block_on(wgpu::Instance::default().request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    (Arc::new(device), Arc::new(queue))
}

fn target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

#[test]
fn rendering_into_a_texture_matches_the_readback_path() {
    let (device, queue) = gpu();
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let mut surface = JagSurface::new(device.clone(), queue.clone(), format);
    let color = [200, 30, 90, 255];

    let mut canvas = surface.begin_frame(5, 3);
    canvas.clear(ColorLinPremul::from_srgba_u8(color));
    let (_, _, expected) = surface.end_frame_headless(canvas).unwrap();

    let texture = target(&device, 5, 3, format);
    let mut canvas = surface.begin_frame(5, 3);
    canvas.clear(ColorLinPremul::from_srgba_u8(color));
    surface.end_frame_into_texture(canvas, &texture).unwrap();

    // Read the texture back to compare: 5 pixels of 4 bytes is padded to 256.
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256 * 3,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(3),
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let mapped = buffer.slice(..).get_mapped_range().unwrap();
    let mut rgba = Vec::new();
    for row in 0..3usize {
        for pixel in mapped[row * 256..row * 256 + 20].chunks_exact(4) {
            rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    assert_eq!(rgba, expected);
}

#[test]
fn a_target_of_the_wrong_size_or_format_is_refused() {
    let (device, queue) = gpu();
    let mut surface = JagSurface::new(device.clone(), queue, wgpu::TextureFormat::Bgra8UnormSrgb);
    for texture in [
        target(&device, 4, 3, wgpu::TextureFormat::Bgra8UnormSrgb),
        target(&device, 5, 3, wgpu::TextureFormat::Rgba8UnormSrgb),
    ] {
        let mut canvas = surface.begin_frame(5, 3);
        canvas.clear(ColorLinPremul::from_srgba_u8([1, 2, 3, 255]));
        assert!(surface.end_frame_into_texture(canvas, &texture).is_err());
    }
}
