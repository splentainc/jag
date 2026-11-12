//! YCbCr planes to 8-bit RGBA, per ITU-T H.273.

use super::av1::{Cicp, Frame, Layout, Plane};

// H.273 MatrixCoefficients code points.
const MATRIX_IDENTITY: u32 = 0;
const MATRIX_BT709: u32 = 1;
const MATRIX_UNSPECIFIED: u32 = 2;
const MATRIX_FCC: u32 = 4;
const MATRIX_BT470BG: u32 = 5;
const MATRIX_BT601: u32 = 6;
const MATRIX_SMPTE240: u32 = 7;
const MATRIX_BT2020_NCL: u32 = 9;

// H.273 TransferCharacteristics code points for HDR signals.
const TRANSFER_PQ: u32 = 16;
const TRANSFER_HLG: u32 = 18;

// AV1 spec 6.4.2 `chroma_sample_position`.
const CHROMA_VERTICAL: u32 = 1;
const CHROMA_COLOCATED: u32 = 2;

enum Matrix {
    /// Planes hold G, B, R directly.
    Identity,
    /// Luma weights Kr and Kb (H.273 Table 4).
    Weights { kr: f32, kb: f32 },
}

fn matrix_for(code: u32) -> Result<Matrix, String> {
    let (kr, kb) = match code {
        MATRIX_IDENTITY => return Ok(Matrix::Identity),
        MATRIX_BT709 => (0.2126, 0.0722),
        // MIAF 7.3.6.4: unspecified matrix coefficients mean BT.601.
        MATRIX_UNSPECIFIED | MATRIX_BT470BG | MATRIX_BT601 => (0.299, 0.114),
        MATRIX_FCC => (0.30, 0.11),
        MATRIX_SMPTE240 => (0.212, 0.087),
        MATRIX_BT2020_NCL => (0.2627, 0.0593),
        other => return Err(format!("unsupported AVIF matrix coefficients {other}")),
    };
    Ok(Matrix::Weights { kr, kb })
}

/// Sample scaling for one bit depth and range (H.273 8.3, equations 20-31).
struct Range {
    luma_offset: f32,
    luma_scale: f32,
    chroma_offset: f32,
    chroma_scale: f32,
}

impl Range {
    fn new(bit_depth: u32, full_range: bool, identity: bool) -> Self {
        let step = (1u32 << (bit_depth - 8)) as f32;
        let max = ((1u32 << bit_depth) - 1) as f32;
        let (luma_offset, luma_scale) = if full_range {
            (0.0, max)
        } else {
            (16.0 * step, 219.0 * step)
        };
        // Identity planes are all colour channels and share the luma scaling.
        let (chroma_offset, chroma_scale) = if identity {
            (luma_offset, luma_scale)
        } else if full_range {
            ((1u32 << (bit_depth - 1)) as f32, max)
        } else {
            ((1u32 << (bit_depth - 1)) as f32, 224.0 * step)
        };
        Self {
            luma_offset,
            luma_scale,
            chroma_offset,
            chroma_scale,
        }
    }

    fn luma(&self, sample: f32) -> f32 {
        (sample - self.luma_offset) / self.luma_scale
    }

    fn chroma(&self, sample: f32) -> f32 {
        (sample - self.chroma_offset) / self.chroma_scale
    }
}

/// Bilinear read of a subsampled chroma plane at a luma pixel.
struct ChromaSampler {
    /// Luma pixels per chroma sample on each axis (1 or 2).
    step: [f32; 2],
    /// Chroma-grid offset of luma pixel 0 on each axis.
    origin: [f32; 2],
}

impl ChromaSampler {
    fn new(layout: Layout, position: u32) -> Self {
        let halved = [
            matches!(layout, Layout::I420 | Layout::I422),
            layout == Layout::I420,
        ];
        // AV1 spec 6.4.2: "vertical" co-sites chroma with the left luma
        // column, "colocated" with the top-left luma sample. Otherwise the
        // sample sits at the centre of its luma block.
        let cosited = [
            matches!(position, CHROMA_VERTICAL | CHROMA_COLOCATED),
            position == CHROMA_COLOCATED,
        ];
        let axis = |i: usize| match (halved[i], cosited[i]) {
            (false, _) => (1.0, 0.0),
            (true, true) => (2.0, 0.0),
            (true, false) => (2.0, -0.25),
        };
        let (x, y) = (axis(0), axis(1));
        Self {
            step: [x.0, y.0],
            origin: [x.1, y.1],
        }
    }

    fn sample(&self, plane: &Plane, x: usize, y: usize) -> f32 {
        let fx = (x as f32 / self.step[0] + self.origin[0]).clamp(0.0, (plane.width - 1) as f32);
        let fy = (y as f32 / self.step[1] + self.origin[1]).clamp(0.0, (plane.height - 1) as f32);
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        let (x1, y1) = (
            (x0 + 1).min(plane.width - 1),
            (y0 + 1).min(plane.height - 1),
        );
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let at = |x: usize, y: usize| f32::from(plane.samples[y * plane.width + x]);
        let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
        let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
        top + (bottom - top) * ty
    }
}

fn to_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Convert a decoded colour frame, with its optional alpha frame, to
/// straight-alpha RGBA8. `cicp` is the colour description that applies to the
/// colour frame (container `colr` when present, otherwise the bitstream's).
pub(super) fn to_rgba(
    color: &Frame,
    cicp: Cicp,
    alpha: Option<&Frame>,
    premultiplied: bool,
) -> Result<image::RgbaImage, String> {
    if matches!(cicp.transfer, TRANSFER_PQ | TRANSFER_HLG) {
        return Err("HDR AVIF (PQ/HLG transfer) is not supported".into());
    }
    let matrix = matrix_for(cicp.matrix)?;
    let identity = matches!(matrix, Matrix::Identity);
    if identity && color.layout != Layout::I444 {
        return Err("identity matrix requires 4:4:4 AVIF".into());
    }
    let (width, height) = (color.luma.width, color.luma.height);
    if let Some(alpha) = alpha
        && (alpha.luma.width != width || alpha.luma.height != height)
    {
        return Err("AVIF alpha plane size differs from the colour plane".into());
    }
    let range = Range::new(color.bit_depth, cicp.full_range, identity);
    let alpha_range = alpha.map(|a| Range::new(a.bit_depth, a.cicp.full_range, false));
    let sampler = ChromaSampler::new(color.layout, color.chroma_position);

    let mut out = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            let luma = range.luma(f32::from(color.luma.samples[index]));
            let mut rgb = if color.layout == Layout::Monochrome {
                [luma; 3]
            } else {
                let u = range.chroma(sampler.sample(&color.chroma[0], x, y));
                let v = range.chroma(sampler.sample(&color.chroma[1], x, y));
                match matrix {
                    Matrix::Identity => [v, luma, u],
                    Matrix::Weights { kr, kb } => {
                        let r = luma + 2.0 * (1.0 - kr) * v;
                        let b = luma + 2.0 * (1.0 - kb) * u;
                        let g = (luma - kr * r - kb * b) / (1.0 - kr - kb);
                        [r, g, b]
                    }
                }
            };
            let a = match (alpha, &alpha_range) {
                (Some(alpha), Some(range)) => range
                    .luma(f32::from(alpha.luma.samples[index]))
                    .clamp(0.0, 1.0),
                _ => 1.0,
            };
            if premultiplied && a > 0.0 {
                rgb = rgb.map(|c| c / a);
            }
            out.extend(rgb.map(to_byte));
            out.push(to_byte(a));
        }
    }
    image::RgbaImage::from_raw(width as u32, height as u32, out)
        .ok_or_else(|| "AVIF pixel buffer size mismatch".to_string())
}
