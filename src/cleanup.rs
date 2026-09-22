//! Cleanup operations (`-c`): retention by age, version count, or orphans.
//!
//! All criteria operate on the backup artifacts belonging to the **given
//! original path** (same path semantics as `--list`/`--restore`); the source
//! itself does not need to exist (except `Orphan`, which requires it gone).
//!
//! Age is based on artifact mtime (≈ backup creation time), versions are
//! ranked by suffix string — the same "latest = lexicographically greatest"
//! rule `restore` uses.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::fsutil::walk_files;
use crate::naming::{mirror_of, strip_rel_suffix, SEP};

/// Versions kept by [`Criterion::Keep`] when `--keep` is given without a value.
pub const DEFAULT_KEEP: u64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Criterion {
    /// Delete backups older than N days (by artifact mtime).
    Days(u64),
    /// Keep only the newest N versions per item, delete the rest.
    Keep(u64),
    /// Delete all backups belonging to a source path that no longer exists.
    Orphan,
}

/// Backup artifacts of one original path.
struct Artifacts {
    /// Direct entries of `dest` matching `<name>~<suffix>` (files or whole-dir backups).
    flat: Vec<PathBuf>,
    /// Per-file artifacts (absolute, relative) under the recursive layout `dest/<name>/`.
    per_file: Vec<(PathBuf, PathBuf)>,
    /// The recursive layout root `dest/<name>/`, if it exists.
    layout: Option<PathBuf>,
}

fn collect_artifacts(src: &Path, dest: &Path) -> Result<Artifacts, String> {
    let mirror = mirror_of(src)?;
    let name_str = mirror
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| format!("{} is not valid UTF-8", src.display()))?;
    let prefix = format!("{name_str}{SEP}");

    let mut flat = Vec::new();
    let mut per_file = Vec::new();
    let mut layout = None;

    // Flat artifacts live next to the mirror, i.e. in dest/<mirror parent>/.
    let scoped = dest.join(mirror.parent().unwrap_or(Path::new("")));
    if scoped.is_dir() {
        let rd =
            fs::read_dir(&scoped).map_err(|e| format!("cannot read {}: {e}", scoped.display()))?;
        let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort();
        for path in entries {
            let matches = path
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|f| f.starts_with(&prefix));
            if matches {
                flat.push(path);
            }
        }
    }

    // Recursive backup layout lives at the exact mirror path.
    let root = dest.join(&mirror);
    if root.is_dir() {
        per_file = walk_files(&root)?;
        layout = Some(root);
    }

    Ok(Artifacts { flat, per_file, layout })
}

pub fn cleanup(src: &Path, dest: &Path, criterion: Criterion) -> Result<(), String> {
    let art = collect_artifacts(src, dest)?;
    let removed = match criterion {
        Criterion::Days(d) => cleanup_days(&art, d)?,
        Criterion::Keep(k) => cleanup_keep(&art, k)?,
        Criterion::Orphan => cleanup_orphan(src, &art)?,
    };
    println!("cleanup: removed {removed} backup artifact(s)");
    Ok(())
}

fn cleanup_days(art: &Artifacts, days: u64) -> Result<usize, String> {
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(days.saturating_mul(86_400)))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let is_stale = |p: &Path| -> Result<bool, String> {
        let m = fs::metadata(p).map_err(|e| format!("cannot stat {}: {e}", p.display()))?;
        Ok(m.modified().is_ok_and(|t| t < cutoff))
    };

    let mut removed = 0;
    let mut touched_dirs: Vec<PathBuf> = Vec::new();
    for p in &art.flat {
        if is_stale(p)? {
            remove_artifact(p)?;
            removed += 1;
        }
    }
    for (p, _) in &art.per_file {
        if is_stale(p)? {
            if let Some(parent) = p.parent() {
                touched_dirs.push(parent.to_path_buf());
            }
            remove_artifact(p)?;
            removed += 1;
        }
    }
    prune_empty_dirs(&touched_dirs, art.layout.as_deref());
    Ok(removed)
}

fn cleanup_keep(art: &Artifacts, keep: u64) -> Result<usize, String> {
    let mut removed = 0;

    // Whole-dir backups and single-file backups each form their own version series.
    for is_dir in [false, true] {
        let mut versions: Vec<PathBuf> = art
            .flat
            .iter()
            .filter(|p| p.is_dir() == is_dir)
            .cloned()
            .collect();
        removed += trim_versions(&mut versions, keep)?.len();
    }

    // Recursive layout: versions are grouped by original (suffix-stripped) relative path.
    let mut groups: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for (abs, rel) in &art.per_file {
        groups.entry(strip_rel_suffix(rel)).or_default().push(abs.clone());
    }
    let mut touched_dirs: Vec<PathBuf> = Vec::new();
    for versions in groups.values_mut() {
        let gone = trim_versions(versions, keep)?;
        for p in &gone {
            if let Some(parent) = p.parent() {
                touched_dirs.push(parent.to_path_buf());
            }
        }
        removed += gone.len();
    }
    prune_empty_dirs(&touched_dirs, art.layout.as_deref());
    Ok(removed)
}

fn cleanup_orphan(src: &Path, art: &Artifacts) -> Result<usize, String> {
    if src.exists() {
        println!("source {} still exists; no orphan backups to remove", src.display());
        return Ok(0);
    }
    let mut removed = 0;
    for p in &art.flat {
        remove_artifact(p)?;
        removed += 1;
    }
    if let Some(root) = &art.layout {
        remove_artifact(root)?;
        removed += 1;
    }
    Ok(removed)
}

/// Sort versions ascending (oldest = lexicographically smallest name) and
/// remove all but the newest `keep`. Returns the removed paths.
fn trim_versions(versions: &mut Vec<PathBuf>, keep: u64) -> Result<Vec<PathBuf>, String> {
    let keep = usize::try_from(keep).unwrap_or(usize::MAX);
    if versions.len() <= keep {
        return Ok(Vec::new());
    }
    versions.sort();
    let doomed = versions.drain(..versions.len() - keep).collect::<Vec<_>>();
    for p in &doomed {
        remove_artifact(p)?;
    }
    Ok(doomed)
}

fn remove_artifact(path: &Path) -> Result<(), String> {
    let meta =
        fs::symlink_metadata(path).map_err(|e| format!("cannot stat {}: {e}", path.display()))?;
    let res = if meta.is_dir() { fs::remove_dir_all(path) } else { fs::remove_file(path) };
    res.map_err(|e| format!("cannot remove {}: {e}", path.display()))?;
    println!("removed backup {}", path.display());
    Ok(())
}

/// Best-effort removal of directories (deepest first) left empty by deletions.
fn prune_empty(dirs: &[PathBuf]) {
    let mut sorted = dirs.to_vec();
    sorted.sort();
    sorted.dedup();
    sorted.reverse(); // deepest paths first: "a/b/c" > "a/b"
    for d in sorted {
        let _ = fs::remove_dir(d);
    }
}

fn prune_empty_dirs(dirs: &[PathBuf], root: Option<&Path>) {
    prune_empty(dirs);
    if let Some(r) = root {
        let _ = fs::remove_dir(r); // only succeeds if now empty
    }
}
