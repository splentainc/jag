//! Sealed macOS system fonts can share reclaimable file pages. User fonts keep
//! owned bytes: mapping a caller-writable file would violate FontFace snapshots.
use super::shared_data::{FontData, SharedData};
use std::os::unix::ffi::OsStrExt;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};

static FILES: OnceLock<Mutex<HashMap<PathBuf, Weak<SharedData>>>> = OnceLock::new();

pub(super) fn open(path: &Path) -> Option<std::io::Result<Arc<SharedData>>> {
    let path = path.canonicalize().ok()?;
    if !path.starts_with("/System/Library/Fonts") {
        return None;
    }
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut status = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: the C path is terminated, and statfs initializes status on success.
    if unsafe { libc::statfs(c_path.as_ptr(), status.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: statfs succeeded above. Older/unsealed writable systems copy bytes.
    if unsafe { status.assume_init() }.f_flags & libc::MNT_RDONLY as u32 == 0 {
        return None;
    }
    Some((|| {
        let mut files = FILES.get_or_init(Mutex::default).lock().unwrap();
        files.retain(|_, value| value.strong_count() != 0);
        if let Some(data) = files.get(&path).and_then(Weak::upgrade) {
            return Ok(data);
        }
        let file = std::fs::File::open(&path)?;
        // SAFETY: canonicalization confines this to Apple's sealed, read-only
        // system font directory. Its files cannot be modified or truncated
        // while this OS instance runs. Writable/user font paths are excluded.
        let mapping = unsafe { memmap2::MmapOptions::new().map(&file)? };
        let data = Arc::new(SharedData {
            bytes: FontData::SystemFile(mapping, OnceLock::new()),
        });
        files.insert(path, Arc::downgrade(&data));
        Ok(data)
    })())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_faces_share_file_pages_and_legacy_exports_stay_separate() {
        let path = Path::new("/System/Library/Fonts/SFNS.ttf");
        let first = open(path).unwrap().unwrap();
        let second = open(path).unwrap().unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        let FontData::SystemFile(mapping, _) = &first.bytes else {
            panic!("system file was copied")
        };
        let face = super::super::FontFace::from_path(path, 0).unwrap();
        assert_eq!(face.borrowed_bytes(), &*first.bytes);
        assert_eq!(face.borrowed_bytes().as_ptr(), mapping.as_ptr());
        let owned = face.as_bytes();
        assert_eq!(&*owned, face.borrowed_bytes());
        assert!(Arc::ptr_eq(&owned, &second.bytes.export()));
        assert!(
            open(Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../fonts/Geist/Geist-VariableFont_wght.ttf"
            )))
            .is_none()
        );
    }
}
