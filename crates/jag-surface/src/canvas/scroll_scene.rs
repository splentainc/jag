use super::Canvas;

/// A capture boundary also records side channels. Paint with live pixels or
/// newly generated masks cannot be replayed as commands alone.
pub struct ScrollSceneCapture {
    start: usize,
    base: jag_draw::Transform2D,
    side_channels: [usize; 9],
    next_mask_id: u64,
}

impl Canvas {
    pub(crate) fn capture_side_channels(&self) -> [usize; 9] {
        [
            self.glyph_draws.len(),
            self.svg_draws.len(),
            self.image_draws.len(),
            self.raw_image_draws.len(),
            self.backdrop_blur_draws.len(),
            self.generated_mask_textures.len(),
            self.url_mask_textures.len(),
            self.overlay_draws.len(),
            self.scrim_draws.len(),
        ]
    }

    pub fn begin_scroll_scene_capture(&self) -> ScrollSceneCapture {
        ScrollSceneCapture {
            start: self.command_count(),
            base: self.current_transform(),
            side_channels: self.capture_side_channels(),
            next_mask_id: self.next_generated_mask_texture_id,
        }
    }

    pub fn finish_scroll_scene_capture(
        &mut self,
        capture: ScrollSceneCapture,
    ) -> Option<std::sync::Arc<jag_draw::ScrollScene>> {
        if capture.side_channels != self.capture_side_channels() {
            return None;
        }
        let commands = &self.display_list().commands[capture.start..];
        // These effects carry additional world-space mask/backdrop geometry;
        // they need a committed resource binding before translation-only replay.
        if commands.iter().any(|command| {
            matches!(
                command,
                jag_draw::Command::BackdropFilter(_)
                    | jag_draw::Command::PushFilter(
                        jag_draw::FilterEffect::Mask(_) | jag_draw::FilterEffect::MaskGroup(_)
                    )
            )
        }) {
            return None;
        }
        Some(self.wrap_scroll_scene(capture))
    }

    /// Freeze this canvas's content for compositor scrolling. Generated/URL
    /// mask bindings stay owned by this immutable canvas and its clones. Unlike
    /// cross-canvas capture, this does not export a resource-less reusable scene.
    pub fn finish_scroll_scene_commit(&mut self, capture: ScrollSceneCapture) -> bool {
        let after = self.capture_side_channels();
        if after
            .iter()
            .zip(capture.side_channels)
            .enumerate()
            .any(|(i, (a, b))| !matches!(i, 5 | 6) && *a != b)
        {
            return false;
        }
        // A backdrop's effective device clip mixes fixed viewport and moving
        // ancestor clips. Keep it producer-owned until clip ownership is bound.
        if self.display_list().commands[capture.start..].iter().any(|command| {
            matches!(command, jag_draw::Command::BackdropFilter(draw) if draw.clip.is_some())
        }) {
            return false;
        }
        self.wrap_scroll_scene(capture);
        true
    }

    fn wrap_scroll_scene(
        &mut self,
        capture: ScrollSceneCapture,
    ) -> std::sync::Arc<jag_draw::ScrollScene> {
        let commands = &self.display_list().commands[capture.start..];
        let scene = std::sync::Arc::new(jag_draw::ScrollScene::new(
            commands.to_vec().into(),
            capture.base,
            self.dpi_scale,
            self.text_provider.clone(),
        ));
        self.painter.truncate_commands(capture.start);
        self.replay_scroll_scene(&scene, &Default::default());
        scene
    }

    pub fn can_replay_scroll_scene(&self, scene: &jag_draw::ScrollScene) -> bool {
        scene.dpi_scale == self.dpi_scale
            && scene.base.m[..4] == self.current_transform().m[..4]
            && match (&scene.provider, &self.text_provider) {
                (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
    }

    pub fn replay_scroll_scene(
        &mut self,
        scene: &std::sync::Arc<jag_draw::ScrollScene>,
        inputs: &jag_draw::ScrollInputs,
    ) {
        self.painter
            .extend_commands(&[jag_draw::Command::DrawScrollScene {
                scene: scene.clone(),
                base: self.current_transform(),
                inputs: std::sync::Arc::new(inputs.clone()),
            }]);
    }
}

/// Immutable scroll commands together with the mask resources they reference.
/// Unlike command-only captures, this can replay CSS masks and outer shadows
/// onto a new canvas without dropping their texture bindings.
pub struct RetainedScrollScene {
    scene: std::sync::Arc<jag_draw::ScrollScene>,
    generated: Vec<super::GeneratedMaskTexture>,
    urls: Vec<super::UrlMaskTexture>,
    mask_ids: std::ops::Range<u64>,
}

impl Canvas {
    pub fn finish_retained_scroll_scene(
        &mut self,
        capture: ScrollSceneCapture,
    ) -> Option<RetainedScrollScene> {
        let after = self.capture_side_channels();
        if after
            .iter()
            .zip(capture.side_channels)
            .enumerate()
            .any(|(i, (a, b))| !matches!(i, 5 | 6) && *a != b)
            || self.display_list().commands[capture.start..]
                .iter()
                .any(|c| matches!(c, jag_draw::Command::BackdropFilter(_)))
        {
            return None;
        }
        let generated = self.generated_mask_textures[capture.side_channels[5]..].to_vec();
        let urls = self.url_mask_textures[capture.side_channels[6]..].to_vec();
        let mask_ids = capture.next_mask_id..self.next_generated_mask_texture_id;
        Some(RetainedScrollScene {
            scene: self.wrap_scroll_scene(capture),
            generated,
            urls,
            mask_ids,
        })
    }

    pub fn can_replay_retained_scroll_scene(&self, retained: &RetainedScrollScene) -> bool {
        self.can_replay_scroll_scene(&retained.scene)
            // Keep resource IDs stable for compiled/tile caches. If earlier
            // producers allocated into this range, replay must not overwrite
            // their bindings; let the producer make a fresh capture instead.
            && (retained.mask_ids.is_empty()
                || self.next_generated_mask_texture_id <= retained.mask_ids.start)
    }

    /// Returns false without modifying the canvas if its resources or scale
    /// conflict with the capture. The caller then paints a fresh version.
    pub fn replay_retained_scroll_scene(
        &mut self,
        retained: &RetainedScrollScene,
        inputs: &jag_draw::ScrollInputs,
    ) -> bool {
        if !self.can_replay_retained_scroll_scene(retained) {
            return false;
        }
        self.generated_mask_textures
            .extend(retained.generated.iter().cloned());
        self.url_mask_textures.extend(retained.urls.iter().cloned());
        if !retained.mask_ids.is_empty() {
            self.next_generated_mask_texture_id = retained.mask_ids.end;
        }
        self.replay_scroll_scene(&retained.scene, inputs);
        true
    }
}
