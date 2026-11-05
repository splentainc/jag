//! Conservative visibility in the final viewport, even inside flat 3D groups.
use anyhow::Result;
use jag_draw::{
    Command, DisplayList, FilterEffect, Viewport,
    spatial::{self, Matrix4},
};

pub(super) fn expands(commands: &[Command]) -> bool {
    commands.iter().any(|c| {
        matches!(
            c,
            Command::PushFilter(FilterEffect::Blur(_) | FilterEffect::DropShadow(_))
                | Command::BackdropFilter(_)
        )
    })
}

pub(super) fn visible(
    commands: &[Command],
    transform: Option<Matrix4>,
    viewport: Viewport,
    scale: f32,
) -> Result<bool> {
    let Some(transform) = transform else {
        return Ok(true);
    };
    // Retained scroll commands have not been resolved into these coordinates.
    if commands.iter().any(|c| {
        matches!(
            c,
            Command::DrawScrollScene { .. } | Command::BackdropFilter(_)
        )
    }) {
        return Ok(true);
    }
    let mut bounds = commands
        .iter()
        .filter_map(jag_draw::command_bounds)
        .reduce(super::union);
    if expands(commands) {
        let plan = jag_draw::build_compositor_plan(&DisplayList {
            viewport,
            commands: commands.to_vec(),
        })?;
        for surface in plan.surfaces.iter().filter(|s| s.parent.is_none()) {
            if let Some(r) = surface.bounds {
                bounds = Some(bounds.map_or(r, |b| super::union(b, r)));
            }
        }
    }
    let Some(bounds) = bounds else {
        return Ok(true);
    };
    let quad = spatial::quad(transform, bounds);
    if quad.iter().flatten().any(|v| !v.is_finite()) {
        return Ok(true);
    }
    let Some(bounds) = spatial::bounds(&quad) else {
        return Ok(false);
    };
    // Include one device pixel for edge antialiasing and floating-point phase.
    let margin = 1. / scale;
    Ok(bounds.x <= viewport.width as f32 / scale + margin
        && bounds.y <= viewport.height as f32 / scale + margin
        && bounds.x + bounds.w >= -margin
        && bounds.y + bounds.h >= -margin)
}
