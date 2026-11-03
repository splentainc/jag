use super::PassManager;
use std::sync::Arc;

/// Immutable bindings transferred with a completed scene. Strong frame leases
/// survive producer retirement and remain alive through queue submission.
#[derive(Clone, Default)]
pub struct ExternalTextureBindings {
    _leases: Vec<Arc<()>>,
    views: std::collections::HashMap<crate::ExternalTextureId, Arc<wgpu::TextureView>>,
    linear: std::collections::HashSet<crate::ExternalTextureId>,
    owners:
        std::collections::HashMap<crate::ExternalTextureId, Arc<dyn std::any::Any + Send + Sync>>,
    lifetimes: std::collections::HashMap<crate::ExternalTextureId, std::sync::Weak<()>>,
}

impl PassManager {
    pub fn snapshot_external_textures(&self) -> ExternalTextureBindings {
        ExternalTextureBindings {
            _leases: self
                .external_texture_lifetimes
                .values()
                .filter_map(std::sync::Weak::upgrade)
                .collect(),
            views: self.external_textures.clone(),
            linear: self.linear_external_textures.clone(),
            owners: self.external_texture_owners.clone(),
            lifetimes: self.external_texture_lifetimes.clone(),
        }
    }

    pub fn install_external_textures(&mut self, bindings: &ExternalTextureBindings) {
        self.external_textures.clone_from(&bindings.views);
        self.linear_external_textures.clone_from(&bindings.linear);
        self.external_texture_owners.clone_from(&bindings.owners);
        self.external_texture_lifetimes
            .clone_from(&bindings.lifetimes);
    }

    /// Register an externally-rendered texture for compositing in the current frame.
    pub fn register_external_texture(
        &mut self,
        id: crate::display_list::ExternalTextureId,
        view: wgpu::TextureView,
    ) {
        self.register_shared_external_texture(id, Arc::new(view));
    }

    /// Register a retained surface without transferring its cache ownership.
    pub fn register_shared_external_texture(
        &mut self,
        id: crate::display_list::ExternalTextureId,
        view: Arc<wgpu::TextureView>,
    ) {
        self.external_texture_owners.remove(&id);
        self.external_texture_lifetimes.remove(&id);
        self.linear_external_textures.remove(&id);
        self.external_textures.insert(id, view);
    }

    /// Register a texture whose sampling yields linear values (an `*Srgb`
    /// format), such as a 3D viewport; the image shader encodes it for the
    /// target's blend space. [`Self::register_external_texture`] textures
    /// already hold blend-space values.
    pub fn register_linear_external_texture(
        &mut self,
        id: crate::display_list::ExternalTextureId,
        view: wgpu::TextureView,
    ) {
        self.external_texture_owners.remove(&id);
        self.external_texture_lifetimes.remove(&id);
        self.external_textures.insert(id, Arc::new(view));
        self.linear_external_textures.insert(id);
    }

    /// Copy current-frame bindings when a host changes composition precision.
    pub fn copy_external_textures_to(&self, destination: &mut Self) {
        destination
            .external_textures
            .clone_from(&self.external_textures);
        destination
            .linear_external_textures
            .clone_from(&self.linear_external_textures);
        destination
            .external_texture_owners
            .clone_from(&self.external_texture_owners);
        destination
            .external_texture_lifetimes
            .clone_from(&self.external_texture_lifetimes);
    }

    /// Clear all registered external textures (call after frame).
    pub fn clear_external_textures(&mut self) {
        self.external_textures.clear();
        self.external_texture_owners.clear();
        self.external_texture_lifetimes.clear();
        self.linear_external_textures.clear();
        self.composite_direct.release_expired_textures();
        self.composite_offscreen.release_expired_textures();
    }
}

impl PassManager {
    /// Keep a frame lease for exactly as long as its native binding is retained.
    /// Callers clear registrations only after submitting the frame on the shared queue.
    pub fn register_linear_external_texture_with_owner(
        &mut self,
        id: crate::ExternalTextureId,
        view: wgpu::TextureView,
        owner: Arc<dyn std::any::Any + Send + Sync>,
    ) {
        self.register_linear_external_texture(id, view);
        self.external_texture_owners.insert(id, owner);
    }
}

impl PassManager {
    /// A native owner protects in-flight reads; a weak lifetime separately
    /// revokes future sampling when the owning canvas/realm is reset or retired.
    pub fn register_linear_external_texture_with_lifetime(
        &mut self,
        id: crate::ExternalTextureId,
        view: wgpu::TextureView,
        owner: Arc<dyn std::any::Any + Send + Sync>,
        lifetime: std::sync::Weak<()>,
    ) {
        self.register_linear_external_texture_with_owner(id, view, owner);
        self.external_texture_lifetimes.insert(id, lifetime);
    }
    pub fn remove_external_texture(&mut self, id: crate::ExternalTextureId) {
        self.external_textures.remove(&id);
        self.external_texture_owners.remove(&id);
        self.external_texture_lifetimes.remove(&id);
        self.linear_external_textures.remove(&id);
        self.composite_direct.release_expired_textures();
        self.composite_offscreen.release_expired_textures();
    }
    pub fn prune_expired_external_textures(&mut self) -> usize {
        let expired: Vec<_> = self
            .external_texture_lifetimes
            .iter()
            .filter(|(_, lifetime)| lifetime.strong_count() == 0)
            .map(|(id, _)| *id)
            .collect();
        for id in &expired {
            self.remove_external_texture(*id);
        }
        // A copied registration can have kept a view alive during an earlier
        // removal. Revisit cached bind groups even if this pass has no new IDs.
        self.composite_direct.release_expired_textures();
        self.composite_offscreen.release_expired_textures();
        expired.len()
    }
    /// Native lifecycle diagnostics; includes retained compositor/tile bindings.
    pub fn external_texture_count(&self) -> usize {
        self.external_textures.len()
    }
}
