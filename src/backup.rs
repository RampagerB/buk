//! Backup operations (default mode).
//!
//! Artifacts are keyed by the mirrored absolute source path, e.g. source
//! `/home/u/notes.txt` → `dest/home/u/notes.txt~<suffix>`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::fsutil::{copy_one, parallel_copy, walk_files};
use crate::naming::{file_name, mirror_of, suffix_name};

/// Back up `src` (absolute, normalized) into `dest` using a suffix computed
/// once per execution.
pub fn backup(src: &Path, dest: &Path, suffix: &str, recursive: bool) -> Result<(), String> {
    let meta = fs::metadata(src).map_err(|e| format!("cannot access {}: {e}", src.display()))?;
    fs::create_dir_all(dest).map_err(|e| format!("cannot create {}: {e}", dest.display()))?;
    let mirror = mirror_of(src)?;
    let name = file_name(&mirror)?;

    if meta.is_file() {
        let parent_mirror = mirror.parent().unwrap_or(Path::new(""));
        let dst = dest.join(parent_mirror).join(suffix_name(name, suffix));
        copy_one(src, &dst)?;
        println!("backed up {} -> {}", src.display(), dst.display());
        return Ok(());
    }

    let files = walk_files(src)?;
    if !recursive {
        // Whole folder copied as-is under one suffixed directory.
        let mut target_mirror = mirror.clone();
        target_mirror.set_file_name(suffix_name(name, suffix));
        let target = dest.join(target_mirror);
        // Create the mirrored parents (and the target itself, so an empty
        // source directory still produces a backup).
        fs::create_dir_all(&target)
            .map_err(|e| format!("cannot create {}: {e}", target.display()))?;
        let jobs: Vec<(PathBuf, PathBuf)> =
            files.iter().map(|(p, rel)| (p.clone(), target.join(rel))).collect();
        parallel_copy(&jobs)?;
        println!(
            "backed up directory {} -> {} ({} file(s))",
            src.display(),
            target.display(),
            jobs.len()
        );
    } else {
        // Each sub-file backed up individually, all with the same suffix.
        let base = dest.join(&mirror);
        fs::create_dir_all(&base).map_err(|e| format!("cannot create {}: {e}", base.display()))?;
        let mut jobs = Vec::with_capacity(files.len());
        for (p, rel) in &files {
            let fname = rel.file_name().ok_or_else(|| format!("cannot name {}", rel.display()))?;
            let mut new_rel = rel.clone();
            new_rel.set_file_name(suffix_name(fname, suffix));
            jobs.push((p.clone(), base.join(new_rel)));
        }
        parallel_copy(&jobs)?;
        println!(
            "backed up {} file(s) from {} -> {} (suffix: {})",
            jobs.len(),
            src.display(),
            base.display(),
            suffix
        );
    }
    Ok(())
}
