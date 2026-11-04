use crate::canvas::{GeneratedMaskTexture, UrlMaskTexture};
use crate::gpu_texture::{
    Texture2dSpec, create_default_texture_view, create_texture_2d, register_external_texture,
    upload_texture_2d,
};

use super::JagSurface;

impl JagSurface {
    pub(super) fn register_generated_mask_textures(&mut self, masks: &[GeneratedMaskTexture]) {
        for mask in masks {
            let texture = create_texture_2d(
                &self.device,
                "generated-css-mask",
                Texture2dSpec {
                    width: mask.width,
                    height: mask.height,
                    format: jag_draw::wgpu::TextureFormat::Rgba8UnormSrgb,
                    usage: jag_draw::wgpu::TextureUsages::TEXTURE_BINDING
                        | jag_draw::wgpu::TextureUsages::COPY_DST,
                },
            );
            upload_texture_2d(
                &self.queue,
                &texture,
                [mask.width, mask.height],
                mask.width * 4,
                &mask.pixels,
            );
            register_external_texture(
                &mut self.pass,
                mask.id,
                create_default_texture_view(&texture),
            );
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
