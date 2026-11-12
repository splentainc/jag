#[test]
fn backdrop_filter_snapshots_before_later_transparent_content() {
    let instance = wgpu::Instance::default();
    let Ok(adapter) =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        return;
    };
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let mut surface = JagSurface::new(
        Arc::new(device),
        Arc::new(queue),
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    surface.set_frame_cache_enabled(false);
    let mut canvas = surface.begin_frame(24, 16);
    canvas.clear(ColorLinPremul::default());
    canvas.fill_rect(
        0.0,
        0.0,
        24.0,
        16.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([255, 0, 0, 255])),
        1,
    );
    canvas.backdrop_filter_rect(
        jag_draw::Rect {
            x: 4.0,
            y: 2.0,
            w: 16.0,
            h: 12.0,
        },
        vec![
            FilterEffect::ColorMatrix(ColorMatrix {
                rows: [
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
                bias: [0.0; 4],
            }),
            FilterEffect::ColorMatrix(ColorMatrix {
                rows: [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
                bias: [0.25, 0.0, 0.0, 0.0],
            }),
        ],
        2,
    );
    canvas.fill_rect(
        12.0,
        2.0,
        8.0,
        12.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([0, 255, 0, 254])),
        3,
    );

    let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    let filtered = pixel(&pixels, width, 8, 8);
    let later = pixel(&pixels, width, 16, 8);
    assert!(filtered[2] > 250, "channel swap missing: {filtered:?}");
    assert!(
        (60..=68).contains(&filtered[0]),
        "chain order wrong: {filtered:?}"
    );
    assert!(later[1] > 250, "later content was filtered: {later:?}");
}

fn assert_backdrop_filter_captures_ancestor_framebuffer(filter_ancestor: bool) {
    let instance = wgpu::Instance::default();
    let Ok(adapter) =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        return;
    };
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let mut surface = JagSurface::new(
        Arc::new(device),
        Arc::new(queue),
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    surface.set_frame_cache_enabled(false);
    let mut canvas = surface.begin_frame(16, 8);
    canvas.clear(ColorLinPremul::default());
    canvas.fill_rect(
        0.0,
        0.0,
        16.0,
        8.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([0, 0, 255, 255])),
        0,
    );
    if filter_ancestor {
        canvas.push_filter(FilterEffect::ColorMatrix(ColorMatrix {
            rows: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            bias: [0.0; 4],
        }));
    } else {
        canvas.push_opacity(1.0);
    }
    canvas.fill_rect(
        0.0,
        0.0,
        8.0,
        8.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([255, 0, 0, 255])),
        1,
    );
    canvas.backdrop_filter_rect(
        jag_draw::Rect {
            x: 0.0,
            y: 0.0,
            w: 16.0,
            h: 8.0,
        },
        vec![FilterEffect::ColorMatrix(ColorMatrix {
            rows: [
                [0.0, 0.0, 1.0, 0.0],
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            bias: [0.0; 4],
        })],
        2,
    );
    if filter_ancestor {
        canvas.pop_filter();
    } else {
        canvas.pop_opacity();
    }

    let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    let captured = pixel(&pixels, width, 4, 4);
    let outside_group_capture = pixel(&pixels, width, 12, 4);
    assert!(
        captured[1] > 250 && captured[0] < 5 && captured[2] < 5,
        "group-local red backdrop was not transformed: {captured:?}"
    );
    assert!(
        outside_group_capture[2] > 250 && outside_group_capture[0] < 5,
        "nested backdrop leaked into the root framebuffer: {outside_group_capture:?}"
    );
}

#[test]
fn backdrop_filter_inside_opacity_captures_the_group_framebuffer() {
    assert_backdrop_filter_captures_ancestor_framebuffer(false);
}

#[test]
fn backdrop_filter_inside_filter_captures_the_group_framebuffer() {
    assert_backdrop_filter_captures_ancestor_framebuffer(true);
}

#[test]
fn drop_shadow_keeps_source_rows_and_casts_downward() {
    let Some(mut surface) = test_surface() else {
        return;
    };
    let mut canvas = surface.begin_frame(16, 32);
    canvas.clear(ColorLinPremul::default());
    canvas.push_filter(FilterEffect::DropShadow(DropShadow {
        offset: [0.0, 8.0],
        blur_radius: 0.0,
        color: SrgbColor::rgba(0, 0, 255, 255),
    }));
    canvas.fill_rect(
        4.0,
        2.0,
        8.0,
        6.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([255, 0, 0, 255])),
        1,
    );
    canvas.pop_filter();

    let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    let source = pixel(&pixels, width, 8, 5);
    let shadow = pixel(&pixels, width, 8, 13);
    let below = pixel(&pixels, width, 8, 24);
    assert!(source[0] > 250 && source[2] < 5, "source moved: {source:?}");
    assert!(
        shadow[2] > 240 && shadow[3] > 240,
        "shadow should sit 8px below the source: {shadow:?}"
    );
    assert!(below[3] < 5, "nothing belongs in the lower half: {below:?}");
}
