//! Recomposition of a finished, immutable canvas without invoking its producer.
use super::Canvas;
use jag_draw::{Command, ScrollInputs, Viewport};

impl Canvas {
    /// Replay completed content at the current output size. The producer's
    /// retained scroll scenes own their commands and resources; updating inputs
    /// never reads a document or changes that committed version.
    pub fn recompose(
        &self,
        viewport: Viewport,
        dpi_scale: f32,
        translation: [f32; 2],
        inputs: &ScrollInputs,
    ) -> Self {
        let mut result = self.clone();
        result.viewport = viewport;
        result.dpi_scale = dpi_scale;
        let mut commands = self.display_list().commands.clone();
        for command in &mut commands {
            if let Command::DrawScrollScene {
                base, inputs: live, ..
            } = command
            {
                base.m[4] += translation[0];
                base.m[5] += translation[1];
                let mut merged = (**live).clone();
                merged.extend(inputs.iter().map(|(key, value)| (key.clone(), *value)));
                *live = std::sync::Arc::new(merged);
            }
        }
        result.painter = jag_draw::Painter::begin_frame(viewport);
        result.painter.extend_commands(&commands);
        result
    }

    /// Non-retained side channels cannot yet accept independent scroll
    /// transforms. They remain displayable while the producer is busy, but
    /// input requiring their movement must be routed to that producer.
    pub fn supports_independent_scroll(&self) -> bool {
        self.capture_side_channels() == [0; 9]
            && self
                .display_list()
                .commands
                .iter()
                .any(|command| matches!(command, Command::DrawScrollScene { .. }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ImageFitMode, ScrimDraw};
    use super::*;
    use jag_draw::{Brush, ColorLinPremul, Command, Painter};
    fn test_canvas() -> Canvas {
        let viewport = Viewport {
            width: 320,
            height: 240,
        };
        Canvas {
            spatial_layers: Vec::new(),
            viewport,
            painter: Painter::begin_frame(viewport),
            clear_color: None,
            text_provider: None,
            glyph_draws: Vec::new(),
            svg_draws: Vec::new(),
            image_draws: Vec::new(),
            backdrop_blur_draws: Vec::new(),
            raw_image_draws: Vec::new(),
            dpi_scale: 1.0,
            clip_stack: vec![None],
            rounded_clip_stack: vec![None],
            overlay_draws: Vec::new(),
            scrim_draws: Vec::<ScrimDraw>::new(),
            opacity_stack: vec![1.0],
            generated_mask_textures: Vec::new(),
            url_mask_textures: Vec::new(),
            next_generated_mask_texture_id: 0x6000_0000_0000_0000,
        }
    }

    #[test]
    fn recomposition_keeps_commit_immutable_and_rebases_live_scroll_after_resize() {
        let mut canvas = test_canvas();
        let capture = canvas.begin_scroll_scene_capture();
        canvas.push_scroll_layer("nested", [0., 0.]);
        canvas.fill_rect(
            10.,
            20.,
            40.,
            30.,
            Brush::Solid(ColorLinPremul::from_srgba_u8([0, 0, 0, 255])),
            0,
        );
        canvas.pop_scroll_layer();
        let scene = canvas.finish_scroll_scene_capture(capture).unwrap();
        assert!(canvas.supports_independent_scroll());
        let inputs = [("nested".into(), [0., 12.])].into_iter().collect();
        let moved = canvas.recompose(
            Viewport {
                width: 640,
                height: 480,
            },
            2.,
            [0., -50.],
            &inputs,
        );
        let Command::DrawScrollScene {
            base,
            inputs,
            scene: retained,
        } = &moved.display_list().commands[0]
        else {
            panic!("retained scene")
        };
        assert!(std::sync::Arc::ptr_eq(&scene, retained));
        assert_eq!(base.m[5], -50.);
        assert_eq!(inputs["nested"], [0., 12.]);
        assert_eq!(moved.viewport().width, 640);
        let Command::DrawScrollScene { base, inputs, .. } = &canvas.display_list().commands[0]
        else {
            panic!("original scene")
        };
        assert_eq!(base.m[5], 0.);
        assert!(inputs.is_empty());
        assert_eq!(canvas.viewport().width, 320);
        canvas.draw_image("live.png", [0., 0.], [20., 20.], ImageFitMode::Contain, 0);
        assert!(!canvas.supports_independent_scroll());
    }
}
