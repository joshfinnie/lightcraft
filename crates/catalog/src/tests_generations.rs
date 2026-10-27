//! A damaged `catalog.snap` falls back to the snapshot before it, a bit flip that still parses
//! is caught by the checksum, and an upgrade keeps what older builds can open.

use crate::journal::{GENERATIONS, LOG, SNAPSHOT, VERSION, encode_record};
use crate::*;

fn add_photo(j: &mut Journal, c: &mut Catalog, name: &str) {
    let id = c.alloc_photo_id();
    let op = Op::AddPhoto { photo: Box::new(Photo::new(id, Source::File { path: format!("/p/{name}") }, name, "JPEG", 4, 3, "2026-01-01")) };
    c.apply(op.clone()).unwrap();
    j.append(&[op]).unwrap();
}

fn names(c: &Catalog) -> Vec<String> {
    let mut n: Vec<String> = c.photos().map(|p| p.file_name.clone()).collect();
    n.sort();
    n
}

/// Three compactions: generation 1 holds the state of the second, generation 2 of the first.
fn library() -> (MemStore, Catalog) {
    let m = MemStore::new();
    let (mut j, mut c, _) = Journal::open(Box::new(m.clone())).unwrap();
    for name in ["first.jpg", "second.jpg", "third.jpg"] {
        add_photo(&mut j, &mut c, name);
        j.snapshot(&c).unwrap();
    }
    add_photo(&mut j, &mut c, "after.jpg");
    (m, c)
}

#[test]
fn snapshots_carry_a_checksum_and_the_previous_two_are_kept() {
    let (m, c) = library();
    let snap = String::from_utf8(m.get(SNAPSHOT).unwrap()).unwrap();
    assert!(snap.trim_end().ends_with('}') && snap.contains(",\"crc\":"), "{snap}");
    let (_, loaded, r) = Journal::open(Box::new(m.clone())).unwrap();
    assert_eq!(names(&loaded), names(&c));
    assert!(r.restored.is_none() && !r.checksum_mismatch);
    let gen1: serde_json::Value = serde_json::from_slice(&m.get(GENERATIONS[0]).unwrap()).unwrap();
    let gen2: serde_json::Value = serde_json::from_slice(&m.get(GENERATIONS[1]).unwrap()).unwrap();
    assert_eq!(gen1["catalog"]["photos"].as_object().map(|p| p.len()), Some(2));
    assert_eq!(gen2["catalog"]["photos"].as_object().map(|p| p.len()), Some(1));
}

/// A flipped byte inside a file name still parses as JSON: only the checksum catches it.
#[test]
fn a_snapshot_that_fails_its_checksum_falls_back_to_the_previous_one() {
    let (m, _) = library();
    let good = m.get(SNAPSHOT).unwrap();
    let flipped = String::from_utf8(good.clone()).unwrap().replacen("third.jpg", "thirX.jpg", 1).into_bytes();
    m.set(SNAPSHOT, flipped.clone());
    let (_, c, r) = Journal::open(Box::new(m.clone())).unwrap();
    let why = r.restored.expect("restored from a generation");
    assert!(why.contains("checksum") && why.contains("catalog.snap.1"), "{why}");
    // the state of the previous compaction; the damaged file and the log are kept
    assert_eq!(names(&c), ["first.jpg", "second.jpg"]);
    assert_eq!(m.get(&format!("{SNAPSHOT}.damaged-3")).unwrap(), flipped);
    assert!(m.get(&format!("{LOG}.before-restore-3")).is_some());
    // the next open is clean
    let (_, c2, r2) = Journal::open(Box::new(m)).unwrap();
    assert!(r2.restored.is_none());
    assert_eq!(names(&c2), names(&c));
}

#[test]
fn an_unreadable_snapshot_falls_back_instead_of_refusing_the_library() {
    let (m, _) = library();
    let good = m.get(SNAPSHOT).unwrap();
    m.set(SNAPSHOT, good[..good.len() / 2].to_vec());
    let (_, c, r) = Journal::open(Box::new(m.clone())).unwrap();
    assert!(r.restored.is_some());
    assert_eq!(names(&c), ["first.jpg", "second.jpg"]);
    // with no generation to fall back to, it is refused as before, and nothing is changed
    let lone = MemStore::new();
    lone.set(SNAPSHOT, good[..good.len() / 2].to_vec());
    assert!(matches!(Journal::open(Box::new(lone.clone())), Err(CatalogError::Corrupt(_))));
    assert_eq!(lone.get(SNAPSHOT).unwrap(), good[..good.len() / 2]);
}

/// Without a sound generation, a snapshot that parses but fails its checksum is still opened
/// (most of it is right) and reported.
#[test]
fn a_checksum_mismatch_with_nothing_to_fall_back_to_is_loaded_and_reported() {
    let m = MemStore::new();
    let (mut j, mut c, _) = Journal::open(Box::new(m.clone())).unwrap();
    add_photo(&mut j, &mut c, "only.jpg");
    j.snapshot(&c).unwrap();
    drop(j);
    for g in GENERATIONS {
        m.set(g, b"not a snapshot".to_vec());
    }
    let snap = String::from_utf8(m.get(SNAPSHOT).unwrap()).unwrap().replace("only.jpg", "onlY.jpg");
    m.set(SNAPSHOT, snap.into_bytes());
    let (_, c, r) = Journal::open(Box::new(m)).unwrap();
    assert!(r.checksum_mismatch && r.restored.is_none());
    assert_eq!(names(&c), ["onlY.jpg"]);
}

/// Opening an older format keeps its snapshot and log under `.v<old>`, so the build that wrote
/// them can still open a copy of the library.
#[test]
fn an_upgrade_keeps_the_old_format_files() {
    let mut c = Catalog::new();
    let id = c.alloc_photo_id();
    let add = Op::AddPhoto { photo: Box::new(Photo::new(id, Source::File { path: "/p/a.jpg".into() }, "a.jpg", "JPEG", 4, 3, "2026-01-01")) };
    let m = MemStore::new();
    let old_snap =
        format!("{{\"format\":\"lightcraft-catalog\",\"version\":{},\"seq\":0,\"catalog\":{}}}\n", VERSION - 1, c.to_snapshot()).into_bytes();
    let old_log = format!("{}\n", encode_record(1, &add)).into_bytes();
    m.set(SNAPSHOT, old_snap.clone());
    m.set(LOG, old_log.clone());
    c.apply(add).unwrap();
    let (_, loaded, r) = Journal::open(Box::new(m.clone())).unwrap();
    assert_eq!(r.upgraded_from, Some(VERSION - 1));
    assert_eq!(names(&loaded), names(&c));
    assert_eq!(m.get(&format!("{SNAPSHOT}.v{}", VERSION - 1)).unwrap(), old_snap);
    assert_eq!(m.get(&format!("{LOG}.v{}", VERSION - 1)).unwrap(), old_log);
}

/// On disk the generations are second names of the old snapshots, never partial files.
#[test]
fn fs_store_keeps_generations() {
    let dir = std::env::temp_dir().join(format!("lc-generations-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (mut j, mut c, _) = Journal::open(Box::new(FsStore::open(&dir).unwrap())).unwrap();
    for name in ["a.jpg", "b.jpg"] {
        add_photo(&mut j, &mut c, name);
        j.snapshot(&c).unwrap();
    }
    drop(j);
    let gen1 = std::fs::read(dir.join(GENERATIONS[0])).unwrap();
    assert!(String::from_utf8_lossy(&gen1).contains("a.jpg") && !String::from_utf8_lossy(&gen1).contains("b.jpg"));
    std::fs::write(dir.join(SNAPSHOT), b"{").unwrap();
    let (_, c2, r) = Journal::open(Box::new(FsStore::open(&dir).unwrap())).unwrap();
    assert!(r.restored.is_some());
    assert_eq!(names(&c2), ["a.jpg"]);
    let _ = std::fs::remove_dir_all(&dir);
}
