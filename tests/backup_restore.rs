//! End-to-end behavior tests: backup → list → restore roundtrips.

use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;

use buk::list::collect_backups;
use buk::naming::{mirror_of, SEP};
use buk::{run, Opt, DEFAULT_TEMPLATE};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("buk_test_{}_{}", std::process::id(), tag));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn opt(path: PathBuf) -> Opt {
    Opt {
        path,
        recursive: false,
        restore: false,
        at: None,
        template: Some(DEFAULT_TEMPLATE.to_string()),
        list: false,
        cleanup: false,
        days: None,
        keep: None,
        orphan: false,
    }
}

fn suffix_of(name: &OsStr) -> String {
    name.to_str().unwrap().rsplit_once(SEP).unwrap().1.to_string()
}

#[test]
fn multi_file_backup_shares_one_suffix() {
    let root = tmp("suffix");
    let src = root.join("data");
    fs::create_dir_all(src.join("sub")).unwrap();
    fs::write(src.join("a.txt"), b"a").unwrap();
    fs::write(src.join("sub/b.txt"), b"b").unwrap();
    let dest = root.join("dest");

    let mut o = opt(src.clone());
    o.recursive = true;
    run(&o, &dest).unwrap();

    let found = collect_backups(&src, &dest).unwrap();
    assert_eq!(found.len(), 2, "both sub-files should be backed up");
    let suffixes: Vec<String> =
        found.iter().map(|p| suffix_of(p.file_name().unwrap())).collect();
    assert!(suffixes.iter().all(|s| !s.is_empty()));
    assert_eq!(suffixes[0], suffixes[1], "one suffix per execution");
}

#[test]
fn file_backup_list_restore_roundtrip() {
    let root = tmp("file");
    let src = root.join("note.txt");
    fs::write(&src, b"original").unwrap();
    let dest = root.join("dest");

    run(&opt(src.clone()), &dest).unwrap();
    assert_eq!(collect_backups(&src, &dest).unwrap().len(), 1);

    fs::write(&src, b"changed").unwrap();
    let mut o = opt(src.clone());
    o.restore = true;
    run(&o, &dest).unwrap();
    assert_eq!(fs::read(&src).unwrap(), b"original");
}

#[test]
fn dir_backup_restore_roundtrip() {
    let root = tmp("dir");
    let src = root.join("docs");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("f.txt"), b"data").unwrap();
    let dest = root.join("dest");

    run(&opt(src.clone()), &dest).unwrap();
    assert_eq!(collect_backups(&src, &dest).unwrap().len(), 1);

    fs::remove_dir_all(&src).unwrap();
    let mut o = opt(src.clone());
    o.restore = true;
    run(&o, &dest).unwrap();
    assert_eq!(fs::read(src.join("f.txt")).unwrap(), b"data");
}

#[test]
fn list_works_after_source_deleted() {
    let root = tmp("list");
    let src = root.join("gone.txt");
    fs::write(&src, b"x").unwrap();
    let dest = root.join("dest");
    run(&opt(src.clone()), &dest).unwrap();
    fs::remove_file(&src).unwrap();
    assert_eq!(collect_backups(&src, &dest).unwrap().len(), 1);
}

#[test]
fn restore_missing_backup_errors() {
    let root = tmp("nobackup");
    let src = root.join("nothing.txt");
    fs::write(&src, b"x").unwrap();
    let dest = root.join("dest");
    fs::create_dir_all(&dest).unwrap();
    let mut o = opt(src);
    o.restore = true;
    assert!(run(&o, &dest).is_err());
}

#[test]
fn list_and_restore_conflict_errors() {
    let root = tmp("conflict");
    let src = root.join("x.txt");
    fs::write(&src, b"x").unwrap();
    let mut o = opt(src);
    o.list = true;
    o.restore = true;
    assert!(run(&o, &root.join("dest")).is_err());
}

#[test]
fn restore_at_picks_nearest_version() {
    let root = tmp("restore_at");
    let src = root.join("note.txt");
    let dest = root.join("dest");
    let mirror = mirror_of(&src).unwrap();
    let vdir = dest.join(mirror.parent().unwrap());
    fs::create_dir_all(&vdir).unwrap();
    fs::write(vdir.join("note.txt~20260101_120000"), b"jan").unwrap();
    fs::write(vdir.join("note.txt~20260301_120000"), b"mar").unwrap();
    fs::write(vdir.join("note.txt~20260501_120000"), b"may").unwrap();
    fs::write(&src, b"current").unwrap();

    let mut o = opt(src.clone());
    o.restore = true;

    // Date-only target (midnight Mar 15) → Mar 1 is nearest.
    o.at = Some("2026-03-15".to_string());
    run(&o, &dest).unwrap();
    assert_eq!(fs::read(&src).unwrap(), b"mar");

    // Exact timestamp → that version.
    o.at = Some("2026-05-01 12:00:00".to_string());
    run(&o, &dest).unwrap();
    assert_eq!(fs::read(&src).unwrap(), b"may");

    // Input in the template's own format also parses.
    o.at = Some("20260101_120000".to_string());
    run(&o, &dest).unwrap();
    assert_eq!(fs::read(&src).unwrap(), b"jan");
}

#[test]
fn restore_at_validation_errors() {
    let root = tmp("restore_at_err");
    let src = root.join("note.txt");
    fs::write(&src, b"x").unwrap();
    let dest = root.join("dest");

    // Unparseable date-time string.
    let mut o = opt(src.clone());
    o.restore = true;
    o.at = Some("not-a-date".to_string());
    assert!(run(&o, &dest).is_err());

    // --at without --restore.
    let mut o = opt(src.clone());
    o.at = Some("2026-01-01".to_string());
    assert!(run(&o, &dest).is_err());

    // --at but no candidate suffix is parseable with the active template.
    let mirror = mirror_of(&src).unwrap();
    let vdir = dest.join(mirror.parent().unwrap());
    fs::create_dir_all(&vdir).unwrap();
    fs::write(vdir.join("note.txt~garbage"), b"x").unwrap();
    let mut o = opt(src);
    o.restore = true;
    o.at = Some("2026-01-01".to_string());
    assert!(run(&o, &dest).is_err());
}

#[test]
fn same_basename_sources_do_not_collide() {
    let root = tmp("collision");
    let a = root.join("a");
    let b = root.join("b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    let fa = a.join("dup.txt");
    let fb = b.join("dup.txt");
    fs::write(&fa, b"from-a").unwrap();
    fs::write(&fb, b"from-b").unwrap();
    let dest = root.join("dest");

    run(&opt(fa.clone()), &dest).unwrap();
    run(&opt(fb.clone()), &dest).unwrap();

    // Mirrored layout: two distinct artifact series.
    let ba = collect_backups(&fa, &dest).unwrap();
    let bb = collect_backups(&fb, &dest).unwrap();
    assert_eq!(ba.len(), 1);
    assert_eq!(bb.len(), 1);
    assert_ne!(ba[0], bb[0], "same-basename sources must get separate artifacts");

    // Each source restores its own content, not the other's.
    fs::write(&fa, b"tampered-a").unwrap();
    fs::write(&fb, b"tampered-b").unwrap();
    let mut o = opt(fa.clone());
    o.restore = true;
    run(&o, &dest).unwrap();
    let mut o = opt(fb.clone());
    o.restore = true;
    run(&o, &dest).unwrap();
    assert_eq!(fs::read(&fa).unwrap(), b"from-a");
    assert_eq!(fs::read(&fb).unwrap(), b"from-b");
}
