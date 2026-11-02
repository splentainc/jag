#[test]
fn resolved_texture_mask_applies_alpha_and_luminance_coverage() {
    let instance = wgpu::Instance::default();
    let Ok(adapter) =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        return;
    };
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let device = Arc::new(device);
    let queue = Arc::new(queue);
    let mask_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("resolved-mask-test"),
        size: wgpu::Extent3d {
            width: 4,
            height: 8,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mask_pixels = [255, 0, 0, 128].repeat(32);
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &mask_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &mask_pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(16),
            rows_per_image: Some(8),
        },
        wgpu::Extent3d {
            width: 4,
            height: 8,
            depth_or_array_layers: 1,
        },
    );
    let url_texture = Arc::new(device.create_texture(&wgpu::TextureDescriptor {
        label: Some("url-mask-test"),
        size: wgpu::Extent3d {
            width: 2,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    }));
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &url_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &[0, 0, 0, 255, 0, 0, 0, 0],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(8),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 2,
            height: 1,
            depth_or_array_layers: 1,
        },
    );

    let mut surface = JagSurface::new(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb);
    surface.set_frame_cache_enabled(false);
    let mask_id = ExternalTextureId(42);
    surface
        .pass_manager()
        .register_external_texture(mask_id, mask_texture.create_view(&Default::default()));
    surface.pass_manager().store_loaded_image(
        std::path::Path::new("url-mask-test"),
        url_texture,
        2,
        1,
    );
    let mut canvas = surface.begin_frame(44, 8);
    canvas.clear(ColorLinPremul::default());
    for (x, mode, z) in [(0.0, MaskMode::Alpha, 1), (4.0, MaskMode::Luminance, 2)] {
        canvas.push_filter(FilterEffect::Mask(MaskEffect {
            texture_id: mask_id,
            mode,
            rect: jag_draw::Rect {
                x,
                y: 0.0,
                w: 4.0,
                h: 8.0,
            },
            mapping: None,
        }));
        canvas.fill_rect(
            x,
            0.0,
            4.0,
            8.0,
            Brush::Solid(ColorLinPremul::from_srgba_u8([255; 4])),
            z,
        );
        canvas.pop_filter();
    }
    canvas.push_filter(FilterEffect::Mask(MaskEffect {
        texture_id: mask_id,
        mode: MaskMode::Alpha,
        rect: jag_draw::Rect {
            x: 8.0,
            y: 0.0,
            w: 2.0,
            h: 8.0,
        },
        mapping: None,
    }));
    canvas.fill_rect(
        8.0,
        0.0,
        4.0,
        8.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([255; 4])),
        3,
    );
    canvas.pop_filter();
    assert!(canvas.push_url_mask(
        "url-mask-test",
        jag_draw::Rect {
            x: 32.0,
            y: 0.0,
            w: 8.0,
            h: 8.0,
        },
        jag_draw::Rect {
            x: 32.0,
            y: 0.0,
            w: 2.0,
            h: 8.0,
        },
        [4.0, 8.0],
        [true, false],
        MaskMode::Alpha,
    ));
    canvas.fill_rect(
        32.0,
        0.0,
        8.0,
        8.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([255; 4])),
        8,
    );
    canvas.pop_filter();
    assert!(canvas.push_url_mask(
        "missing-url-mask-test",
        jag_draw::Rect {
            x: 40.0,
            y: 0.0,
            w: 4.0,
            h: 8.0,
        },
        jag_draw::Rect {
            x: 40.0,
            y: 0.0,
            w: 4.0,
            h: 8.0,
        },
        [4.0, 8.0],
        [false; 2],
        MaskMode::Alpha,
    ));
    canvas.fill_rect(
        40.0,
        0.0,
        4.0,
        8.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([255; 4])),
        9,
    );
    canvas.pop_filter();
    assert!(canvas.push_generated_mask_pattern(
        jag_draw::Rect {
            x: 24.0,
            y: 0.0,
            w: 8.0,
            h: 8.0,
        },
        jag_draw::Rect {
            x: 24.0,
            y: 0.0,
            w: 2.0,
            h: 8.0,
        },
        [4.0, 8.0],
        [true, false],
        &Brush::LinearGradient {
            start: [24.0, 0.0],
            end: [26.0, 0.0],
            stops: vec![
                (0.0, ColorLinPremul::from_srgba_u8([0, 0, 0, 255])),
                (1.0, ColorLinPremul::from_srgba_u8([0, 0, 0, 255])),
            ],
        },
        MaskMode::Alpha,
    ));
    canvas.fill_rect(
        24.0,
        0.0,
        8.0,
        8.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([255; 4])),
        7,
    );
    canvas.pop_filter();
    for (rect, brush, z) in [
        (
            jag_draw::Rect {
                x: 16.0,
                y: 0.0,
                w: 4.0,
                h: 8.0,
            },
            Brush::RadialGradient {
                center: [18.0, 4.0],
                radius: 2.0,
                stops: vec![
                    (0.0, ColorLinPremul::from_srgba_u8([0, 0, 0, 0])),
                    (1.0, ColorLinPremul::from_srgba_u8([0, 0, 0, 255])),
                ],
            },
            5,
        ),
        (
            jag_draw::Rect {
                x: 20.0,
                y: 0.0,
                w: 4.0,
                h: 8.0,
            },
            Brush::ConicGradient {
                center: [22.0, 4.0],
                start_angle: 0.0,
                stops: vec![
                    (0.0, ColorLinPremul::from_srgba_u8([0, 0, 0, 0])),
                    (0.5, ColorLinPremul::from_srgba_u8([0, 0, 0, 255])),
                    (1.0, ColorLinPremul::from_srgba_u8([0, 0, 0, 0])),
                ],
            },
            6,
        ),
    ] {
        assert!(canvas.push_generated_mask(rect, &brush, MaskMode::Alpha));
        canvas.fill_rect(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            Brush::Solid(ColorLinPremul::from_srgba_u8([255; 4])),
            z,
        );
        canvas.pop_filter();
    }
    assert!(canvas.push_generated_mask(
        jag_draw::Rect {
            x: 12.0,
            y: 0.0,
            w: 4.0,
            h: 8.0,
        },
        &Brush::LinearGradient {
            start: [12.0, 4.0],
            end: [16.0, 4.0],
            stops: vec![
                (0.0, ColorLinPremul::from_srgba_u8([0, 0, 0, 0])),
                (1.0, ColorLinPremul::from_srgba_u8([0, 0, 0, 255])),
            ],
        },
        MaskMode::Alpha,
    ));
    canvas.fill_rect(
        12.0,
        0.0,
        4.0,
        8.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([255; 4])),
        4,
    );
    canvas.pop_filter();

    let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    assert_alpha_near(pixel(&pixels, width, 2, 4), 128);
    assert_alpha_near(pixel(&pixels, width, 6, 4), 27);
    assert_alpha_near(pixel(&pixels, width, 9, 4), 128);
    assert_alpha_near(pixel(&pixels, width, 11, 4), 0);
    assert!(pixel(&pixels, width, 12, 4)[3] < 64);
    assert!(pixel(&pixels, width, 15, 4)[3] > 190);
    assert!(pixel(&pixels, width, 18, 4)[3] < 100);
    assert!(pixel(&pixels, width, 16, 4)[3] > 160);
    let conic_top = pixel(&pixels, width, 22, 1)[3];
    let conic_bottom = pixel(&pixels, width, 22, 6)[3];
    assert!(
        conic_top < 80 && conic_bottom > 160,
        "conic alpha top/bottom: {conic_top}/{conic_bottom}"
    );
    assert!(pixel(&pixels, width, 24, 4)[3] > 240);
    assert!(pixel(&pixels, width, 27, 4)[3] < 10);
    assert!(pixel(&pixels, width, 28, 4)[3] > 240);
    assert!(pixel(&pixels, width, 32, 4)[3] > 150);
    assert!(pixel(&pixels, width, 33, 4)[3] < 100);
    assert!(pixel(&pixels, width, 35, 4)[3] < 10);
    assert!(pixel(&pixels, width, 36, 4)[3] > 150);
    assert!(pixel(&pixels, width, 42, 4)[3] < 10);
}
