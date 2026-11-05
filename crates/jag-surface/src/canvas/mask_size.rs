//! Raster resolution is independent of the logical mask tile used by the GPU.
//! Bound allocations before rasterization, including very large CSS geometry.
use super::GeneratedMaskTexture;
use std::borrow::Cow;

// At most 64 MiB per RGBA mask, independently of a device's dimension limit.
const MAX_PIXELS: u64 = 16 * 1024 * 1024;

pub(super) fn raster_size(size: [f32; 2], scale: f32, limit: u32) -> Option<[u32; 2]> {
    if limit == 0
        || !scale.is_finite()
        || scale <= 0.
        || size.iter().any(|v| !v.is_finite() || *v <= 0.)
    {
        return None;
    }
    let mut dimensions =
        size.map(|v| ((v as f64 * scale as f64).ceil().max(1.).min(limit as f64)) as u32);
    let area = dimensions[0] as u64 * dimensions[1] as u64;
    if area > MAX_PIXELS {
        let shrink = (MAX_PIXELS as f64 / area as f64).sqrt();
        dimensions = dimensions.map(|v| (v as f64 * shrink).floor().max(1.) as u32);
    }
    Some(dimensions)
}

impl GeneratedMaskTexture {
    // A completed canvas may be submitted to a different surface/device. Keep
    // its immutable pixels intact and adapt only the receiving device's upload.
    pub(crate) fn upload_pixels(&self, limit: u32) -> ([u32; 2], Cow<'_, [u8]>) {
        let size = [self.width, self.height];
        if size.iter().all(|v| *v <= limit) {
            return (size, Cow::Borrowed(&self.pixels));
        }
        let target = [self.width.min(limit), self.height.min(limit)];
        let source = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(
            self.width,
            self.height,
            &*self.pixels,
        )
        .expect("generated mask dimensions match RGBA storage");
        let pixels = image::imageops::resize(
            &source,
            target[0],
            target[1],
            image::imageops::FilterType::Triangle,
        )
        .into_raw();
        (target, Cow::Owned(pixels))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_both_axes_without_reducing_an_in_range_axis() {
        assert_eq!(raster_size([4816., 24.], 2., 8192), Some([8192, 48]));
        assert_eq!(raster_size([24., 4816.], 2., 8192), Some([48, 8192]));
        assert_eq!(raster_size([100., 30.], 2., 8192), Some([200, 60]));
        assert_eq!(raster_size([0.1, 0.1], 1., 8192), Some([1, 1]));
        assert_eq!(raster_size([9632., 24.], 1., 4096), Some([4096, 24]));
    }

    #[test]
    fn huge_finite_geometry_cannot_overflow_or_allocate_unbounded_pixels() {
        for size in [[f32::MAX; 2], [1e9, 0.1], [1e6, 1e5], [0.1, 1e9]] {
            let dimensions = raster_size(size, f32::MAX, 16384).unwrap();
            assert!(dimensions.iter().all(|v| *v > 0 && *v <= 16384));
            assert!(dimensions[0] as u64 * dimensions[1] as u64 <= MAX_PIXELS);
        }
        assert_eq!(raster_size([f32::MAX; 2], f32::MAX, 8192), Some([4096; 2]));
    }

    #[test]
    fn invalid_geometry_does_not_produce_a_raster() {
        for value in [0., -1., f32::INFINITY, f32::NAN] {
            assert!(raster_size([value, 10.], 1., 8192).is_none());
            assert!(raster_size([10., value], 1., 8192).is_none());
            assert!(raster_size([10.; 2], value, 8192).is_none());
        }
        assert!(raster_size([10.; 2], 1., 0).is_none());
    }
}
