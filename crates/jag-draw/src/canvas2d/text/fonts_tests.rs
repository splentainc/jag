use super::*;

const QUOTA: usize = 64 * 1024 * 1024;

#[test]
fn a_face_over_quota_is_refused_once_and_remembered() {
    let mut fonts = Fonts::default();
    let Some(id) = fonts.db.faces().next().map(|face| face.id) else {
        return; // no fonts on this machine
    };
    fonts.bytes = QUOTA;
    assert!(fonts.face(id).is_none());
    assert!(fonts.rejected.contains(&id));
    assert!(fonts.cache.is_empty());
    assert_eq!(fonts.bytes, QUOTA, "a refused face must not use quota");
}

#[test]
fn removing_a_font_frees_quota_so_refused_faces_are_tried_again() {
    let mut fonts = Fonts::default();
    let Some(id) = fonts.db.faces().next().map(|face| face.id) else {
        return;
    };
    fonts.bytes = QUOTA;
    assert!(fonts.face(id).is_none());
    fonts.remove("no such family", 400, false);
    assert!(fonts.rejected.is_empty());
}
