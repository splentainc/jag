//! Bounded raster reuse. Translation belongs to the effect placement, not its
//! pixels. Entries own immutable pixels so an in-flight canvas survives eviction.
use jag_draw::{Brush, Rect, RoundedRect};
use std::{cell::RefCell, collections::VecDeque, sync::Arc};

const BYTE_LIMIT: usize = 256 * 1024 * 1024;
const ENTRY_LIMIT: usize = 128;
type Key = Vec<u32>;
#[derive(Default)]
struct Cache {
    entries: VecDeque<(Key, Arc<[u8]>)>,
    bytes: usize,
}
thread_local! { static CACHE: RefCell<Cache> = RefCell::new(Cache::default()); }

pub(super) fn pixels(key: Key, raster: impl FnOnce() -> Vec<u8>) -> Arc<[u8]> {
    CACHE.with(|slot| {
        let mut cache = slot.borrow_mut();
        if let Some(index) = cache.entries.iter().position(|(old, _)| *old == key) {
            let entry = cache.entries.remove(index).unwrap();
            let result = entry.1.clone();
            cache.entries.push_back(entry);
            return result;
        }
        let pixels: Arc<[u8]> = raster().into();
        if std::env::var_os("JAG_PROFILE_MASKS").is_some() {
            eprintln!(
                "[mask-raster] kind={} size={}x{} bytes={} resident={} entries={}",
                key[0],
                key[1],
                key[2],
                pixels.len(),
                cache.bytes,
                cache.entries.len()
            );
        }
        if pixels.len() <= BYTE_LIMIT {
            while cache.bytes + pixels.len() > BYTE_LIMIT || cache.entries.len() >= ENTRY_LIMIT {
                cache.bytes -= cache.entries.pop_front().unwrap().1.len();
            }
            cache.bytes += pixels.len();
            cache.entries.push_back((key, pixels.clone()));
        }
        pixels
    })
}

fn rect(key: &mut Key, r: Rect) {
    key.extend([r.x, r.y, r.w, r.h].map(f32::to_bits));
}

pub(super) fn gradient(
    paint: Rect,
    tile: Rect,
    step: [f32; 2],
    repeat: [bool; 2],
    brush: &Brush,
    linear: [f32; 6],
    size: [u32; 2],
) -> Key {
    let mut key = vec![0, size[0], size[1]];
    rect(&mut key, paint);
    rect(&mut key, tile);
    key.extend(step.map(f32::to_bits));
    key.extend(repeat.map(u32::from));
    key.extend(linear[..4].iter().map(|x| x.to_bits()));
    let stops = match brush {
        Brush::LinearGradient { start, end, stops } => {
            key.push(0);
            key.extend(start.iter().chain(end).map(|x| x.to_bits()));
            stops
        }
        Brush::RadialGradient {
            center,
            radius,
            stops,
        } => {
            key.push(1);
            key.extend(center.iter().chain([radius]).map(|x| x.to_bits()));
            stops
        }
        Brush::ConicGradient {
            center,
            start_angle,
            stops,
        } => {
            key.push(2);
            key.extend(center.iter().chain([start_angle]).map(|x| x.to_bits()));
            stops
        }
        Brush::Solid(_) => unreachable!("generated mask validated as gradient"),
    };
    for (offset, color) in stops {
        key.push(offset.to_bits());
        // Rasterization interpolates these exact sRGBA bytes.
        key.push(u32::from_le_bytes(color.to_srgba_u8()));
    }
    key
}

pub(super) fn shadow(bounds: Rect, owner: RoundedRect, linear: [f32; 6], size: [u32; 2]) -> Key {
    let mut key = vec![1, size[0], size[1]];
    rect(&mut key, bounds);
    rect(&mut key, owner.rect);
    key.extend(
        [
            owner.radii.tl,
            owner.radii.tr,
            owner.radii.br,
            owner.radii.bl,
        ]
        .map(f32::to_bits),
    );
    key.extend(linear[..4].iter().map(|x| x.to_bits()));
    key
}

// Largest singular value covers shear as well as scale. Quantized upward to
// avoid reallocating for floating-point noise in a rotation's unit vectors.
pub(super) fn raster_scale([a, b, c, d, _, _]: [f32; 6], dpi: f32) -> f32 {
    let sum = a * a + b * b + c * c + d * d;
    let determinant = a * d - b * c;
    let max_scale =
        ((sum + (sum * sum - 4. * determinant * determinant).max(0.).sqrt()) * 0.5).sqrt();
    ((max_scale.max(1.) * dpi.max(1.) * 16.).ceil() / 16.).max(1.)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identical_rasters_share_storage_and_eviction_keeps_inflight_pixels_alive() {
        CACHE.with(|c| *c.borrow_mut() = Cache::default());
        let key = vec![0, 1, 1, 1000];
        let original = pixels(key.clone(), || vec![1, 2, 3, 4]);
        let shared = pixels(key.clone(), || panic!("cache hit must not rasterize"));
        assert!(Arc::ptr_eq(&original, &shared));
        for n in 0..ENTRY_LIMIT as u32 + 1 {
            pixels(vec![0, 1, 1, n], || vec![5; 4]);
        }
        assert_eq!(&*original, &[1, 2, 3, 4]);
        let replaced = pixels(key, || vec![1, 2, 3, 4]);
        assert!(!Arc::ptr_eq(&original, &replaced));
        CACHE.with(|c| {
            let c = c.borrow();
            assert!(c.entries.len() <= ENTRY_LIMIT);
            assert!(c.bytes <= BYTE_LIMIT);
        });
    }
    #[test]
    fn keys_include_pixels_geometry_stops_and_resolution() {
        let rect = Rect {
            x: 0.,
            y: 0.,
            w: 10.,
            h: 20.,
        };
        let color = jag_draw::ColorLinPremul::from_srgba_u8([255; 4]);
        let brush = Brush::LinearGradient {
            start: [0., 0.],
            end: [10., 20.],
            stops: vec![(0., color), (1., color)],
        };
        let identity = [1., 0., 0., 1., 0., 0.];
        let key = gradient(
            rect,
            rect,
            [10., 20.],
            [false; 2],
            &brush,
            identity,
            [10, 20],
        );
        assert_ne!(
            key,
            gradient(
                rect,
                rect,
                [10., 20.],
                [false; 2],
                &brush,
                identity,
                [20, 40]
            )
        );
        assert_ne!(
            key,
            gradient(
                rect,
                rect,
                [10., 20.],
                [true; 2],
                &brush,
                identity,
                [10, 20]
            )
        );
        let mut changed = brush.clone();
        if let Brush::LinearGradient { stops, .. } = &mut changed {
            stops[0].0 = 0.25;
        }
        assert_ne!(
            key,
            gradient(
                rect,
                rect,
                [10., 20.],
                [false; 2],
                &changed,
                identity,
                [10, 20]
            )
        );
        assert_eq!(raster_scale([0., 1., -1., 0., 200., 300.], 2.), 2.);
        assert!(raster_scale([1., 0., 2., 1., 0., 0.], 1.) > 2.);
    }
}
