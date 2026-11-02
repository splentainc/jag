#[test]
fn mask_group_composites_all_css_operators_before_applying_content() {
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
    let upload_mask = |label: &'static str, alpha: u8| {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
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
            texture.as_image_copy(),
            &[0, 0, 0, alpha],
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
        texture
    };
    let top_texture = upload_mask("mask-top-half", 128);
    let below_texture = upload_mask("mask-below-quarter", 64);
    let opaque_texture = upload_mask("mask-text-clip-opaque", 255);
    let top_id = ExternalTextureId(50);
    let below_id = ExternalTextureId(51);
    let opaque_id = ExternalTextureId(52);
    let mut surface = JagSurface::new(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb);
    surface.set_frame_cache_enabled(false);
    surface
        .pass_manager()
        .register_external_texture(top_id, top_texture.create_view(&Default::default()));
    surface
        .pass_manager()
        .register_external_texture(below_id, below_texture.create_view(&Default::default()));
    surface
        .pass_manager()
        .register_external_texture(opaque_id, opaque_texture.create_view(&Default::default()));

    let mut canvas = surface.begin_frame(24, 4);
    canvas.set_text_provider(Arc::new(BlockTextProvider));
    canvas.clear(ColorLinPremul::default());
    for (index, composite) in [
        MaskComposite::Add,
        MaskComposite::Subtract,
        MaskComposite::Intersect,
        MaskComposite::Exclude,
    ]
    .into_iter()
    .enumerate()
    {
        let rect = jag_draw::Rect {
            x: (index * 4) as f32,
            y: 0.0,
            w: 4.0,
            h: 4.0,
        };
        let layer = |texture_id| MaskEffect {
            texture_id,
            mode: MaskMode::Alpha,
            rect,
            mapping: None,
        };
        assert!(canvas.push_mask_group(vec![
            MaskCompositeLayer {
                mask: layer(top_id),
                composite,
                text_clip: false,
            },
            MaskCompositeLayer {
                mask: layer(below_id),
                composite: MaskComposite::Add,
                text_clip: false,
            },
        ]));
        canvas.fill_rect(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            Brush::Solid(ColorLinPremul::from_srgba_u8([255; 4])),
            index as i32,
        );
        canvas.pop_filter();
    }
    let text_rect = jag_draw::Rect {
        x: 16.0,
        y: 0.0,
        w: 8.0,
        h: 4.0,
    };
    assert!(canvas.push_mask_group(vec![MaskCompositeLayer {
        mask: MaskEffect {
            texture_id: opaque_id,
            mode: MaskMode::Alpha,
            rect: text_rect,
            mapping: None,
        },
        composite: MaskComposite::Add,
        text_clip: true,
    }]));
    canvas.fill_rect(
        text_rect.x,
        text_rect.y,
        text_rect.w,
        text_rect.h,
        Brush::Solid(ColorLinPremul::from_srgba_u8([255; 4])),
        5,
    );
    canvas.draw_text_run(
        [19.0, 1.0],
        "x".into(),
        2.0,
        ColorLinPremul::from_srgba_u8([255; 4]),
        6,
    );
    canvas.pop_filter();

    let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    for (x, expected) in [(2, 160), (6, 96), (10, 32), (14, 128)] {
        assert_alpha_near(pixel(&pixels, width, x, 2), expected);
    }
    assert_alpha_near(pixel(&pixels, width, 17, 2), 0);
    assert_alpha_near(pixel(&pixels, width, 19, 2), 255);
    assert_alpha_near(pixel(&pixels, width, 22, 2), 0);
}

fn assert_alpha_near(actual: [u8; 4], expected: u8) {
    assert!(
        actual[3].abs_diff(expected) <= 3,
        "expected alpha near {expected}, got pixel {actual:?}"
    );
}

#[test]
fn overlapping_and_nested_descendants_composite_each_group_once() {
    let instance = wgpu::Instance::default();
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    })) else {
        eprintln!("skipping compositor render test: no GPU adapter available");
        return;
    };
    let Ok((device, queue)) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
    else {
        eprintln!("skipping compositor render test: GPU device unavailable");
        return;
    };

    let mut surface = JagSurface::new(
        Arc::new(device),
        Arc::new(queue),
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    surface.set_frame_cache_enabled(false);
    let mut canvas = surface.begin_frame(24, 12);
    canvas.clear(ColorLinPremul::default());

    let red = Brush::Solid(ColorLinPremul {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    });
    let blue = Brush::Solid(ColorLinPremul {
        r: 0.0,
        g: 0.0,
        b: 1.0,
        a: 1.0,
    });
    canvas.push_opacity(0.5);
    canvas.fill_rect(2.0, 2.0, 8.0, 8.0, red.clone(), 1);
    canvas.fill_rect(6.0, 2.0, 8.0, 8.0, red, 2);
    canvas.push_opacity(0.5);
    canvas.fill_rect(16.0, 2.0, 4.0, 8.0, blue, 3);
    canvas.pop_opacity();
    canvas.pop_opacity();

    let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    let single_red = pixel(&pixels, width, 3, 4);
    let overlapping_red = pixel(&pixels, width, 8, 4);
    let nested_blue = pixel(&pixels, width, 18, 4);

    assert_alpha_near(single_red, 128);
    assert_alpha_near(overlapping_red, 128);
    assert_alpha_near(nested_blue, 64);
}

#[test]
fn transformed_opacity_group_preserves_rounded_rect_corners() {
    let Some(mut surface) = test_surface() else {
        return;
    };
    let mut canvas = surface.begin_frame(24, 24);
    canvas.clear(ColorLinPremul::from_srgba_u8([255, 0, 0, 255]));
    canvas.push_opacity(1.0);
    canvas.push_transform(jag_draw::Transform2D::translate(4.0, 4.0));
    canvas.push_clip_rect(jag_draw::Rect {
        x: 0.0,
        y: 0.0,
        w: 16.0,
        h: 16.0,
    });
    canvas.rounded_rect(
        jag_draw::RoundedRect {
            rect: jag_draw::Rect {
                x: 0.0,
                y: 0.0,
                w: 16.0,
                h: 16.0,
            },
            radii: jag_draw::RoundedRadii {
                tl: 6.0,
                tr: 6.0,
                br: 6.0,
                bl: 6.0,
            },
        },
        Brush::Solid(ColorLinPremul::from_srgba_u8([0, 0, 255, 255])),
        1,
    );
    canvas.pop_clip();
    canvas.pop_transform();
    canvas.pop_opacity();

    let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    let corner = pixel(&pixels, width, 4, 4);
    let center = pixel(&pixels, width, 12, 12);
    assert!(
        corner[0] > 240 && corner[2] < 10,
        "rounded corner must expose the red parent: {corner:?}"
    );
    assert!(
        center[2] > 240 && center[0] < 10,
        "rounded rect center must stay blue: {center:?}"
    );
}

#[test]
fn svg_descendant_is_atomic_with_its_opacity_group() {
    let Some(mut surface) = test_surface() else {
        return;
    };
    let svg = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../images/send.svg");
    let mut canvas = surface.begin_frame(32, 32);
    canvas.clear(ColorLinPremul::default());
    canvas.push_opacity(0.5);
    canvas.fill_rect(
        0.0,
        0.0,
        32.0,
        32.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([0, 0, 255, 255])),
        1,
    );
    canvas.draw_svg_styled(
        svg,
        [4.0, 4.0],
        [24.0, 24.0],
        jag_draw::SvgStyle::new().with_stroke(ColorLinPremul::from_srgba_u8([255; 4])),
        3,
    );
    canvas.pop_opacity();
    canvas.fill_rect(
        0.0,
        0.0,
        32.0,
        32.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([255, 0, 0, 255])),
        2,
    );

    let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    for y in 1..31 {
        for x in 1..31 {
            let actual = pixel(&pixels, width, x, y);
            assert!(
                actual[0] > 250 && actual[1] < 5 && actual[2] < 5 && actual[3] > 250,
                "opacity-group SVG escaped above its sibling at ({x},{y}): {actual:?}"
            );
        }
    }
}

#[test]
fn vector_svg_opacity_group_preserves_z_near_bottom_at_device_scale() {
    let Some(mut surface) = test_surface() else {
        return;
    };
    let scale = 2.0;
    surface.set_logical_pixels(true);
    surface.set_dpi_scale(scale);
    let svg = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../images/send.svg");
    let mut canvas = surface.begin_frame(physical(400.0, scale), physical(128.0, scale));
    canvas.clear(ColorLinPremul::from_srgba_u8([255; 4]));
    canvas.push_opacity(0.5);
    canvas.fill_rect(
        289.0,
        90.0,
        77.0,
        32.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([23, 23, 23, 255])),
        10,
    );
    canvas.draw_svg(svg, [301.0, 98.0], [16.0, 16.0], 12);
    canvas.pop_opacity();
    canvas.fill_rect(
        289.0,
        90.0,
        77.0,
        32.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([255; 4])),
        5,
    );

    let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    let background = pixel(
        &pixels,
        width,
        physical(295.0, scale),
        physical(106.0, scale),
    );
    assert!(
        background[0] < 230 && background[1] < 230 && background[2] < 230,
        "bottom opacity group disappeared at {scale}x: {background:?}"
    );
}
