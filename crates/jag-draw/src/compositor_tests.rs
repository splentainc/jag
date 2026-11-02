use super::*;
use crate::scene::{Brush, ColorLinPremul};

fn rect(x: f32, y: f32, w: f32, h: f32) -> Command {
    Command::DrawRect {
        rect: Rect { x, y, w, h },
        brush: Brush::Solid(ColorLinPremul::default()),
        z: 0,
        transform: Transform2D::identity(),
    }
}

#[test]
fn nested_surfaces_retain_parent_ranges_and_bounds() {
    let list = DisplayList {
        commands: vec![
            Command::PushOpacity(0.5),
            rect(0.0, 0.0, 10.0, 10.0),
            Command::PushOpacity(0.25),
            rect(20.0, 5.0, 5.0, 5.0),
            Command::PopOpacity,
            Command::PopOpacity,
        ],
        ..Default::default()
    };
    let plan = build_compositor_plan(&list).unwrap();

    assert_eq!(plan.surfaces[0].parent, None);
    assert_eq!(plan.surfaces[0].commands, 1..5);
    assert_eq!(
        plan.surfaces[0].bounds,
        Some(Rect {
            x: 0.0,
            y: 0.0,
            w: 25.0,
            h: 10.0
        })
    );
    assert_eq!(plan.surfaces[1].parent, Some(0));
    assert_eq!(plan.surfaces[1].commands, 3..4);
}

#[test]
fn transformed_ink_is_clipped_in_world_space() {
    let list = DisplayList {
        commands: vec![
            Command::PushTransform(Transform2D::translate(10.0, 0.0)),
            Command::PushClip(crate::scene::ClipRect(Rect {
                x: 5.0,
                y: 0.0,
                w: 5.0,
                h: 20.0,
            })),
            Command::PushOpacity(0.7),
            match rect(0.0, 0.0, 10.0, 10.0) {
                Command::DrawRect { rect, brush, z, .. } => Command::DrawRect {
                    rect,
                    brush,
                    z,
                    transform: Transform2D::translate(10.0, 0.0),
                },
                _ => unreachable!(),
            },
            Command::PopOpacity,
            Command::PopClip,
            Command::PopTransform,
        ],
        ..Default::default()
    };
    let surface = &build_compositor_plan(&list).unwrap().surfaces[0];
    assert_eq!(
        surface.bounds,
        Some(Rect {
            x: 15.0,
            y: 0.0,
            w: 5.0,
            h: 10.0
        })
    );
    assert_eq!(
        surface.inherited_clip,
        Some(Rect {
            x: 15.0,
            y: 0.0,
            w: 5.0,
            h: 20.0
        })
    );
    assert_eq!(
        surface.inherited_transform,
        Transform2D::translate(10.0, 0.0)
    );
}

#[test]
fn nested_transformed_clips_intersect_in_world_space() {
    let list = DisplayList {
        commands: vec![
            Command::PushTransform(Transform2D::translate(10.0, 5.0)),
            Command::PushClip(crate::scene::ClipRect(Rect {
                x: 0.0,
                y: 0.0,
                w: 20.0,
                h: 20.0,
            })),
            Command::PushTransform(Transform2D {
                m: [2.0, 0.0, 0.0, 2.0, 10.0, 5.0],
            }),
            Command::PushClip(crate::scene::ClipRect(Rect {
                x: 2.0,
                y: 3.0,
                w: 6.0,
                h: 7.0,
            })),
            Command::PushOpacity(0.5),
            match rect(0.0, 0.0, 10.0, 10.0) {
                Command::DrawRect { rect, brush, z, .. } => Command::DrawRect {
                    rect,
                    brush,
                    z,
                    transform: Transform2D {
                        m: [2.0, 0.0, 0.0, 2.0, 10.0, 5.0],
                    },
                },
                _ => unreachable!(),
            },
            Command::PopOpacity,
            Command::PopClip,
            Command::PopTransform,
            Command::PopClip,
            Command::PopTransform,
        ],
        ..Default::default()
    };
    let surface = &build_compositor_plan(&list).unwrap().surfaces[0];
    assert_eq!(
        surface.inherited_clip,
        Some(Rect {
            x: 14.0,
            y: 11.0,
            w: 12.0,
            h: 14.0,
        })
    );
    assert_eq!(surface.bounds, surface.inherited_clip);
}

#[test]
fn rejects_unbalanced_surface_scopes() {
    let list = DisplayList {
        commands: vec![Command::PushOpacity(0.5)],
        ..Default::default()
    };
    assert!(
        build_compositor_plan(&list)
            .unwrap_err()
            .to_string()
            .contains("no matching pop")
    );
}

#[test]
fn blur_surfaces_expand_for_kernel_support() {
    let list = DisplayList {
        commands: vec![
            Command::PushFilter(FilterEffect::Blur(2.0)),
            rect(10.0, 10.0, 4.0, 4.0),
            Command::PopFilter,
        ],
        ..Default::default()
    };
    let surface = &build_compositor_plan(&list).unwrap().surfaces[0];
    assert_eq!(surface.effect, SurfaceEffect::Blur(2.0));
    assert_eq!(
        surface.bounds,
        Some(Rect {
            x: -2.0,
            y: -2.0,
            w: 28.0,
            h: 28.0,
        })
    );
}

#[test]
fn drop_shadow_bounds_union_source_with_shifted_kernel_support() {
    let shadow = crate::DropShadow {
        offset: [5.0, -2.0],
        blur_radius: 1.0,
        color: crate::SrgbColor::rgba(0, 0, 0, 255),
    };
    let list = DisplayList {
        commands: vec![
            Command::PushFilter(FilterEffect::DropShadow(shadow)),
            rect(10.0, 10.0, 4.0, 4.0),
            Command::PopFilter,
        ],
        ..Default::default()
    };
    let surface = &build_compositor_plan(&list).unwrap().surfaces[0];
    assert_eq!(surface.effect, SurfaceEffect::DropShadow(shadow));
    assert_eq!(
        surface.bounds,
        Some(Rect {
            x: 9.0,
            y: 2.0,
            w: 16.0,
            h: 16.0,
        })
    );
}

#[test]
fn mask_surface_owns_descendants_without_expanding_ink_bounds() {
    let mask = crate::MaskEffect {
        texture_id: crate::ExternalTextureId(7),
        mode: crate::MaskMode::Alpha,
        rect: Rect {
            x: 10.0,
            y: 12.0,
            w: 4.0,
            h: 6.0,
        },
        mapping: None,
    };
    let list = DisplayList {
        commands: vec![
            Command::PushFilter(FilterEffect::Mask(mask)),
            rect(10.0, 12.0, 4.0, 6.0),
            Command::PopFilter,
        ],
        ..Default::default()
    };
    let surface = &build_compositor_plan(&list).unwrap().surfaces[0];
    assert_eq!(surface.effect, SurfaceEffect::Mask(mask));
    assert_eq!(
        surface.bounds,
        Some(Rect {
            x: 10.0,
            y: 12.0,
            w: 4.0,
            h: 6.0
        })
    );
}
