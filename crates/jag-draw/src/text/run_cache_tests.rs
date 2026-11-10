use super::*;
use std::sync::Arc;

fn key(id: u64) -> GlyphRunKey {
    GlyphRunKey {
        source: None,
        features: Vec::new(),
        spacing: None,
        rtl: None,
        text_hash: id,
        size_bits: 0,
        logical_size_bits: 0,
        weight_bits: 0,
        style_bits: 0,
        family_hash: 0,
        provider_id: 0,
    }
}

fn masks(bytes: usize) -> Vec<RasterizedGlyph> {
    vec![RasterizedGlyph {
        source: None,
        offset: [0., 0.],
        mask: GlyphMask::Color(ColorMask {
            width: 2048,
            height: (bytes / (2048 * 4)) as u32,
            data: vec![255; bytes],
        }),
    }]
}

#[test]
fn run_cache_bounds_mask_bytes_not_just_entry_count() {
    let cache = GlyphRunCache::new(2048);
    let first = cache.insert(key(1), masks(8 * 1024 * 1024));
    let weak = Arc::downgrade(&first);
    drop(first);
    cache.insert(key(2), masks(8 * 1024 * 1024));
    cache.insert(key(3), masks(8 * 1024 * 1024));
    assert!(
        cache.get(&key(1)).is_none(),
        "old mask bytes exceed the 16 MiB cache budget"
    );
    assert!(
        weak.upgrade().is_none(),
        "eviction must release cache ownership"
    );
    assert!(cache.get(&key(3)).is_some());
}

#[test]
fn run_cache_evicts_least_recent_use_without_a_whole_cache_clear() {
    let cache = GlyphRunCache::new(2);
    let hot = cache.insert(key(1), Vec::new());
    cache.insert(key(2), Vec::new());
    assert!(Arc::ptr_eq(&hot, &cache.get(&key(1)).unwrap()));
    cache.insert(key(3), Vec::new());
    assert!(cache.get(&key(2)).is_none());
    assert!(Arc::ptr_eq(&hot, &cache.get(&key(1)).unwrap()));
    assert!(cache.get(&key(3)).is_some());
}

#[test]
fn run_cache_large_answer_renders_without_becoming_cache_resident() {
    let cache = GlyphRunCache::new(2048);
    let answer = cache.insert(key(1), masks(20 * 1024 * 1024));
    assert_eq!(answer.len(), 1);
    assert!(cache.get(&key(1)).is_none());
}

#[test]
fn run_cache_eviction_and_clear_preserve_inflight_glyph_leases() {
    let cache = GlyphRunCache::new(1);
    let leased = cache.insert(key(1), masks(8 * 1024));
    cache.insert(key(2), Vec::new());
    assert!(cache.get(&key(1)).is_none());
    assert_eq!(leased[0].mask.width(), 2048);
    let GlyphMask::Color(mask) = &leased[0].mask else {
        panic!("color fixture")
    };
    assert!(mask.data.iter().all(|byte| *byte == 255));
    let second = cache.get(&key(2)).unwrap();
    assert!(Arc::ptr_eq(&second, &cache.insert(key(2), masks(8 * 1024))));
    cache.clear();
    assert!(cache.get(&key(2)).is_none());
    assert!(second.is_empty());
    assert_eq!(leased[0].mask.height(), 1);
}

#[test]
fn run_cache_stays_bounded_across_many_different_document_working_sets() {
    let cache = GlyphRunCache::new(2048);
    for navigation in 0..64 {
        for fragment in 0..64 {
            cache.insert(key(navigation * 64 + fragment), masks(8 * 1024));
        }
        let state = cache.state.lock().unwrap();
        assert!(state.bytes <= GLYPH_RUN_CACHE_BYTES);
        assert!(state.map.len() <= 2048);
    }
    assert!(cache.get(&key(0)).is_none());
    assert!(cache.get(&key(4095)).is_some());
}
