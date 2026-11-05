use crate::canvas::{GeneratedMaskTexture, UrlMaskTexture};
use crate::gpu_texture::{
    Texture2dSpec, create_default_texture_view, create_texture_2d, register_external_texture,
    upload_texture_2d,
};

use super::JagSurface;
use std::{collections::VecDeque, sync::Arc};

#[derive(Default)]
pub(super) struct MaskUploads {
    entries: VecDeque<(Arc<[u8]>, [u32; 2], jag_draw::wgpu::TextureView, u64)>,
    bytes: usize,
}

impl JagSurface {
    pub(super) fn register_generated_mask_textures(&mut self, masks: &[GeneratedMaskTexture]) {
        for mask in masks {
            let size = [mask.width, mask.height];
            if let Some(index) =
                self.mask_uploads
                    .entries
                    .iter()
                    .position(|(pixels, old_size, _, _)| {
                        *old_size == size && Arc::ptr_eq(pixels, &mask.pixels)
                    })
            {
                let entry = self.mask_uploads.entries.remove(index).unwrap();
                register_external_texture(&mut self.pass, mask.id, entry.2.clone());
                self.retained_layers.external_revision(mask.id, entry.3);
                self.mask_uploads.entries.push_back(entry);
                continue;
            }
            let (upload_size, pixels) =
                mask.upload_pixels(self.device.limits().max_texture_dimension_2d);
            let texture = create_texture_2d(
                &self.device,
                "generated-css-mask",
                Texture2dSpec {
                    width: upload_size[0],
                    height: upload_size[1],
                    format: jag_draw::wgpu::TextureFormat::Rgba8UnormSrgb,
                    usage: jag_draw::wgpu::TextureUsages::TEXTURE_BINDING
                        | jag_draw::wgpu::TextureUsages::COPY_DST,
                },
            );
            upload_texture_2d(
                &self.queue,
                &texture,
                upload_size,
                upload_size[0] * 4,
                &pixels,
            );
            let view = create_default_texture_view(&texture);
            register_external_texture(&mut self.pass, mask.id, view.clone());
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
            let revision = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.retained_layers.external_revision(mask.id, revision);
            const LIMIT: usize = 256 * 1024 * 1024;
            if mask.pixels.len() <= LIMIT {
                while self.mask_uploads.bytes + mask.pixels.len() > LIMIT
                    || self.mask_uploads.entries.len() >= 128
                {
                    self.mask_uploads.bytes -=
                        self.mask_uploads.entries.pop_front().unwrap().0.len();
                }
                self.mask_uploads.bytes += mask.pixels.len();
                self.mask_uploads
                    .entries
                    .push_back((mask.pixels.clone(), size, view, revision));
            }
        }
    }

    pub(super) fn register_url_mask_textures(&mut self, masks: &[UrlMaskTexture]) {
        for mask in masks {
            let path = crate::resolve_asset_path(&mask.path);
            let svg = svg_asset(&path);
            let view = if svg {
                self.pass
                    .rasterize_svg_to_view(&path, self.dpi_scale, None, &self.queue)
                    .map(|(view, _, _)| view)
                    .unwrap_or_else(|| self.transparent_mask_view())
            } else if let Some((view, _, _)) = self.pass.try_get_image_view(&path) {
                view
            } else {
                self.pending_image_loads |= self.pass.request_image_load(&path);
                self.transparent_mask_view()
            };
            register_external_texture(&mut self.pass, mask.id, view);
        }
    }

    fn transparent_mask_view(&self) -> jag_draw::wgpu::TextureView {
        let texture = create_texture_2d(
            &self.device,
            "pending-url-mask",
            Texture2dSpec {
                width: 1,
                height: 1,
                format: jag_draw::wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: jag_draw::wgpu::TextureUsages::TEXTURE_BINDING
                    | jag_draw::wgpu::TextureUsages::COPY_DST,
            },
        );
        upload_texture_2d(&self.queue, &texture, [1, 1], 4, &[0; 4]);
        create_default_texture_view(&texture)
    }
}

// Browser caches commonly store SVG responses without an extension. Sniff XML
// content as well as named/built-in SVG assets before selecting the decoder.
fn svg_asset(path: &std::path::Path) -> bool {
    use std::io::Read;
    if path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("svg") || s.eq_ignore_ascii_case("svgz"))
    {
        return true;
    }
    let mut prefix = [0u8; 256];
    let Some(count) = std::fs::File::open(path)
        .ok()
        .and_then(|mut f| f.read(&mut prefix).ok())
    else {
        return false;
    };
    let bytes = &prefix[..count];
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    bytes.iter().find(|b| !b.is_ascii_whitespace()).copied() == Some(b'<')
}
