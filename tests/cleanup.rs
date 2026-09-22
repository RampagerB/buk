//! Cleanup behavior tests: days / keep (default 5) / orphan criteria.

use std::fs;
use std::path::{Path, PathBuf};

use buk::list::collect_backups;
use buk::naming::mirror_of;
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

/// Directory holding the manual versions of `src` inside `dest` (mirrored layout).
fn versions_dir(dest: &Path, src: &Path) -> PathBuf {
    let dir = dest.join(mirror_of(src).unwrap());
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Write `count` fake single-file backup versions (`note.txt~v01` …) for `src`.
fn make_versions(dir: &Path, count: usize) -> Vec<String> {
    (1..=count)
        .map(|i| {
            let suffix = format!("v{i:02}");
            fs::write(dir.join(format!("note.txt~{suffix}")), b"x").unwrap();
            suffix
        })
        .collect()
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn days_keeps_recent_removes_stale() {
    let root = tmp("days");
    let src = root.join("note.txt");
    fs::write(&src, b"x").unwrap();
    let dest = root.join("dest");

    let mut o = opt(src.clone());
    run(&opt(src.clone()), &dest).unwrap(); // real backup first
    assert_eq!(collect_backups(&src, &dest).unwrap().len(), 1);

    // Large window: artifact mtime (now) is newer than the cutoff → kept.
    o.cleanup = true;
    o.days = Some(3650);
    run(&o, &dest).unwrap();
    assert_eq!(collect_backups(&src, &dest).unwrap().len(), 1);

    // Zero-day window: cutoff = now → everything already backed up is stale → removed.
    o.days = Some(0);
    run(&o, &dest).unwrap();
    assert_eq!(collect_backups(&src, &dest).unwrap().len(), 0);
}

#[test]
fn keep_defaults_to_five() {
    let root = tmp("keep_default");
    let dest = root.join("dest");
    let src = root.join("note.txt");
    let dir = versions_dir(&dest, &src);
    make_versions(&dir, 7);

    let mut o = opt(src);
    o.cleanup = true;
    o.keep = Some(None); // bare --keep → default 5
    run(&o, &dest).unwrap();

    let left = names(&dir);
    assert_eq!(left.len(), 5, "expected 5 versions to survive, got {left:?}");
    assert_eq!(left[0], "note.txt~v03", "the two oldest versions are removed");
    assert_eq!(left[4], "note.txt~v07");
}

#[test]
fn keep_explicit_count() {
    let root = tmp("keep_two");
    let dest = root.join("dest");
    let src = root.join("note.txt");
    let dir = versions_dir(&dest, &src);
    make_versions(&dir, 4);

    let mut o = opt(src);
    o.cleanup = true;
    o.keep = Some(Some(2));
    run(&o, &dest).unwrap();

    assert_eq!(names(&dir), vec!["note.txt~v03".to_string(), "note.txt~v04".to_string()]);
}

#[test]
fn orphan_only_removes_when_source_missing() {
    let root = tmp("orphan");
    let src = root.join("note.txt");
    fs::write(&src, b"x").unwrap();
    let dest = root.join("dest");
    run(&opt(src.clone()), &dest).unwrap();

    // Source still exists → orphan cleanup is a no-op.
    let mut o = opt(src.clone());
    o.cleanup = true;
    o.orphan = true;
    run(&o, &dest).unwrap();
    assert_eq!(collect_backups(&src, &dest).unwrap().len(), 1);

    // Source gone → all its backups are removed.
    fs::remove_file(&src).unwrap();
    run(&o, &dest).unwrap();
    assert_eq!(collect_backups(&src, &dest).unwrap().len(), 0);
}

#[test]
fn cleanup_validation_errors() {
    let root = tmp("cleanup_validation");
    let src = root.join("note.txt");
    fs::write(&src, b"x").unwrap();
    let dest = root.join("dest");

    // -c without a criterion
    let mut o = opt(src.clone());
    o.cleanup = true;
    assert!(run(&o, &dest).is_err());

    // criterion without -c
    let mut o = opt(src.clone());
    o.days = Some(1);
    assert!(run(&o, &dest).is_err());

    // two criteria at once
    let mut o = opt(src.clone());
    o.cleanup = true;
    o.days = Some(1);
    o.orphan = true;
    assert!(run(&o, &dest).is_err());

    // cleanup + list
    let mut o = opt(src);
    o.cleanup = true;
    o.days = Some(1);
    o.list = true;
    assert!(run(&o, &dest).is_err());
}
