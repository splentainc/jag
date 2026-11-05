use jag_draw::{BackdropBlurDraw, PathClip, Rect, Transform2D, snap_to_device};

use super::helpers::intersect_rect;
use super::{Canvas, ImageFitMode, RawImageDraw};

impl Canvas {
    /// Record a separately movable content layer. Keys must be stable across
    /// frames and unique within the committed scene (including iframe owners).
    pub fn push_scroll_layer(&mut self, key: impl Into<String>, translation: [f32; 2]) {
        self.painter.push_scroll_layer(key.into(), translation);
    }

    pub fn pop_scroll_layer(&mut self) {
        self.painter.pop_scroll_layer();
    }

    pub fn push_bound_scroll_layer(
        &mut self,
        key: impl Into<String>,
        input_key: impl Into<String>,
        value: [f32; 2],
        factor: [f32; 2],
    ) {
        self.painter
            .push_bound_scroll_layer(key.into(), input_key.into(), value, factor);
    }

    /// Queue an SVG to be rasterized and drawn at origin, scaled to fit within max_size.
    /// Captures the current transform from the painter's transform stack.
    /// Optional style parameter allows overriding fill, stroke, and stroke-width.
    pub fn draw_svg<P: Into<std::path::PathBuf>>(
        &mut self,
        path: P,
        origin: [f32; 2],
        max_size: [f32; 2],
        z: i32,
    ) {
        let path = path.into();
        // Skip draws entirely outside the active clip rect.
        if let Some(clip) = self.clip_rect_local() {
            let bounds = Rect {
                x: origin[0],
                y: origin[1],
                w: max_size[0],
                h: max_size[1],
            };
            if intersect_rect(bounds, clip).is_none() {
                return;
            }
        }
        if self.painter.has_active_effect() {
            // Keep the SVG command until the layer knows its device scale.
            // Painter::svg imports simple SVGs as un-antialiased path geometry,
            // unlike the rasterized side channel used outside effect groups.
            self.painter
                .svg_styled(path, origin, max_size, Default::default(), z);
            return;
        }
        let device_clip = self.clip_stack.last().copied().flatten();
        let rounded_clip = self.rounded_clip_stack.iter().rev().find_map(|clip| *clip);
        let transform = self.painter.current_transform();
        self.svg_draws.push((
            path,
            origin,
            max_size,
            None,
            z,
            self.current_opacity(),
            transform,
            device_clip,
            rounded_clip,
        ));
    }

    /// Queue an SVG with style overrides to be rasterized and drawn.
    pub fn draw_svg_styled<P: Into<std::path::PathBuf>>(
        &mut self,
        path: P,
        origin: [f32; 2],
        max_size: [f32; 2],
        style: jag_draw::SvgStyle,
        z: i32,
    ) {
        let path = path.into();
        // Skip draws entirely outside the active clip rect.
        if let Some(clip) = self.clip_rect_local() {
            let bounds = Rect {
                x: origin[0],
                y: origin[1],
                w: max_size[0],
                h: max_size[1],
            };
            if intersect_rect(bounds, clip).is_none() {
                return;
            }
        }
        if self.painter.has_active_effect() {
            self.painter.svg_styled(path, origin, max_size, style, z);
            return;
        }
        let device_clip = self.clip_stack.last().copied().flatten();
        let rounded_clip = self.rounded_clip_stack.iter().rev().find_map(|clip| *clip);
        let transform = self.painter.current_transform();
        self.svg_draws.push((
            path,
            origin,
            max_size,
            Some(style),
            z,
            self.current_opacity(),
            transform,
            device_clip,
            rounded_clip,
        ));
    }

    /// Queue a raster image (PNG/JPEG/GIF/WebP) to be drawn at origin with the given size.
    /// The fit parameter controls how the image is scaled within the size bounds.
    /// Captures the current transform from the painter's transform stack.
    pub fn draw_image<P: Into<std::path::PathBuf>>(
        &mut self,
        path: P,
        origin: [f32; 2],
        size: [f32; 2],
        fit: ImageFitMode,
        z: i32,
    ) {
        if self.painter.has_scroll_layer() {
            self.painter
                .extend_commands(&[jag_draw::Command::DrawImage {
                    path: path.into(),
                    origin,
                    size,
                    fit,
                    z,
                    transform: self.painter.current_transform(),
                }]);
            return;
        }
        // Skip draws entirely outside the active clip rect.
        if let Some(clip) = self.clip_rect_local() {
            let bounds = Rect {
                x: origin[0],
                y: origin[1],
                w: size[0],
                h: size[1],
            };
            if intersect_rect(bounds, clip).is_none() {
                return;
            }
        }
        let device_clip = self.clip_stack.last().copied().flatten();
        let rounded_clip = self.rounded_clip_stack.iter().rev().find_map(|clip| *clip);
        let transform = self.painter.current_transform();
        self.image_draws.push((
            path.into(),
            origin,
            size,
            fit,
            z,
            self.current_opacity(),
            transform,
            device_clip,
            rounded_clip,
        ));
    }

    /// Queue a CSS backdrop blur over the current framebuffer contents.
    ///
    /// This is a post-process draw that must be interleaved by z-index with
    /// transparent web paint, before the element's own translucent background.
    pub fn backdrop_blur_rect(&mut self, rect: Rect, radius: f32, z: i32) {
        if radius <= 0.0 || !radius.is_finite() {
            return;
        }
        self.backdrop_filter_rect(rect, vec![jag_draw::FilterEffect::Blur(radius)], z);
    }

    /// Queue an authored-order CSS backdrop-filter chain.
    pub fn backdrop_filter_rect(
        &mut self,
        rect: Rect,
        effects: Vec<jag_draw::FilterEffect>,
        z: i32,
    ) {
        if rect.w <= 0.0 || rect.h <= 0.0 || effects.is_empty() {
            return;
        }
        if let Some(clip) = self.clip_rect_local()
            && intersect_rect(rect, clip).is_none()
        {
            return;
        }
        let draw = BackdropBlurDraw {
            rect,
            effects,
            z,
            transform: self.painter.current_transform(),
            clip: self.clip_stack.last().copied().flatten(),
        };
        if self.painter.has_active_effect() {
            self.painter.backdrop_filter(draw);
        } else {
            self.backdrop_blur_draws.push(draw);
        }
    }

    /// Queue raw pixel data to be drawn at origin with the given size.
    /// Pixels should be in BGRA format (4 bytes per pixel) to match CEF native output.
    /// Captures the current transform from the painter's transform stack.
    pub fn draw_raw_image(
        &mut self,
        pixels: Vec<u8>,
        src_width: u32,
        src_height: u32,
        origin: [f32; 2],
        dst_size: [f32; 2],
        z: i32,
    ) {
        // Skip draws entirely outside the active clip rect.
        if let Some(clip) = self.clip_rect_local() {
            let bounds = Rect {
                x: origin[0],
                y: origin[1],
                w: dst_size[0],
                h: dst_size[1],
            };
            if intersect_rect(bounds, clip).is_none() {
                return;
            }
        }
        let device_clip = self.clip_stack.last().copied().flatten();
        let transform = self.painter.current_transform();
        self.raw_image_draws.push(RawImageDraw {
            pixels,
            src_width,
            src_height,
            origin,
            dst_size,
            z,
            transform,
            dirty_rects: Vec::new(), // Full frame update
            clip: device_clip,
        });
    }

    /// Queue raw pixel data with dirty rects for partial update.
    /// Pixels should be in BGRA format (4 bytes per pixel) to match CEF native output.
    /// Only the dirty rectangles will be uploaded to the GPU texture.
    pub fn draw_raw_image_with_dirty_rects(
        &mut self,
        pixels: Vec<u8>,
        src_width: u32,
        src_height: u32,
        origin: [f32; 2],
        dst_size: [f32; 2],
        z: i32,
        dirty_rects: Vec<(u32, u32, u32, u32)>,
    ) {
        // Skip draws entirely outside the active clip rect.
        if let Some(clip) = self.clip_rect_local() {
            let bounds = Rect {
                x: origin[0],
                y: origin[1],
                w: dst_size[0],
                h: dst_size[1],
            };
            if intersect_rect(bounds, clip).is_none() {
                return;
            }
        }
        let device_clip = self.clip_stack.last().copied().flatten();
        let transform = self.painter.current_transform();
        self.raw_image_draws.push(RawImageDraw {
            pixels,
            src_width,
            src_height,
            origin,
            dst_size,
            z,
            transform,
            dirty_rects,
            clip: device_clip,
        });
    }

    pub fn push_transform(&mut self, t: Transform2D) {
        self.painter.push_transform(t);
    }
    pub fn pop_transform(&mut self) {
        self.painter.pop_transform();
    }

    pub fn push_opacity(&mut self, opacity: f32) {
        let parent_opacity = self.current_opacity();
        self.opacity_stack
            .push(parent_opacity * opacity.clamp(0.0, 1.0));
        self.painter.push_opacity(opacity);
    }

    pub fn pop_opacity(&mut self) {
        if self.opacity_stack.len() > 1 {
            self.opacity_stack.pop();
        }
        self.painter.pop_opacity();
    }

    pub fn push_filter(&mut self, effect: jag_draw::FilterEffect) {
        self.painter.push_filter(effect);
    }

    pub fn pop_filter(&mut self) {
        self.painter.pop_filter();
    }

    /// Add a hit-only region (invisible, used for interaction detection)
    pub fn hit_region_rect(&mut self, id: u32, rect: Rect, z: i32) {
        self.painter.hit_region_rect(id, rect, z);
    }

    /// Return the current number of commands in the display list.
    pub fn command_count(&self) -> usize {
        self.painter.command_count()
    }

    /// Get a reference to the display list for hit testing
    pub fn display_list(&self) -> &jag_draw::DisplayList {
        self.painter.display_list()
    }

    /// Snap a rectangle defined in logical coordinates so that, after applying
    /// the current transform and DPI scale, its edges land on physical pixel
    /// boundaries. This assumes the current transform is an axis-aligned
    /// translate/scale (no rotation/skew); for more complex transforms the
    /// original rect is returned unchanged.
    pub fn snap_rect_logical_to_device(&self, rect: Rect) -> Rect {
        let sf = if self.dpi_scale.is_finite() && self.dpi_scale > 0.0 {
            self.dpi_scale
        } else {
            1.0
        };
        let t = self.painter.current_transform();
        let [a, b, c, d, e, f] = t.m;

        // Only handle simple translate/scale transforms. If there is rotation
        // or skew, fall back to the original rect to avoid warping.
        let is_simple = (b.abs() < 1e-4)
            && (c.abs() < 1e-4)
            && ((a - 1.0).abs() < 1e-4)
            && ((d - 1.0).abs() < 1e-4);
        if !is_simple {
            return rect;
        }

        let tx = e;
        let ty = f;

        // Snap both corners in device space, then bring them back to logical
        // by subtracting the translation and dividing by scale factor.
        let x0_device = snap_to_device(rect.x + tx, sf);
        let y0_device = snap_to_device(rect.y + ty, sf);
        let x1_device = snap_to_device(rect.x + rect.w + tx, sf);
        let y1_device = snap_to_device(rect.y + rect.h + ty, sf);

        let x0 = x0_device - tx;
        let y0 = y0_device - ty;
        let x1 = x1_device - tx;
        let y1 = y1_device - ty;

        Rect {
            x: x0,
            y: y0,
            w: (x1 - x0).max(0.0),
            h: (y1 - y0).max(0.0),
        }
    }

    /// Get the current effective clip rect in local (pre-transform) coordinates.
    ///
    /// Returns `None` when no clip is active or when the transform contains
    /// rotation/skew (where axis-aligned clipping would be incorrect).
    /// For axis-aligned transforms (translation + scale), the device-space clip
    /// is inverse-transformed back to the local coordinate space.
    pub(crate) fn clip_rect_local(&self) -> Option<Rect> {
        if self.painter.has_scroll_layer() {
            return None;
        }
        let clip_device = match self.clip_stack.last() {
            Some(Some(r)) => *r,
            _ => return None,
        };
        let t = self.painter.current_transform();
        let [a, b, c, d, e, f] = t.m;

        // Only handle axis-aligned transforms (no rotation/skew).
        if b.abs() > 1e-4 || c.abs() > 1e-4 {
            return None;
        }

        let sf = if self.dpi_scale.is_finite() && self.dpi_scale > 0.0 {
            self.dpi_scale
        } else {
            1.0
        };

        let sx = a * sf;
        let sy = d * sf;
        if sx.abs() < 1e-6 || sy.abs() < 1e-6 {
            return None;
        }

        // Inverse-transform: device = (a * local + e) * sf
        //                  → local = device / (a * sf) - e / a
        let local_x0 = clip_device.x / sx - e / a;
        let local_y0 = clip_device.y / sy - f / d;
        let local_x1 = (clip_device.x + clip_device.w) / sx - e / a;
        let local_y1 = (clip_device.y + clip_device.h) / sy - f / d;

        // Handle negative scales (flips).
        let (lx0, lx1) = if local_x0 < local_x1 {
            (local_x0, local_x1)
        } else {
            (local_x1, local_x0)
        };
        let (ly0, ly1) = if local_y0 < local_y1 {
            (local_y0, local_y1)
        } else {
            (local_y1, local_y0)
        };

        Some(Rect {
            x: lx0,
            y: ly0,
            w: lx1 - lx0,
            h: ly1 - ly0,
        })
    }

    /// The active clip as a [`PathClip`] in the path's local space: the rect from
    /// [`Self::clip_rect_local`] plus the current rounded-clip corner radii (if
    /// any), converted device->local by the transform's scale. `None` when there
    /// is no clip (or a rotated/skewed transform `clip_rect_local` can't invert).
    /// Lets filled/stroked SVG paths honor `border-radius` `overflow:hidden`
    /// corners, not just the bounding rect.
    pub(crate) fn rounded_clip_local(&self) -> Option<PathClip> {
        if self.painter.has_scroll_layer() {
            return None;
        }
        let rect = self.clip_rect_local()?;
        // The nearest active rounded clip. Descendants (e.g. an SVG element's own
        // default `overflow` rect clip) push `None` entries on top of an ancestor
        // rounded `overflow:hidden` box, so checking only the top would miss it.
        let Some(rc) = self
            .rounded_clip_stack
            .iter()
            .rev()
            .find_map(|e| e.as_ref())
        else {
            return Some(PathClip::rect(rect));
        };
        // Convert device-space corner radii to local space by the transform's
        // x-scale (dpi * any CSS scale). These clips occur under uniform scale in
        // practice; a non-uniform scale would make corners slightly elliptical
        // (still rounded, not square). `rect` is the intersected clip bounds; when
        // a tighter inner rect clip cuts inside the rounded box the corners are
        // applied to that tighter rect, which is exact in the common case (inner
        // rect == the rounded box, as for an SVG filling its container).
        let t = self.painter.current_transform();
        let sf = if self.dpi_scale.is_finite() && self.dpi_scale > 0.0 {
            self.dpi_scale
        } else {
            1.0
        };
        let sx = (t.m[0] * sf).abs();
        if sx < 1e-6 {
            return Some(PathClip::rect(rect));
        }
        let radii = [
            rc.radii[0] / sx,
            rc.radii[1] / sx,
            rc.radii[2] / sx,
            rc.radii[3] / sx,
        ];
        Some(PathClip { rect, radii })
    }
}
