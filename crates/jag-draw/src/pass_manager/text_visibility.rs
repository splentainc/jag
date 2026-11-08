//! Cull only the final bitmap quad, in the same device coordinates as the
//! text vertex shader. Cached glyphs remain available for subsequent scrolling.
pub(super) fn visible(
    origin: [f32; 2],
    mask: [u32; 2],
    alpha: f32,
    scroll: [f32; 2],
    inv_logical: f32,
    target: [u32; 2],
) -> bool {
    if alpha <= 0. || mask.contains(&0) {
        return false;
    }
    let x = (origin[0] + scroll[0]) / inv_logical;
    let y = (origin[1] + scroll[1]) / inv_logical;
    // Preserve the renderer's handling of non-finite author inputs.
    if !x.is_finite() || !y.is_finite() {
        return true;
    }
    x < target[0] as f32
        && y < target[1] as f32
        && x + mask[0] as f32 > 0.
        && y + mask[1] as f32 > 0.
}

#[cfg(test)]
mod tests {
    use super::visible;
    #[test]
    fn retina_scroll_and_effect_layer_offsets_preserve_intersecting_glyphs() {
        let test = |origin, scroll| visible(origin, [20, 20], 1., scroll, 0.5, [100, 100]);
        assert!(test([-5., -5.], [0., 0.]));
        assert!(!test([50., 0.], [0., 0.]));
        assert!(!test([0., 1000.], [0., 0.]));
        assert!(test([0., 1000.], [0., -995.]));
        assert!(test([1000., 1000.], [-995., -995.]));
        assert!(!visible([0., 0.], [20, 20], 0., [0., 0.], 1., [100, 100]));
    }
}
