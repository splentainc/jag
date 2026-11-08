//! Intern immutable font bytes without keeping unused font files alive.
use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::{Arc, Mutex, OnceLock, Weak},
};

// Weak pointers target this small owner, not the inline byte allocation. The
// last face releases the large buffer immediately, even before another load.
#[derive(Debug)]
pub(super) struct SharedData {
    pub bytes: FontData,
}
#[derive(Debug)]
pub(super) enum FontData {
    Owned(Arc<[u8]>),
    #[cfg(target_os = "macos")]
    SystemFile(memmap2::Mmap, OnceLock<Arc<[u8]>>),
}
impl std::ops::Deref for FontData {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Owned(bytes) => bytes,
            #[cfg(target_os = "macos")]
            Self::SystemFile(mapping, _) => mapping,
        }
    }
}
impl FontData {
    pub(super) fn export(&self) -> Arc<[u8]> {
        match self {
            Self::Owned(bytes) => bytes.clone(),
            #[cfg(target_os = "macos")]
            Self::SystemFile(mapping, copy) => copy.get_or_init(|| Arc::from(&mapping[..])).clone(),
        }
    }
}
type Pool = HashMap<(usize, u64), Vec<Weak<SharedData>>>;
static POOL: OnceLock<Mutex<Pool>> = OnceLock::new();

pub(super) fn share(data: impl AsRef<[u8]> + Into<Arc<[u8]>>) -> Arc<SharedData> {
    let bytes = data.as_ref();
    let mut hash = DefaultHasher::new();
    bytes.hash(&mut hash);
    let key = (bytes.len(), hash.finish());
    let mut pool = POOL.get_or_init(Mutex::default).lock().unwrap();
    pool.retain(|_, entries| {
        entries.retain(|data| data.strong_count() != 0);
        !entries.is_empty()
    });
    let entries = pool.entry(key).or_default();
    for existing in entries.iter().filter_map(Weak::upgrade) {
        // Hash collisions must never change the selected font's bytes.
        if &*existing.bytes == bytes {
            return existing;
        }
    }
    let shared = Arc::new(SharedData {
        bytes: FontData::Owned(data.into()),
    });
    entries.push(Arc::downgrade(&shared));
    shared
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::FontFace;

    fn font() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fonts/Geist/Geist-VariableFont_wght.ttf"
        ))
        .unwrap()
    }

    #[test]
    fn independently_loaded_faces_share_bytes_and_keep_metrics() {
        let bytes = font();
        let first = FontFace::from_vec(bytes.clone(), 0).unwrap();
        let second = FontFace::from_bytes(Arc::from(bytes.clone()), 0).unwrap();
        assert!(Arc::ptr_eq(&first.as_bytes(), &second.as_bytes()));
        assert_eq!(first.metrics().units_per_em, second.metrics().units_per_em);
        let mut changed = bytes;
        changed.extend_from_slice(b"distinct trailing font metadata");
        let distinct = FontFace::from_vec(changed, 0).unwrap();
        assert!(!Arc::ptr_eq(&first.as_bytes(), &distinct.as_bytes()));
        assert_eq!(
            first.metrics().units_per_em,
            distinct.metrics().units_per_em
        );
    }

    #[test]
    fn shared_font_storage_is_released_with_its_last_face() {
        let mut bytes = font();
        bytes.extend_from_slice(b"weak font lifetime regression");
        let first = FontFace::from_vec(bytes.clone(), 0).unwrap();
        let storage = Arc::downgrade(&first.as_bytes());
        let second = FontFace::from_vec(bytes, 0).unwrap();
        drop(first);
        assert!(storage.upgrade().is_some());
        drop(second);
        assert!(storage.upgrade().is_none());
    }
}
