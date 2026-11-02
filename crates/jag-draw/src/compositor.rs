use std::ops::Range;

use anyhow::{Result, bail};

use crate::display_list::{Command, DisplayList};
use crate::scene::{FilterEffect, Path, PathCmd, Rect, Transform2D};

/// An effect applied once when an isolated surface is composited into its parent.
#[derive(Clone, Debug, PartialEq)]
pub enum SurfaceEffect {
    Opacity(f32),
    Blur(f32),
    ColorMatrix(crate::ColorMatrix),
    DropShadow(crate::DropShadow),
    Mask(crate::MaskEffect),
    MaskGroup(crate::MaskGroupEffect),
}

/// A display-list range that must be rendered into an isolated intermediate surface.
#[derive(Clone, Debug, PartialEq)]
pub struct CompositorSurface {
    pub parent: Option<usize>,
    pub commands: Range<usize>,
    pub effect: SurfaceEffect,
    /// State inherited from scopes that began before this surface.
    pub inherited_clip: Option<Rect>,
    pub inherited_transform: Transform2D,
    /// Conservative world-space ink bounds after transforms and active clips.
    pub bounds: Option<Rect>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompositorPlan {
    pub surfaces: Vec<CompositorSurface>,
}

struct OpenSurface {
    id: usize,
    start: usize,
    effect: SurfaceEffect,
    bounds: Option<Rect>,
}

/// Extract isolated surface ownership before upload destroys display-list grouping.
pub fn build_compositor_plan(list: &DisplayList) -> Result<CompositorPlan> {
    let mut plan = CompositorPlan::default();
    let mut open: Vec<OpenSurface> = Vec::new();
    let mut clips: Vec<Option<Rect>> = vec![None];
    let mut transforms = vec![Transform2D::identity()];

    for (index, command) in list.commands.iter().enumerate() {
        match command {
            Command::PushOpacity(alpha) => {
                let effect = SurfaceEffect::Opacity(alpha.clamp(0.0, 1.0));
                push_surface(&mut plan, &mut open, &clips, &transforms, index, effect);
            }
            Command::PushFilter(filter) => {
                let effect = match filter {
                    FilterEffect::Blur(radius) => SurfaceEffect::Blur(radius.max(0.0)),
                    FilterEffect::ColorMatrix(matrix) => SurfaceEffect::ColorMatrix(*matrix),
                    FilterEffect::DropShadow(shadow) => SurfaceEffect::DropShadow(*shadow),
                    FilterEffect::Mask(mask) => SurfaceEffect::Mask(*mask),
                    FilterEffect::MaskGroup(group) => SurfaceEffect::MaskGroup(group.clone()),
                };
                push_surface(&mut plan, &mut open, &clips, &transforms, index, effect);
            }
            Command::PopOpacity | Command::PopFilter => {
                let Some(surface) = open.last() else {
                    bail!("effect pop at command {index} has no matching push");
                };
                let matches = matches!(
                    (command, &surface.effect),
                    (Command::PopOpacity, SurfaceEffect::Opacity(_))
                        | (Command::PopFilter, SurfaceEffect::Blur(_))
                        | (Command::PopFilter, SurfaceEffect::ColorMatrix(_))
                        | (Command::PopFilter, SurfaceEffect::DropShadow(_))
                        | (Command::PopFilter, SurfaceEffect::Mask(_))
                        | (Command::PopFilter, SurfaceEffect::MaskGroup(_))
                );
                if !matches {
                    bail!("mismatched effect pop at command {index}");
                }
                let surface = open.pop().unwrap();
                let bounds = match surface.effect {
                    SurfaceEffect::Blur(radius) => {
                        surface.bounds.map(|bounds| outset(bounds, radius * 6.0))
                    }
                    SurfaceEffect::Opacity(_) => surface.bounds,
                    SurfaceEffect::ColorMatrix(_) => surface.bounds,
                    SurfaceEffect::DropShadow(shadow) => surface.bounds.and_then(|bounds| {
                        let shifted = Rect {
                            x: bounds.x + shadow.offset[0],
                            y: bounds.y + shadow.offset[1],
                            ..bounds
                        };
                        union(
                            Some(bounds),
                            Some(outset(shifted, shadow.blur_radius * 6.0)),
                        )
                    }),
                    SurfaceEffect::Mask(_) => surface.bounds,
                    SurfaceEffect::MaskGroup(_) => surface.bounds,
                };
                let completed = &mut plan.surfaces[surface.id];
                completed.commands = surface.start..index;
                completed.bounds = bounds;
                if let Some(parent) = open.last_mut() {
                    parent.bounds = union(parent.bounds, bounds);
                }
            }
            Command::PushClip(clip) => {
                let world_clip = transformed_bounds(clip.0, *transforms.last().unwrap());
                clips.push(intersection(*clips.last().unwrap_or(&None), world_clip));
            }
            Command::PopClip => {
                if clips.len() == 1 {
                    bail!("PopClip at command {index} has no matching PushClip");
                }
                clips.pop();
            }
            Command::PushTransform(transform) => transforms.push(*transform),
            Command::PopTransform => {
                if transforms.len() == 1 {
                    bail!("PopTransform at command {index} has no matching PushTransform");
                }
                transforms.pop();
            }
            _ => {
                if let Some(surface) = open.last_mut() {
                    let ink = command_bounds(command).and_then(|bounds| {
                        clips
                            .last()
                            .copied()
                            .flatten()
                            .map_or(Some(bounds), |clip| intersection(Some(bounds), clip))
                    });
                    surface.bounds = union(surface.bounds, ink);
                }
            }
        }
    }

    if let Some(surface) = open.last() {
        bail!(
            "effect push before command {} has no matching pop",
            surface.start
        );
    }
    if clips.len() != 1 {
        bail!(
            "display list has {} unclosed clip scope(s)",
            clips.len() - 1
        );
    }
    if transforms.len() != 1 {
        bail!(
            "display list has {} unclosed transform scope(s)",
            transforms.len() - 1
        );
    }
    Ok(plan)
}

fn push_surface(
    plan: &mut CompositorPlan,
    open: &mut Vec<OpenSurface>,
    clips: &[Option<Rect>],
    transforms: &[Transform2D],
    index: usize,
    effect: SurfaceEffect,
) {
    let id = plan.surfaces.len();
    plan.surfaces.push(CompositorSurface {
        parent: open.last().map(|surface| surface.id),
        commands: index + 1..index + 1,
        effect: effect.clone(),
        inherited_clip: *clips.last().unwrap(),
        inherited_transform: *transforms.last().unwrap(),
        bounds: None,
    });
    open.push(OpenSurface {
        id,
        start: index + 1,
        effect,
        bounds: None,
    });
}

pub fn command_bounds(command: &Command) -> Option<Rect> {
    if let Command::DrawProjectedTexture { bounds, .. } = command {
        return Some(*bounds);
    }
    if let Command::DrawSpatialLayer(layer) = command {
        let bounds = layer
            .commands
            .iter()
            .filter_map(command_bounds)
            .reduce(|a, b| Rect {
                x: a.x.min(b.x),
                y: a.y.min(b.y),
                w: (a.x + a.w).max(b.x + b.w) - a.x.min(b.x),
                h: (a.y + a.h).max(b.y + b.h) - a.y.min(b.y),
            })?;
        return crate::spatial::bounds(&crate::spatial::quad(
            crate::spatial::Matrix4::from_array(layer.matrix),
            bounds,
        ));
    }
    let (rect, transform) = match command {
        Command::DrawCompositeTile { rect, .. } => (*rect, Transform2D::identity()),
        Command::DrawRect {
            rect, transform, ..
        } => (*rect, *transform),
        Command::StrokeRect {
            rect,
            stroke,
            transform,
            ..
        } => (outset(*rect, stroke.width * 0.5), *transform),
        Command::DrawRoundedRect {
            rrect, transform, ..
        } => (rrect.rect, *transform),
        Command::StrokeRoundedRect {
            rrect,
            stroke,
            transform,
            ..
        } => (outset(rrect.rect, stroke.width * 0.5), *transform),
        Command::BoxShadow {
            rrect,
            spec,
            transform,
            ..
        } => {
            let shadow = Rect {
                x: rrect.rect.x + spec.offset[0],
                y: rrect.rect.y + spec.offset[1],
                ..rrect.rect
            };
            (
                outset(shadow, spec.spread + spec.blur_radius * 1.5),
                *transform,
            )
        }
        Command::DrawText { run, transform, .. } => (
            Rect {
                x: run.pos[0],
                y: run.pos[1] - run.size,
                w: run.size * run.text.chars().count() as f32,
                h: run.size * 1.25,
            },
            *transform,
        ),
        Command::DrawEllipse {
            center,
            radii,
            transform,
            ..
        } => (
            Rect {
                x: center[0] - radii[0],
                y: center[1] - radii[1],
                w: radii[0] * 2.0,
                h: radii[1] * 2.0,
            },
            *transform,
        ),
        Command::FillPath {
            path, transform, ..
        } => (path_bounds(path)?, *transform),
        Command::StrokePath {
            path,
            stroke,
            transform,
            ..
        } => (outset(path_bounds(path)?, stroke.width * 0.5), *transform),
        Command::DrawSvg {
            origin,
            max_size,
            transform,
            ..
        } => (origin_size(*origin, *max_size), *transform),
        Command::DrawImage {
            origin,
            size,
            transform,
            ..
        } => (origin_size(*origin, *size), *transform),
        Command::DrawHyperlink {
            hyperlink,
            transform,
            ..
        } => (
            Rect {
                x: hyperlink.pos[0],
                y: hyperlink.pos[1] - hyperlink.size,
                w: hyperlink
                    .measured_width
                    .unwrap_or(hyperlink.size * hyperlink.text.chars().count() as f32),
                h: hyperlink.size * 1.25,
            },
            *transform,
        ),
        Command::DrawExternalTexture {
            rect, transform, ..
        } => (*rect, *transform),
        Command::BackdropFilter(draw) => (draw.rect, draw.transform),
        _ => return None,
    };
    Some(transformed_bounds(rect, transform))
}

fn origin_size(origin: [f32; 2], size: [f32; 2]) -> Rect {
    Rect {
        x: origin[0],
        y: origin[1],
        w: size[0],
        h: size[1],
    }
}

fn outset(rect: Rect, amount: f32) -> Rect {
    let amount = amount.max(0.0);
    Rect {
        x: rect.x - amount,
        y: rect.y - amount,
        w: rect.w + amount * 2.0,
        h: rect.h + amount * 2.0,
    }
}

fn path_bounds(path: &Path) -> Option<Rect> {
    let mut bounds = None;
    for command in &path.cmds {
        match command {
            PathCmd::MoveTo(point) | PathCmd::LineTo(point) => {
                bounds = include_points(bounds, &[*point]);
            }
            PathCmd::QuadTo(control, point) => {
                bounds = include_points(bounds, &[*control, *point]);
            }
            PathCmd::CubicTo(first, second, point) => {
                bounds = include_points(bounds, &[*first, *second, *point]);
            }
            PathCmd::Close => {}
        }
    }
    bounds
}

fn include_points(mut bounds: Option<Rect>, points: &[[f32; 2]]) -> Option<Rect> {
    for point in points {
        bounds = union(
            bounds,
            Some(Rect {
                x: point[0],
                y: point[1],
                w: 0.0,
                h: 0.0,
            }),
        );
    }
    bounds
}

fn transformed_bounds(rect: Rect, transform: Transform2D) -> Rect {
    let points = [
        [rect.x, rect.y],
        [rect.x + rect.w, rect.y],
        [rect.x, rect.y + rect.h],
        [rect.x + rect.w, rect.y + rect.h],
    ]
    .map(|[x, y]| {
        let [a, b, c, d, e, f] = transform.m;
        [a * x + c * y + e, b * x + d * y + f]
    });
    include_points(None, &points).unwrap()
}

fn union(left: Option<Rect>, right: Option<Rect>) -> Option<Rect> {
    match (left, right) {
        (None, other) | (other, None) => other,
        (Some(a), Some(b)) => {
            let x = a.x.min(b.x);
            let y = a.y.min(b.y);
            Some(Rect {
                x,
                y,
                w: (a.x + a.w).max(b.x + b.w) - x,
                h: (a.y + a.h).max(b.y + b.h) - y,
            })
        }
    }
}

fn intersection(left: Option<Rect>, right: Rect) -> Option<Rect> {
    let Some(left) = left else { return Some(right) };
    let x = left.x.max(right.x);
    let y = left.y.max(right.y);
    let far_x = (left.x + left.w).min(right.x + right.w);
    let far_y = (left.y + left.h).min(right.y + right.h);
    (far_x >= x && far_y >= y).then_some(Rect {
        x,
        y,
        w: far_x - x,
        h: far_y - y,
    })
}

#[cfg(test)]
#[path = "compositor_tests.rs"]
mod tests;
