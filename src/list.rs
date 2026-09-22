//! List operations (`-l`): find backup artifacts for an original path.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use crate::fsutil::{format_time, walk_files};
use crate::naming::{mirror_of, SEP};

pub fn cmd_list(src: &Path, dest: &Path) -> Result<(), String> {
    let backups = collect_backups(src, dest)?;
    if backups.is_empty() {
        println!("no backups found for {}", src.display());
        return Ok(());
    }
    for p in &backups {
        match fs::metadata(p) {
            Ok(m) => println!("{}\t{}\t{}", p.display(), m.len(), format_time(m.modified())),
            Err(e) => println!("{}\t?\terror: {e}", p.display()),
        }
    }
    Ok(())
}

/// Find all backup artifacts belonging to the original path `src`.
/// Works even if `src` no longer exists (only its mirrored name is used).
pub fn collect_backups(src: &Path, dest: &Path) -> Result<Vec<PathBuf>, String> {
    let mirror = mirror_of(src)?;
    let name = mirror
        .file_name()
        .ok_or_else(|| format!("{} has no file name", src.display()))?;
    let name_str =
        name.to_str().ok_or_else(|| format!("{} is not valid UTF-8", src.display()))?;
    let prefix = format!("{name_str}{SEP}");
    let mut out = Vec::new();

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
                .and_then(OsStr::to_str)
                .is_some_and(|f| f.starts_with(&prefix));
            if !matches {
                continue;
            }
            if path.is_dir() {
                // Whole-folder backup: list the files inside it.
                out.extend(walk_files(&path)?.into_iter().map(|(p, _)| p));
            } else {
                out.push(path);
            }
        }
    }

    // Recursive backup layout lives at the exact mirror path.
    let per_file_layout = dest.join(&mirror);
    if per_file_layout.is_dir() {
        out.extend(walk_files(&per_file_layout)?.into_iter().map(|(p, _)| p));
    }

    out.sort();
    out.dedup();
    Ok(out)
}
