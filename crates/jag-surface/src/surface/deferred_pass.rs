//! Recording a Canvas does not require shaders, atlases or GPU render passes.
//! Materialize those resources only when a host actually uses the GPU facade.
use jag_draw::{PassManager, wgpu};
use std::{
    ops::{Deref, DerefMut},
    sync::{Arc, OnceLock},
};

pub(super) struct DeferredPass {
    device: Arc<wgpu::Device>,
    target: wgpu::TextureFormat,
    color: Option<(wgpu::TextureFormat, bool)>,
    value: OnceLock<PassManager>,
}
impl DeferredPass {
    pub fn new(device: Arc<wgpu::Device>, target: wgpu::TextureFormat) -> Self {
        Self {
            device,
            target,
            color: None,
            value: OnceLock::new(),
        }
    }
    pub fn with_color_space(
        device: Arc<wgpu::Device>,
        target: wgpu::TextureFormat,
        offscreen: wgpu::TextureFormat,
        web: bool,
    ) -> Self {
        Self {
            device,
            target,
            color: Some((offscreen, web)),
            value: OnceLock::new(),
        }
    }
    pub fn get(&self) -> Option<&PassManager> {
        self.value.get()
    }
    pub fn get_mut(&mut self) -> Option<&mut PassManager> {
        self.value.get_mut()
    }
    fn create(&self) -> PassManager {
        if std::env::var_os("JAG_PROFILE_PASSES").is_some() {
            eprintln!(
                "[gpu-pass-init] thread={:?} target={:?} color={:?}\n{}",
                std::thread::current().name(),
                self.target,
                self.color,
                std::backtrace::Backtrace::force_capture()
            );
        }
        match self.color {
            Some((offscreen, web)) => {
                PassManager::new_with_color_space(self.device.clone(), self.target, offscreen, web)
            }
            None => PassManager::new(self.device.clone(), self.target),
        }
    }
}
impl Deref for DeferredPass {
    type Target = PassManager;
    fn deref(&self) -> &PassManager {
        self.value.get_or_init(|| self.create())
    }
}
impl DerefMut for DeferredPass {
    fn deref_mut(&mut self) -> &mut PassManager {
        if self.value.get().is_none() {
            let _ = self.value.set(self.create());
        }
        self.value.get_mut().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recording_and_color_selection_allocate_no_passes_then_render_real_pixels() {
        let adapter =
            pollster::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
                .unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let mut surface = super::super::JagSurface::new(
            Arc::new(device),
            Arc::new(queue),
            wgpu::TextureFormat::Rgba8UnormSrgb,
        );
        assert!(surface.pass.get().is_none());
        surface.set_web_color_composition(true);
        surface.set_web_color_composition(false);
        surface.set_web_color_composition(true);
        let mut canvas = surface.begin_frame(2, 2);
        canvas.clear(jag_draw::ColorLinPremul::from_srgba_u8([255, 0, 0, 255]));
        let _ = surface.snapshot_external_textures();
        surface.clear_external_textures();
        surface.remove_external_texture(jag_draw::ExternalTextureId(42));
        assert_eq!(surface.prune_expired_external_textures(), 0);
        assert!(surface.pass.get().is_none());
        let (_, _, pixels) = surface.end_frame_headless(canvas).unwrap();
        assert!(surface.pass.get().is_some());
        assert_eq!(&pixels[..4], &[255, 0, 0, 255]);
    }
}
