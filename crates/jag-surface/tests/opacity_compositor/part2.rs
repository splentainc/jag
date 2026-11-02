#[test]
fn transformed_clip_bounds_nested_effect_layers_in_world_space() {
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
    let mut canvas = surface.begin_frame(32, 12);
    canvas.clear(ColorLinPremul::default());
    canvas.push_opacity(0.5);
    canvas.push_transform(jag_draw::Transform2D::translate(8.0, 2.0));
    canvas.push_clip_rect(jag_draw::Rect {
        x: 0.0,
        y: 0.0,
        w: 8.0,
        h: 8.0,
    });
    canvas.push_opacity(0.5);
    canvas.fill_rect(
        0.0,
        0.0,
        16.0,
        8.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([255; 4])),
        1,
    );
    canvas.pop_opacity();
    canvas.pop_clip();
    canvas.pop_transform();
    canvas.pop_opacity();

    let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    assert_alpha_near(pixel(&pixels, width, 7, 5), 0);
    assert_alpha_near(pixel(&pixels, width, 10, 5), 64);
    assert_alpha_near(pixel(&pixels, width, 17, 5), 0);
}

#[test]
fn blur_filter_spreads_surface_alpha_beyond_descendant_ink() {
    let instance = wgpu::Instance::default();
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    })) else {
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
    let mut canvas = surface.begin_frame(32, 16);
    canvas.clear(ColorLinPremul::default());
    canvas.push_filter(FilterEffect::Blur(1.5));
    canvas.fill_rect(
        10.0,
        4.0,
        12.0,
        8.0,
        Brush::Solid(ColorLinPremul {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        }),
        1,
    );
    canvas.pop_filter();

    let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    let far = pixel(&pixels, width, 1, 8)[3];
    let halo = pixel(&pixels, width, 8, 8)[3];
    let center = pixel(&pixels, width, 16, 8)[3];
    assert!(far <= 2, "far pixel should remain transparent, got {far}");
    assert!(
        halo > 2,
        "blur should spread alpha beyond source ink, got {halo}"
    );
    assert!(center > halo, "center {center} should exceed halo {halo}");
}

#[test]
fn color_matrix_filter_uses_srgb_and_preserves_alpha() {
    let instance = wgpu::Instance::default();
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    })) else {
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
    let mut canvas = surface.begin_frame(12, 12);
    canvas.clear(ColorLinPremul::default());
    canvas.push_filter(FilterEffect::ColorMatrix(ColorMatrix {
        rows: [
            [0.5, 0.0, 0.0, 0.0],
            [0.0, 0.5, 0.0, 0.0],
            [0.0, 0.0, 0.5, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        bias: [0.0; 4],
    }));
    canvas.fill_rect(
        2.0,
        2.0,
        8.0,
        8.0,
        Brush::Solid(ColorLinPremul {
            r: 0.25,
            g: 0.25,
            b: 0.25,
            a: 1.0,
        }),
        1,
    );
    canvas.pop_filter();

    let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    let transformed = pixel(&pixels, width, 6, 6);
    assert!(
        transformed[..3]
            .iter()
            .all(|channel| (65..=72).contains(channel)),
        "sRGB brightness should produce channels near 69: {transformed:?}"
    );
    assert!(
        transformed[3] > 250,
        "alpha should be preserved: {transformed:?}"
    );
}

#[test]
fn drop_shadow_keeps_source_above_shifted_tinted_alpha() {
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
    let mut canvas = surface.begin_frame(32, 16);
    canvas.clear(ColorLinPremul::default());
    canvas.push_filter(FilterEffect::DropShadow(DropShadow {
        offset: [8.0, 0.0],
        blur_radius: 0.0,
        color: SrgbColor::rgba(0, 0, 255, 255),
    }));
    canvas.fill_rect(
        4.0,
        4.0,
        6.0,
        8.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([255, 0, 0, 255])),
        1,
    );
    canvas.pop_filter();

    let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    let source = pixel(&pixels, width, 6, 8);
    let gap = pixel(&pixels, width, 11, 8);
    let shadow = pixel(&pixels, width, 15, 8);
    assert!(
        source[0] > 250 && source[2] < 5,
        "source changed: {source:?}"
    );
    assert!(gap[3] < 5, "zero blur should keep a sharp gap: {gap:?}");
    assert!(
        shadow[2] > 240 && shadow[3] > 240,
        "shifted shadow missing: {shadow:?}"
    );
}

#[test]
fn nested_transformed_drop_shadow_keeps_css_offset_across_device_scales() {
    for scale in [1.0, 1.25, 2.0] {
        let Some(mut surface) = test_surface() else {
            return;
        };
        surface.set_logical_pixels(true);
        surface.set_dpi_scale(scale);
        let mut canvas = surface.begin_frame(physical(32.0, scale), physical(16.0, scale));
        canvas.clear(ColorLinPremul::default());
        canvas.push_opacity(0.5);
        canvas.push_filter(FilterEffect::DropShadow(DropShadow {
            offset: [8.0, 0.0],
            blur_radius: 0.0,
            color: SrgbColor::rgba(0, 0, 255, 255),
        }));
        canvas.push_transform(jag_draw::Transform2D::translate(2.0, 0.0));
        canvas.fill_rect(
            2.0,
            4.0,
            6.0,
            8.0,
            Brush::Solid(ColorLinPremul::from_srgba_u8([255, 0, 0, 255])),
            1,
        );
        canvas.pop_transform();
        canvas.pop_filter();
        canvas.pop_opacity();

        let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
        let source = pixel(&pixels, width, physical(6.0, scale), physical(8.0, scale));
        let gap = pixel(&pixels, width, physical(11.0, scale), physical(8.0, scale));
        let shadow = pixel(&pixels, width, physical(14.0, scale), physical(8.0, scale));
        assert_alpha_near(source, 128);
        assert_alpha_near(gap, 0);
        assert_alpha_near(shadow, 128);
    }
}

#[test]
fn blur_halo_keeps_css_extent_across_device_scales() {
    for scale in [1.0, 1.25, 2.0] {
        let Some(mut surface) = test_surface() else {
            return;
        };
        surface.set_logical_pixels(true);
        surface.set_dpi_scale(scale);
        let mut canvas = surface.begin_frame(physical(32.0, scale), physical(16.0, scale));
        canvas.clear(ColorLinPremul::default());
        canvas.push_filter(FilterEffect::Blur(1.5));
        canvas.fill_rect(
            10.0,
            4.0,
            12.0,
            8.0,
            Brush::Solid(ColorLinPremul::from_srgba_u8([255; 4])),
            1,
        );
        canvas.pop_filter();

        let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
        let far = pixel(&pixels, width, physical(1.0, scale), physical(8.0, scale))[3];
        let halo = pixel(&pixels, width, physical(8.0, scale), physical(8.0, scale))[3];
        let center = pixel(&pixels, width, physical(16.0, scale), physical(8.0, scale))[3];
        assert!(far <= 2, "far alpha changed at {scale}x: {far}");
        assert!(halo > 2, "blur halo vanished at {scale}x: {halo}");
        assert!(center > halo, "center {center} <= halo {halo} at {scale}x");
    }
}

#[test]
fn nested_mask_keeps_css_geometry_across_device_scales() {
    for scale in [1.0, 1.25, 2.0] {
        let Some((device, queue, mut surface)) = test_gpu() else {
            return;
        };
        let mask_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cross-dpr-mask"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            mask_texture.as_image_copy(),
            &[0, 0, 0, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let mask_id = ExternalTextureId(90);
        surface
            .pass_manager()
            .register_external_texture(mask_id, mask_texture.create_view(&Default::default()));
        surface.set_logical_pixels(true);
        surface.set_dpi_scale(scale);
        let mut canvas = surface.begin_frame(physical(24.0, scale), physical(12.0, scale));
        canvas.clear(ColorLinPremul::default());
        canvas.push_opacity(0.5);
        canvas.push_filter(FilterEffect::Mask(MaskEffect {
            texture_id: mask_id,
            mode: MaskMode::Alpha,
            rect: jag_draw::Rect {
                x: 8.0,
                y: 2.0,
                w: 8.0,
                h: 8.0,
            },
            mapping: None,
        }));
        canvas.fill_rect(
            4.0,
            2.0,
            16.0,
            8.0,
            Brush::Solid(ColorLinPremul::from_srgba_u8([255; 4])),
            1,
        );
        canvas.pop_filter();
        canvas.pop_opacity();

        let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
        for (x, alpha) in [(6.0, 0), (10.0, 128), (18.0, 0)] {
            assert_alpha_near(
                pixel(&pixels, width, physical(x, scale), physical(6.0, scale)),
                alpha,
            );
        }
    }
}
