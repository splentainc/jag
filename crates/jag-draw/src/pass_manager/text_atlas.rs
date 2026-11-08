//! Keep the established atlas width; allocate rows only as visible glyphs need
//! them. UVs use the current height, so growth preserves full-resolution masks.
use super::PassManager;

fn required_height(sizes: impl Iterator<Item = [u32; 2]>) -> u32 {
    let (mut x, mut y, mut row, mut bottom) = (0u32, 0u32, 0u32, 1u32);
    for [w, h] in sizes {
        if x.saturating_add(w) >= 4096 {
            x = 0;
            y = y.saturating_add(row);
            row = 0;
        }
        row = row.max(h);
        bottom = bottom.max(y.saturating_add(h));
        x = x.saturating_add(w);
    }
    bottom.min(4096).next_power_of_two()
}

impl PassManager {
    pub(super) fn prepare_text_atlas(&mut self, sizes: impl Iterator<Item = [u32; 2]>) {
        let height = required_height(sizes);
        if self.text_mask_atlas.height() >= height {
            return;
        }
        let texture = crate::gpu_texture::create_texture_2d(
            &self.device,
            "text-mask-atlas",
            crate::gpu_texture::Texture2dSpec {
                width: 4096,
                height,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::RENDER_ATTACHMENT,
            },
        );
        let view = crate::gpu_texture::create_default_texture_view(&texture);
        self.text_bind_group = crate::gpu_bindings::create_bind_group(
            &self.device,
            "text-mask-bgl",
            &self.text.tex_bgl,
            &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.text.sampler),
                },
            ],
        );
        self.text_mask_atlas_view = view;
        self.text_mask_atlas = texture;
    }
}

#[cfg(test)]
mod tests {
    use super::required_height;
    #[test]
    fn atlas_rows_grow_for_the_actual_packing_including_tall_rows_and_wraps() {
        assert_eq!(required_height(std::iter::empty()), 1);
        assert_eq!(required_height([[100, 24], [100, 80]].into_iter()), 128);
        assert_eq!(required_height([[3000, 200], [3000, 200]].into_iter()), 512);
        assert_eq!(required_height(std::iter::repeat_n([3000, 300], 30)), 4096);
    }
}
