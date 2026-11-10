//! Bind completed files to immutable image draws without repeating page layout.
use super::Canvas;
use jag_draw::{Command, Painter, ScrollScene, spatial::SpatialLayer};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

#[derive(Default)]
pub struct ImageResourceBindings {
    paths: HashMap<PathBuf, PathBuf>,
    scenes: HashMap<u64, Arc<ScrollScene>>,
    layers: HashMap<usize, (Arc<SpatialLayer>, Arc<SpatialLayer>)>,
    live_scenes: HashSet<u64>,
    live_layers: HashSet<usize>,
}
impl ImageResourceBindings {
    /// A completed resource can repaint its already committed image boxes.
    pub fn bind(&mut self, source: PathBuf, file: PathBuf) -> bool {
        if self.paths.get(&source) == Some(&file) {
            return false;
        }
        self.paths.insert(source, file);
        self.scenes.clear();
        self.layers.clear();
        true
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    fn commands(&mut self, source: &[Command]) -> (Vec<Command>, bool) {
        let mut changed = false;
        let commands = source
            .iter()
            .map(|command| {
                match command {
                    Command::DrawImage {
                        path,
                        origin,
                        size,
                        fit,
                        z,
                        transform,
                    } => {
                        if let Some(file) = self.paths.get(path) {
                            changed = true;
                            if is_svg(file) {
                                return Command::DrawSvg {
                                    path: file.clone(),
                                    origin: *origin,
                                    max_size: *size,
                                    style: None,
                                    z: *z,
                                    transform: *transform,
                                };
                            }
                            return Command::DrawImage {
                                path: file.clone(),
                                origin: *origin,
                                size: *size,
                                fit: *fit,
                                z: *z,
                                transform: *transform,
                            };
                        }
                    }
                    Command::DrawSvg {
                        path,
                        origin,
                        max_size,
                        style,
                        z,
                        transform,
                    } => {
                        if let Some(file) = self.paths.get(path) {
                            changed = true;
                            return Command::DrawSvg {
                                path: file.clone(),
                                origin: *origin,
                                max_size: *max_size,
                                style: *style,
                                z: *z,
                                transform: *transform,
                            };
                        }
                    }
                    Command::DrawScrollScene {
                        scene,
                        base,
                        inputs,
                    } => {
                        self.live_scenes.insert(scene.id);
                        let resolved = if let Some(cached) = self.scenes.get(&scene.id) {
                            cached.clone()
                        } else {
                            let (commands, changed) = self.commands(&scene.commands);
                            let resolved = if changed {
                                Arc::new(ScrollScene::new(
                                    commands.into(),
                                    scene.base,
                                    scene.dpi_scale,
                                    scene.provider.clone(),
                                ))
                            } else {
                                scene.clone()
                            };
                            self.scenes.insert(scene.id, resolved.clone());
                            resolved
                        };
                        changed |= !Arc::ptr_eq(scene, &resolved);
                        return Command::DrawScrollScene {
                            scene: resolved,
                            base: *base,
                            inputs: inputs.clone(),
                        };
                    }
                    Command::DrawSpatialLayer(layer) => {
                        let key = Arc::as_ptr(layer) as usize;
                        self.live_layers.insert(key);
                        let resolved = if let Some((_, cached)) = self.layers.get(&key) {
                            cached.clone()
                        } else {
                            let (commands, changed) = self.commands(&layer.commands);
                            let resolved = if changed {
                                let mut copy = (**layer).clone();
                                copy.commands = commands;
                                Arc::new(copy)
                            } else {
                                layer.clone()
                            };
                            self.layers.insert(key, (layer.clone(), resolved.clone()));
                            resolved
                        };
                        changed |= !Arc::ptr_eq(layer, &resolved);
                        return Command::DrawSpatialLayer(resolved);
                    }
                    _ => {}
                }
                command.clone()
            })
            .collect();
        (commands, changed)
    }
}
fn is_svg(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
}
impl Canvas {
    /// Resolve ready image files while preserving the producer's geometry and
    /// immutable command ownership. Retained scene identities stay stable until
    /// a resource binding or committed scene changes.
    pub fn with_image_resources(&self, bindings: &mut ImageResourceBindings) -> Self {
        if bindings.paths.is_empty() {
            return self.clone();
        }
        bindings.live_scenes.clear();
        bindings.live_layers.clear();
        let mut result = self.clone();
        let (commands, _) = bindings.commands(&self.display_list().commands);
        result.painter = Painter::begin_frame(self.viewport);
        result.painter.extend_commands(&commands);
        result.image_draws.retain_mut(|draw| {
            if let Some(file) = bindings.paths.get(&draw.0) {
                if is_svg(file) {
                    result.svg_draws.push((
                        file.clone(),
                        draw.1,
                        draw.2,
                        None,
                        draw.4,
                        draw.5,
                        draw.6,
                        draw.7,
                        draw.8,
                    ));
                    return false;
                }
                draw.0 = file.clone();
            }
            true
        });
        for draw in &mut result.svg_draws {
            if let Some(file) = bindings.paths.get(&draw.0) {
                draw.0 = file.clone();
            }
        }
        for mask in &mut result.url_mask_textures {
            if let Some(file) = bindings.paths.get(&mask.path) {
                mask.path = file.clone();
            }
        }
        bindings
            .scenes
            .retain(|id, _| bindings.live_scenes.contains(id));
        bindings
            .layers
            .retain(|id, _| bindings.live_layers.contains(id));
        result
    }
}
