//! Raw pixel draws (video frames, WebView pixels) become image quads.
use super::{JagSurface, apply_transform_to_point, set_last_raw_image_rect};
use crate::canvas::RawImageDraw;
use jag_draw::wgpu;
use std::sync::Arc;

/// An image quad as the unified pass takes it.
pub(super) type PreparedImage = (
    std::path::PathBuf,
    [f32; 2],
    [f32; 2],
    i32,
    f32,
    jag_draw::Transform2D,
    Option<jag_draw::Rect>,
    Option<jag_draw::RoundedRectClipGpu>,
);

impl JagSurface {
    /// Upload each raw draw's pixels and queue a quad for it. A texture is
    /// kept per draw index and recreated only when the source size changes.
    pub(super) fn prepare_raw_image_draws(
        &mut self,
        raw_draws: &[RawImageDraw],
        prepared: &mut Vec<PreparedImage>,
    ) {
        for (i, raw_draw) in raw_draws.iter().enumerate() {
            if raw_draw.src_width == 0 || raw_draw.src_height == 0 {
                continue;
            }

            // Use a fixed path for webview texture - reused across frames
            let raw_path = std::path::PathBuf::from(format!("__webview_texture_{}__", i));

            // If pixels are empty, reuse cached texture from previous frame
            let has_new_pixels = !raw_draw.pixels.is_empty();

            // Check if we need a new texture (only if size changed)
            let need_new_texture =
                if let Some((_, cached_w, cached_h)) = self.pass.try_get_image_view(&raw_path) {
                    // Only recreate if dimensions changed - always reuse otherwise
                    cached_w != raw_draw.src_width || cached_h != raw_draw.src_height
                } else {
                    true
                };

            // Create texture only when needed (first time or size change)
            if need_new_texture && has_new_pixels {
                // Create texture with BGRA format to match CEF's native output
                // This eliminates CPU-side BGRA->RGBA conversion
                let texture = crate::gpu_texture::create_texture_2d(
                    &self.device,
                    "cef-webview-texture",
                    crate::gpu_texture::Texture2dSpec {
                        width: raw_draw.src_width,
                        height: raw_draw.src_height,
                        // BGRA format matches CEF native output - no conversion needed
                        format: wgpu::TextureFormat::Bgra8UnormSrgb,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    },
                );

                // Store in image cache for reuse
                self.pass.store_loaded_image(
                    &raw_path,
                    Arc::new(texture),
                    raw_draw.src_width,
                    raw_draw.src_height,
                );
            }

            // Upload pixels only when we have new data
            if has_new_pixels {
                if let Some((tex, _, _)) = self.pass.get_cached_texture(&raw_path) {
                    // Always upload full frame - CEF provides complete buffer even for partial updates.
                    // The dirty_rects are informational but the buffer is always complete.
                    // This ensures no flickering from partial/stale data.
                    crate::gpu_texture::upload_texture_2d(
                        &self.queue,
                        &tex,
                        [raw_draw.src_width, raw_draw.src_height],
                        raw_draw.src_width * 4,
                        &raw_draw.pixels,
                    );
                }
            }

            // Skip rendering if no cached texture exists (no pixels uploaded yet)
            if self.pass.try_get_image_view(&raw_path).is_none() {
                continue;
            }

            // Apply the canvas transform to the origin - same as regular images.
            // The origin from draw_raw_image is in local (viewport) coordinates and
            // needs to be transformed to screen coordinates.
            let transformed_origin = apply_transform_to_point(raw_draw.origin, raw_draw.transform);

            // Store the transformed rect for hit testing (accessible via get_last_raw_image_rect)
            set_last_raw_image_rect(
                transformed_origin[0],
                transformed_origin[1],
                raw_draw.dst_size[0],
                raw_draw.dst_size[1],
            );

            prepared.push((
                raw_path,
                transformed_origin,
                raw_draw.dst_size,
                raw_draw.z,
                1.0,
                jag_draw::Transform2D::identity(),
                raw_draw.clip,
                None, // no rounded clip for raw images
            ));
        }
    }
}
