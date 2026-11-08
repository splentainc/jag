use std::sync::Arc;
#[path = "allocator_pool.rs"]
mod pool;
use pool::ResourcePool;

use crate::gpu_texture::{Texture2dSpec, create_default_texture_view, create_texture_2d};
use crate::gpu_transfer::allocate_buffer_resource;

#[derive(Debug)]
pub struct OwnedTexture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub key: TexKey,
}

#[derive(Debug)]
pub struct OwnedBuffer {
    pub buffer: wgpu::Buffer,
    pub key: BufKey,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct TexKey {
    pub width: u32,
    pub height: u32,
    pub format: wgpu::TextureFormat,
    pub usage: wgpu::TextureUsages,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct BufKey {
    pub size: u64,
    pub usage: wgpu::BufferUsages,
}

/// Simple render allocator with basic pooling for textures and buffers.
pub struct RenderAllocator {
    device: Arc<wgpu::Device>,
    texture_pool: ResourcePool<TexKey, wgpu::Texture>,
    buffer_pool: ResourcePool<BufKey, wgpu::Buffer>,
}

fn pooled_buffer_size(size: u64) -> u64 {
    let size = size.max(4);
    if size <= 1_048_576 {
        size.next_power_of_two()
    } else {
        let alignment = 1_048_576;
        size.div_ceil(alignment) * alignment
    }
}

impl RenderAllocator {
    pub fn new(device: Arc<wgpu::Device>) -> Self {
        Self {
            device,
            texture_pool: ResourcePool::new(128 * 1024 * 1024, 128),
            buffer_pool: ResourcePool::new(16 * 1024 * 1024, 128),
        }
    }

    pub fn begin_frame(&mut self) {
        // Idle storage spans frames; allocation and release update accounting.
    }

    pub fn end_frame(&mut self) {
        // Callers explicitly release resources after their last encoded use.
    }

    pub fn allocate_texture(&mut self, key: TexKey) -> OwnedTexture {
        let texture = self.texture_pool.take(&key).unwrap_or_else(|| {
            create_texture_2d(
                &self.device,
                "alloc:tex",
                Texture2dSpec {
                    width: key.width,
                    height: key.height,
                    format: key.format,
                    usage: key.usage,
                },
            )
        });
        let view = create_default_texture_view(&texture);
        OwnedTexture { texture, view, key }
    }

    pub fn release_texture(&mut self, tex: OwnedTexture) {
        let (block_w, block_h) = tex.key.format.block_dimensions();
        let block_bytes = tex.key.format.block_copy_size(None).unwrap_or(16) as u64;
        let bytes = u64::from(tex.key.width.div_ceil(block_w))
            .saturating_mul(u64::from(tex.key.height.div_ceil(block_h)))
            .saturating_mul(block_bytes);
        self.texture_pool.put(tex.key, tex.texture, bytes);
        self.profile_idle_storage();
    }

    pub fn allocate_buffer(&mut self, key: BufKey) -> OwnedBuffer {
        let key = BufKey {
            size: pooled_buffer_size(key.size),
            usage: key.usage,
        };
        let buffer = self.buffer_pool.take(&key).unwrap_or_else(|| {
            allocate_buffer_resource(&self.device, "alloc:buf", key.size, key.usage)
        });
        OwnedBuffer { buffer, key }
    }

    pub fn release_buffer(&mut self, buf: OwnedBuffer) {
        self.buffer_pool.put(buf.key, buf.buffer, buf.key.size);
        self.profile_idle_storage();
    }

    fn profile_idle_storage(&self) {
        if std::env::var_os("JAG_PROFILE_ALLOCATOR").is_some() {
            eprintln!(
                "[gpu-idle-pool] textures={} buffers={}",
                self.texture_pool.bytes(),
                self.buffer_pool.bytes()
            );
        }
    }
}
