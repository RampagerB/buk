//! Restore operations (`-s`): always takes the *original* path.
//!
//! Without `--at`, the newest version wins (lexicographically greatest
//! suffix — the same rule cleanup uses). With `--at WHEN`, the version whose
//! suffix timestamp is closest to `WHEN` wins; ties prefer the greater suffix.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{NaiveDate, NaiveDateTime};

use crate::fsutil::{copy_one, parallel_copy, walk_files};
use crate::naming::{mirror_of, strip_rel_suffix, SEP};

/// Restore the latest (or `--at`-nearest) backups for the original path `src`.
/// `suffix_template` is only used to parse candidate suffixes when `at` is set.
pub fn restore(
    src: &Path,
    dest: &Path,
    at: Option<NaiveDateTime>,
    suffix_template: &str,
) -> Result<(), String> {
    let mirror = mirror_of(src)?;
    let name_str = mirror
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| format!("{} is not valid UTF-8", src.display()))?
        .to_string();
    let prefix = format!("{name_str}{SEP}");

    let mut flat_files: Vec<PathBuf> = Vec::new();
    let mut flat_dirs: Vec<PathBuf> = Vec::new();
    let scoped = dest.join(mirror.parent().unwrap_or(Path::new("")));
    if scoped.is_dir() {
        let rd =
            fs::read_dir(&scoped).map_err(|e| format!("cannot read {}: {e}", scoped.display()))?;
        let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort();
        for path in entries {
            let matches = path
                .file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|f| f.starts_with(&prefix));
            if !matches {
                continue;
            }
            if path.is_dir() {
                flat_dirs.push(path);
            } else {
                flat_files.push(path);
            }
        }
    }
    let per_file_layout = dest.join(&mirror);

    let src_missing = !src.exists();
    let src_is_dir = src.is_dir();

    if src_missing || src_is_dir {
        // Preferred: whole-folder backup (dest/<mirror>~<suffix>/).
        if let Some(best) = pick(flat_dirs, at, suffix_template)? {
            let jobs: Vec<(PathBuf, PathBuf)> = walk_files(&best)?
                .into_iter()
                .map(|(p, rel)| (p, src.join(rel)))
                .collect();
            parallel_copy(&jobs)?;
            println!(
                "restored {} file(s) from {} -> {}",
                jobs.len(),
                best.display(),
                src.display()
            );
            return Ok(());
        }
        // Recursive backup layout (dest/<mirror>/...): each file restored from
        // its own nearest/newest version.
        if per_file_layout.is_dir() {
            let mut groups: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
            for (p, rel) in walk_files(&per_file_layout)? {
                groups.entry(strip_rel_suffix(&rel)).or_default().push(p);
            }
            let mut jobs: Vec<(PathBuf, PathBuf)> = Vec::new();
            for (rel, versions) in &groups {
                if let Some(best) = pick(versions.clone(), at, suffix_template)? {
                    jobs.push((best, src.join(rel)));
                }
            }
            if jobs.is_empty() {
                return Err(format!("no backup found for {}", src.display()));
            }
            parallel_copy(&jobs)?;
            println!(
                "restored {} file(s) from {} -> {}",
                jobs.len(),
                per_file_layout.display(),
                src.display()
            );
            return Ok(());
        }
    }

    if src_missing || !src_is_dir {
        // Single-file backup (dest/<mirror>/<name>~<suffix>).
        if let Some(best) = pick(flat_files, at, suffix_template)? {
            copy_one(&best, src)?;
            println!("restored {} -> {}", best.display(), src.display());
            return Ok(());
        }
    }

    Err(format!("no backup found for {}", src.display()))
}

/// Choose among candidate backup artifacts: nearest suffix timestamp to `at`
/// when given, otherwise the lexicographically greatest name ("latest").
fn pick(
    candidates: Vec<PathBuf>,
    at: Option<NaiveDateTime>,
    suffix_template: &str,
) -> Result<Option<PathBuf>, String> {
    if candidates.is_empty() {
        return Ok(None);
    }
    let Some(target) = at else {
        return Ok(candidates.into_iter().max());
    };

    let total = candidates.len();
    let mut best: Option<(u64, PathBuf)> = None;
    for c in candidates {
        let time = c
            .file_name()
            .and_then(OsStr::to_str)
            .and_then(|n| n.rsplit_once(SEP))
            .map(|(_, suffix)| suffix)
            .and_then(|suffix| parse_when(suffix, suffix_template));
        let Some(time) = time else { continue }; // unparseable suffix: cannot rank it
        let dist = (time - target).num_seconds().unsigned_abs();
        let better = match &best {
            None => true,
            Some((d, p)) => dist < *d || (dist == *d && &c > p), // tie → greater suffix
        };
        if better {
            best = Some((dist, c));
        }
    }
    match best {
        Some((_, p)) => Ok(Some(p)),
        None => Err(format!(
            "--at: none of the {total} backup candidate(s) has a date-time suffix \
             parseable with template {suffix_template:?}"
        )),
    }
}

/// Parse a user-supplied date-time (or a backup suffix) into a local
/// wall-clock `NaiveDateTime`. Tries `template` first, then common formats;
/// date-only values become midnight.
pub fn parse_target_datetime(s: &str, template: &str) -> Result<NaiveDateTime, String> {
    parse_when(s, template).ok_or_else(|| {
        format!(
            "cannot parse date-time {s:?}; examples: \"2026-09-20 14:30\", \
             \"2026-09-20 14:30:05\", \"2026-09-20\" (midnight), \"20260920_143000\""
        )
    })
}

fn parse_when(s: &str, template: &str) -> Option<NaiveDateTime> {
    const DT_FORMATS: &[&str] = &[
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y/%m/%d %H:%M:%S",
        "%Y/%m/%d %H:%M",
        "%Y%m%d_%H%M%S",
        "%Y-%m-%dT%H:%M:%S",
    ];
    const DATE_FORMATS: &[&str] = &["%Y-%m-%d", "%Y%m%d", "%Y/%m/%d"];

    // The active suffix template comes first (it is what the names use).
    for f in std::iter::once(template).chain(DT_FORMATS.iter().copied()) {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, f) {
            return Some(dt);
        }
    }
    for f in std::iter::once(template).chain(DATE_FORMATS.iter().copied()) {
        if let Ok(d) = NaiveDate::parse_from_str(s, f) {
            return d.and_hms_opt(0, 0, 0);
        }
    }
    None
}
