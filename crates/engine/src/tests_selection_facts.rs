//! [`Session::selection_facts`] is remembered between frames: it has to follow every edit of the
//! catalog and every change of the selection, as asking each photo again would.

use lightcraft_catalog::{Op, Photo, PhotoId, Source};
use lightcraft_develop::ProcessVersion;
use serde_json::json;

use crate::{SelectionFacts, Session};

fn library() -> (Session, Vec<PhotoId>) {
    let mut s = Session::new();
    let ids: Vec<PhotoId> = (1..=6u64)
        .map(|i| {
            let p = Photo::new(PhotoId(i), Source::File { path: format!("/p/{i}.jpg") }, &format!("{i}.jpg"), "JPG", 60, 40, "2026-01-01");
            s.catalog.apply(Op::AddPhoto { photo: Box::new(p) }).unwrap();
            PhotoId(i)
        })
        .collect();
    (s, ids)
}

fn select(s: &mut Session, ids: &[PhotoId]) {
    s.execute("library.select", &json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>()})).unwrap();
}

/// What the facts are when every selected photo is asked again.
fn asked(s: &Session) -> SelectionFacts {
    let sel = &s.selection;
    let targets = if sel.ids.is_empty() { sel.active.as_slice() } else { sel.ids.as_slice() };
    SelectionFacts {
        deleted: sel.ids.iter().chain(&sel.active).any(|id| s.catalog.photo(*id).is_some_and(|p| p.deleted)),
        outdated: targets.iter().any(|id| s.catalog.photo(*id).is_some_and(|p| p.develop.process.is_outdated())),
        stacked: targets.iter().any(|id| s.catalog.stack_of(*id).is_some()),
    }
}

#[test]
fn the_facts_follow_edits_and_the_selection() {
    let (mut s, ids) = library();
    let check = |s: &Session, want: SelectionFacts, why: &str| {
        assert_eq!(s.selection_facts(), want, "{why}");
        assert_eq!(s.selection_facts(), asked(s), "{why}: as if asked again");
    };
    select(&mut s, &ids[..2]);
    check(&s, SelectionFacts::default(), "nothing to say");

    s.execute("stack.group", &json!({"ids": [ids[1].0, ids[2].0]})).unwrap();
    check(&s, SelectionFacts { stacked: true, ..Default::default() }, "the stack is selected");
    select(&mut s, &ids[..2]);
    check(&s, SelectionFacts { stacked: true, ..Default::default() }, "a selected photo is stacked");

    let mut d = (*s.develop_of(ids[0]).unwrap()).clone();
    d.process = ProcessVersion::V1;
    s.set_develop(ids[0], d, "old process").unwrap();
    check(&s, SelectionFacts { stacked: true, outdated: true, ..Default::default() }, "a selected photo went back a process");

    select(&mut s, &ids[3..]);
    check(&s, SelectionFacts::default(), "another selection");

    s.execute("photo.delete", &json!({})).unwrap();
    assert_eq!(s.selection_facts(), asked(&s), "after Delete");
    // (the selection moved on: the deleted photos are picked as Recently Deleted would)
    s.selection.ids = vec![ids[3]];
    s.selection.active = Some(ids[3]);
    check(&s, SelectionFacts { deleted: true, ..Default::default() }, "a deleted photo is selected");

    // the same ids picked again, in another order: another selection
    select(&mut s, &[ids[2], ids[0]]);
    check(&s, SelectionFacts { stacked: true, outdated: true, ..Default::default() }, "photos picked again");
    s.selection.active = Some(ids[5]);
    assert_eq!(s.selection_facts(), asked(&s), "the active photo changed without a command");
}
