use super::{is_rotated, shader_clip};
use jag_draw::{Rect, Transform2D};

/// Where the shader would place a device pixel in clip space.
fn to_clip(from_device: [f32; 6], point: [f32; 2]) -> [f32; 2] {
    let [a, b, c, d, e, f] = from_device;
    [
        a * point[0] + c * point[1] + e,
        b * point[0] + d * point[1] + f,
    ]
}

fn assert_close(actual: [f32; 2], expected: [f32; 2]) {
    assert!(
        (actual[0] - expected[0]).abs() < 0.01 && (actual[1] - expected[1]).abs() < 0.01,
        "expected {expected:?}, got {actual:?}"
    );
}

#[test]
fn translated_and_scaled_clip_stays_a_device_rect() {
    let transform = Transform2D::translate(10.0, 20.0).concat(Transform2D::scale(2.0, 3.0));
    assert!(!is_rotated(transform));
    let clip = shader_clip(
        Rect {
            x: 1.0,
            y: 2.0,
            w: 4.0,
            h: 5.0,
        },
        [3.0; 4],
        transform,
        2.0,
    )
    .unwrap();
    let corner = transform_point(transform, [1.0, 2.0]);
    assert_close(
        [clip.rect.x, clip.rect.y],
        [corner[0] * 2.0, corner[1] * 2.0],
    );
    assert_close([clip.rect.w, clip.rect.h], [16.0, 30.0]);
    assert_eq!(clip.radii, [12.0; 4]);
    assert_eq!(
        clip.from_device,
        jag_draw::RoundedRectClipGpu::DEVICE_ALIGNED
    );
}

#[test]
fn rotated_clip_maps_each_device_corner_back_to_its_local_corner() {
    let transform = Transform2D::rotate_around((-6.0f32).to_radians(), 100.0, 50.0);
    assert!(is_rotated(transform));
    let scale = 2.0;
    let rect = Rect {
        x: 40.0,
        y: 30.0,
        w: 172.0,
        h: 365.0,
    };
    let clip = shader_clip(rect, [22.0; 4], transform, scale).unwrap();
    assert_eq!(clip.radii, [44.0; 4]);
    for (local, expected) in [
        ([rect.x, rect.y], [clip.rect.x, clip.rect.y]),
        (
            [rect.x + rect.w, rect.y + rect.h],
            [clip.rect.x + clip.rect.w, clip.rect.y + clip.rect.h],
        ),
    ] {
        let device = transform_point(transform, local).map(|v| v * scale);
        assert_close(to_clip(clip.from_device, device), expected);
    }
    assert_close([clip.rect.w, clip.rect.h], [344.0, 730.0]);
}

#[test]
fn singular_transform_has_no_clip() {
    let flat = Transform2D {
        m: [1.0, 1.0, 1.0, 1.0, 0.0, 0.0],
    };
    assert!(
        shader_clip(
            Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0
            },
            [0.0; 4],
            flat,
            1.0
        )
        .is_none()
    );
}

fn transform_point(transform: Transform2D, point: [f32; 2]) -> [f32; 2] {
    let [a, b, c, d, e, f] = transform.m;
    [
        a * point[0] + c * point[1] + e,
        b * point[0] + d * point[1] + f,
    ]
}
