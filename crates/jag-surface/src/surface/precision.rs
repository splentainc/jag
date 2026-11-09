//! Float composition with explicit native-linear or browser-sRGB blending.
use super::deferred_pass::DeferredPass;
use super::{JagSurface, opacity::LayerGeometry};
use anyhow::Result;
use jag_draw::{Command, ExternalTextureId, SurfaceEffect, TextProvider, wgpu};
use std::sync::Arc;

impl JagSurface {
    /// Upgrade this surface's intermediate buffers when native content may
    /// contain colors outside sRGB. The final display format stays unchanged.
    /// Ordinary surfaces retain the existing bounded SDR rendering path.
    pub fn enable_extended_color_composition(&mut self) {
        if !self.web_srgb_composition {
            self.native_extended_color_composition = true;
        }
        if self.extended_color_composition {
            return;
        }
        self.rebuild_color_composition();
    }

    /// Browser CSS compositing uses encoded sRGB with exact authored alpha.
    /// Float intermediates preserve precision; sources project into the sRGB page gamut.
    pub fn enable_web_color_composition(&mut self) {
        self.set_web_color_composition(true);
    }

    /// Select the page blend space when a host switches between web and native IR.
    pub fn set_web_color_composition(&mut self, enabled: bool) {
        if self.web_srgb_composition == enabled {
            return;
        }
        self.web_srgb_composition = enabled;
        self.extended_group_pass = None;
        self.rebuild_color_composition();
    }

    fn rebuild_color_composition(&mut self) {
        self.extended_color_composition =
            self.web_srgb_composition || self.native_extended_color_composition;
        let mut pass = if self.extended_color_composition {
            DeferredPass::with_color_space(
                self.device.clone(),
                self.surface_format,
                wgpu::TextureFormat::Rgba16Float,
                self.web_srgb_composition,
            )
        } else {
            DeferredPass::new(self.device.clone(), self.surface_format)
        };
        if let Some(old) = self.pass.get_mut() {
            old.copy_external_textures_to(&mut pass);
            pass.set_scroll_offset(old.scroll_offset());
            std::mem::swap(&mut old.image_cache, &mut pass.image_cache);
            std::mem::swap(&mut old.svg_cache, &mut pass.svg_cache);
        }
        self.pass = pass;
        self.frame_cache = None;
        self.retained_layers.clear();
        self.scroll_tiles = Default::default();
    }

    pub(super) fn render_effect_group_layer(
        &mut self,
        geometry: LayerGeometry,
        mut commands: Vec<Command>,
        effect: SurfaceEffect,
        text_provider: Option<&Arc<dyn TextProvider + Send + Sync>>,
        logical_scale: f32,
        stable_id: Option<ExternalTextureId>,
    ) -> Result<ExternalTextureId> {
        // Resolve retained pixels before preparing or swapping the float pass.
        // Scrolling can reuse hundreds of tiles; copying all external bindings
        // into and out of a render pass for each cache hit is unnecessary.
        super::opacity::localize_clips(&mut commands, geometry.origin, logical_scale);
        let tex_id = stable_id.unwrap_or_else(|| self.allocate_synthetic_external_texture_id());
        let images_ready = commands.iter().all(|command| match command {
            Command::DrawImage { path, .. } => {
                self.pass.is_image_ready(&crate::resolve_asset_path(path))
            }
            _ => true,
        });
        let retained_key = images_ready
            .then(|| {
                self.retained_layers.key(
                    &commands,
                    &effect,
                    geometry.origin,
                    geometry.pixel_size,
                    logical_scale,
                    text_provider,
                )
            })
            .flatten();
        if let Some(key) = &retained_key
            && let Some(view) = self.retained_layers.lookup(tex_id, key)
        {
            self.pass.register_shared_external_texture(tex_id, view);
            return Ok(tex_id);
        }
        if !self.extended_color_composition {
            return self.render_effect_group_layer_inner(
                geometry,
                commands,
                effect,
                text_provider,
                (tex_id, retained_key),
            );
        }
        // Effect groups use the root's precision and blend space. A cached
        // float-target pass keeps its shader formats and transfer consistent.
        let mut group_pass = self.extended_group_pass.take().unwrap_or_else(|| {
            DeferredPass::with_color_space(
                self.device.clone(),
                wgpu::TextureFormat::Rgba16Float,
                wgpu::TextureFormat::Rgba16Float,
                self.web_srgb_composition,
            )
        });
        group_pass.begin_composite_frame();
        group_pass.set_scale_factor(self.dpi_scale);
        group_pass.set_logical_pixels(self.logical_pixels);
        group_pass.set_ui_scale(self.ui_scale);
        self.pass.copy_external_textures_to(&mut group_pass);
        std::mem::swap(&mut self.pass.image_cache, &mut group_pass.image_cache);
        std::mem::swap(&mut self.pass.svg_cache, &mut group_pass.svg_cache);
        std::mem::swap(&mut self.pass, &mut group_pass);
        let display_format =
            std::mem::replace(&mut self.surface_format, wgpu::TextureFormat::Rgba16Float);
        let result = self.render_effect_group_layer_inner(
            geometry,
            commands,
            effect,
            text_provider,
            (tex_id, retained_key),
        );
        // Restore both success and failure paths. Newly created group views
        // belong to the parent frame; the cached group pass retains no views.
        self.surface_format = display_format;
        self.pass.copy_external_textures_to(&mut group_pass);
        self.pass.clear_external_textures();
        std::mem::swap(&mut self.pass, &mut group_pass);
        std::mem::swap(&mut self.pass.image_cache, &mut group_pass.image_cache);
        std::mem::swap(&mut self.pass.svg_cache, &mut group_pass.svg_cache);
        self.extended_group_pass = Some(group_pass);
        result
    }
}
