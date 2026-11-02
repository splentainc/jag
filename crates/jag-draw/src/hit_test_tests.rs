use super::*;
use crate::{Command, DisplayList, Viewport};

#[test]
fn retained_hit_properties_follow_scroll_and_invalidate_changed_clip_ownership() {
    let viewport = Viewport {
        width: 200,
        height: 200,
    };
    let mut painter = crate::Painter::begin_frame(viewport);
    painter.push_bound_scroll_layer("list".into(), "offset".into(), [0.0, 0.0], [-1.0, -1.0]);
    painter.hit_region_rect(
        42,
        Rect {
            x: 20.0,
            y: 80.0,
            w: 40.0,
            h: 20.0,
        },
        1,
    );
    painter.pop_scroll_layer();
    let scene = Arc::new(crate::ScrollScene::new(
        painter.finish().commands.into(),
        Transform2D::identity(),
        1.0,
        None,
    ));
    let make_list = |offset: f32, inside_clip: bool| {
        let draw = Command::DrawScrollScene {
            scene: scene.clone(),
            base: Transform2D::identity(),
            inputs: Arc::new([("offset".into(), [0.0, offset])].into_iter().collect()),
        };
        let clip = Command::PushClip(ClipRect(Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 60.0,
        }));
        DisplayList {
            viewport,
            commands: if inside_clip {
                vec![clip, draw, Command::PopClip]
            } else {
                vec![draw, clip, Command::PopClip]
            },
        }
    };
    let mut index = HitIndex::build(&make_list(0.0, true));
    assert_ne!(index.topmost_at([30.0, 90.0]).unwrap().region_id, Some(42));
    assert!(index.update_retained_scenes(&make_list(40.0, true)));
    assert_eq!(index.topmost_at([30.0, 50.0]).unwrap().region_id, Some(42));
    assert_ne!(index.topmost_at([30.0, 90.0]).unwrap().region_id, Some(42));
    assert!(
        !index.update_retained_scenes(&make_list(0.0, false)),
        "moving a scene out of a clip must rebuild its inherited clip state"
    );
    assert_eq!(
        HitIndex::build(&make_list(0.0, false))
            .topmost_at([30.0, 90.0])
            .unwrap()
            .region_id,
        Some(42)
    );
}

#[test]
fn inline_hyperlink_hit_wins_over_overlapping_text_region() {
    let mut list = DisplayList {
        viewport: Viewport {
            width: 400,
            height: 120,
        },
        commands: Vec::new(),
    };

    let hyperlink = Hyperlink {
        text: "default blue underlined link".to_string(),
        pos: [20.0, 36.0], // baseline
        size: 16.0,
        color: ColorLinPremul::from_srgba_u8([0x00, 0x66, 0xcc, 0xff]),
        url: "https://example.com".to_string(),
        weight: 400.0,
        measured_width: Some(180.0),
        underline: true,
        underline_color: None,
        family: None,
        style: FontStyle::Normal,
    };

    // Inline hyperlink draw.
    list.commands.push(Command::DrawHyperlink {
        hyperlink,
        z: 0,
        transform: Transform2D::identity(),
        id: 1,
    });

    // Simulate overlapping paragraph text hit region at z + 10.
    list.commands.push(Command::HitRegionRect {
        id: 4242,
        rect: Rect {
            x: 10.0,
            y: 10.0,
            w: 300.0,
            h: 40.0,
        },
        z: 10,
        transform: Transform2D::identity(),
    });

    let hit = HitIndex::build(&list)
        .topmost_at([80.0, 28.0])
        .expect("expected a hit over inline hyperlink");
    assert_eq!(hit.kind, HitKind::Hyperlink);
    match hit.shape {
        HitShape::Hyperlink { url, .. } => assert_eq!(url, "https://example.com"),
        other => panic!("expected hyperlink hit shape, got {other:?}"),
    }
}
