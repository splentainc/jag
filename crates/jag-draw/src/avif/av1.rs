//! One AV1 still frame decoded with `rav1d`, copied out as plain planes.

use std::mem::MaybeUninit;
use std::ptr::NonNull;

use rav1d::include::dav1d::data::Dav1dData;
use rav1d::include::dav1d::dav1d::{Dav1dContext, Dav1dSettings};
use rav1d::include::dav1d::headers::{
    DAV1D_PIXEL_LAYOUT_I400, DAV1D_PIXEL_LAYOUT_I420, DAV1D_PIXEL_LAYOUT_I422,
    DAV1D_PIXEL_LAYOUT_I444,
};
use rav1d::include::dav1d::picture::Dav1dPicture;
use rav1d::src::lib::{
    dav1d_close, dav1d_data_create, dav1d_data_unref, dav1d_default_settings, dav1d_get_picture,
    dav1d_open, dav1d_picture_unref, dav1d_send_data,
};

/// Chroma plane size relative to luma.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Layout {
    Monochrome,
    /// Chroma halved horizontally and vertically.
    I420,
    /// Chroma halved horizontally.
    I422,
    I444,
}

/// CICP colour description (ITU-T H.273 code points) of a frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Cicp {
    pub transfer: u32,
    pub matrix: u32,
    pub full_range: bool,
}

pub(super) struct Plane {
    pub width: usize,
    pub height: usize,
    pub samples: Vec<u16>,
}

pub(super) struct Frame {
    pub bit_depth: u32,
    pub layout: Layout,
    pub cicp: Cicp,
    /// AV1 `chroma_sample_position` (AV1 spec 6.4.2).
    pub chroma_position: u32,
    pub luma: Plane,
    /// U then V; empty for monochrome.
    pub chroma: Vec<Plane>,
}

/// Releases the decoder state on every exit path.
struct Session {
    context: Option<Dav1dContext>,
    data: Dav1dData,
    picture: Dav1dPicture,
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: each pointer is to a live field that rav1d initialised or
        // that is still in its `Default` (empty) state; all three calls accept
        // empty values.
        unsafe {
            dav1d_picture_unref(Some(NonNull::from(&mut self.picture)));
            dav1d_data_unref(Some(NonNull::from(&mut self.data)));
            dav1d_close(Some(NonNull::from(&mut self.context)));
        }
    }
}

/// Decode the first frame of an AV1 OBU stream.
pub(super) fn decode_frame(obu: &[u8]) -> Result<Frame, String> {
    if obu.is_empty() {
        return Err("empty AV1 payload".into());
    }
    let mut settings = MaybeUninit::<Dav1dSettings>::uninit();
    // SAFETY: `dav1d_default_settings` fully writes the pointed-to settings.
    let mut settings = unsafe {
        dav1d_default_settings(NonNull::from(&mut settings).cast());
        settings.assume_init()
    };
    // One frame in flight: the picture is ready as soon as its data is sent.
    settings.max_frame_delay = 1;

    let mut session = Session {
        context: None,
        data: Dav1dData::default(),
        picture: Dav1dPicture::default(),
    };
    // SAFETY: both pointers reference live, writable locals.
    let opened = unsafe {
        dav1d_open(
            Some(NonNull::from(&mut session.context)),
            Some(NonNull::from(&mut settings)),
        )
    };
    if opened.0 != 0 || session.context.is_none() {
        return Err(format!("AV1 decoder failed to open ({})", opened.0));
    }

    // SAFETY: `session.data` is writable; on success the returned buffer is
    // `obu.len()` bytes long and owned by `session.data`.
    let buffer = unsafe { dav1d_data_create(Some(NonNull::from(&mut session.data)), obu.len()) };
    if buffer.is_null() {
        return Err("AV1 input buffer allocation failed".into());
    }
    // SAFETY: `buffer` has room for `obu.len()` bytes and cannot overlap `obu`.
    unsafe { std::ptr::copy_nonoverlapping(obu.as_ptr(), buffer, obu.len()) };

    let mut decoded = false;
    loop {
        let remaining = session.data.sz;
        if remaining > 0 {
            // SAFETY: the context is open and `session.data` is a live buffer.
            unsafe { dav1d_send_data(session.context, Some(NonNull::from(&mut session.data))) };
        }
        // SAFETY: the context is open and `session.picture` is empty.
        let got = unsafe {
            dav1d_get_picture(session.context, Some(NonNull::from(&mut session.picture)))
        };
        if got.0 == 0 {
            decoded = true;
            break;
        }
        // No picture and no input consumed: the stream holds no frame.
        if session.data.sz == remaining {
            break;
        }
    }
    if !decoded {
        return Err("AV1 stream produced no frame".into());
    }
    copy_out(&session.picture)
}

fn copy_out(picture: &Dav1dPicture) -> Result<Frame, String> {
    let width = usize::try_from(picture.p.w).map_err(|_| "negative AV1 width")?;
    let height = usize::try_from(picture.p.h).map_err(|_| "negative AV1 height")?;
    if width == 0 || height == 0 {
        return Err("empty AV1 frame".into());
    }
    let bit_depth = u32::try_from(picture.p.bpc).map_err(|_| "negative AV1 bit depth")?;
    if !matches!(bit_depth, 8 | 10 | 12) {
        return Err(format!("unsupported AV1 bit depth {bit_depth}"));
    }
    let layout = match picture.p.layout {
        DAV1D_PIXEL_LAYOUT_I400 => Layout::Monochrome,
        DAV1D_PIXEL_LAYOUT_I420 => Layout::I420,
        DAV1D_PIXEL_LAYOUT_I422 => Layout::I422,
        DAV1D_PIXEL_LAYOUT_I444 => Layout::I444,
        other => return Err(format!("unknown AV1 pixel layout {other}")),
    };
    let header = picture.seq_hdr.ok_or("AV1 frame has no sequence header")?;
    // SAFETY: rav1d keeps the sequence header alive while the picture is.
    let header = unsafe { header.as_ref() };

    let chroma_width = match layout {
        Layout::I420 | Layout::I422 => width.div_ceil(2),
        _ => width,
    };
    let chroma_height = match layout {
        Layout::I420 => height.div_ceil(2),
        _ => height,
    };
    let luma = copy_plane(picture, 0, width, height, bit_depth)?;
    let chroma = if layout == Layout::Monochrome {
        Vec::new()
    } else {
        vec![
            copy_plane(picture, 1, chroma_width, chroma_height, bit_depth)?,
            copy_plane(picture, 2, chroma_width, chroma_height, bit_depth)?,
        ]
    };
    Ok(Frame {
        bit_depth,
        layout,
        cicp: Cicp {
            transfer: header.trc,
            matrix: header.mtrx,
            full_range: header.color_range != 0,
        },
        chroma_position: header.chr,
        luma,
        chroma,
    })
}

fn copy_plane(
    picture: &Dav1dPicture,
    index: usize,
    width: usize,
    height: usize,
    bit_depth: u32,
) -> Result<Plane, String> {
    let base = picture.data[index].ok_or("AV1 frame is missing a plane")?;
    // dav1d shares one stride between the two chroma planes.
    let stride =
        usize::try_from(picture.stride[index.min(1)]).map_err(|_| "negative AV1 plane stride")?;
    let bytes_per_sample = if bit_depth == 8 { 1 } else { 2 };
    if stride < width * bytes_per_sample {
        return Err("AV1 plane stride is shorter than its row".into());
    }
    let mut samples = Vec::with_capacity(width * height);
    for row in 0..height {
        // SAFETY: rav1d allocates `stride` bytes for each of the plane's rows,
        // and `width * bytes_per_sample <= stride` was checked above.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                base.as_ptr().cast::<u8>().add(row * stride),
                width * bytes_per_sample,
            )
        };
        if bit_depth == 8 {
            samples.extend(bytes.iter().map(|&b| u16::from(b)));
        } else {
            samples.extend(
                bytes
                    .chunks_exact(2)
                    .map(|pair| u16::from_ne_bytes([pair[0], pair[1]])),
            );
        }
    }
    Ok(Plane {
        width,
        height,
        samples,
    })
}
