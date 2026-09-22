//! Filesystem primitives: tree walking, copying, and the parallel copy pool.

use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::SystemTime;

/// Recursively collect (absolute path, path relative to `dir`) for every file.
pub fn walk_files(dir: &std::path::Path) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    fn rec(
        dir: &std::path::Path,
        base: &std::path::Path,
        out: &mut Vec<(PathBuf, PathBuf)>,
    ) -> Result<(), String> {
        let rd = fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        for entry in rd {
            let entry = entry.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
            let path = entry.path();
            let ft =
                entry.file_type().map_err(|e| format!("cannot stat {}: {e}", path.display()))?;
            if ft.is_dir() {
                rec(&path, base, out)?;
            } else {
                let rel = path.strip_prefix(base).map_err(|e| e.to_string())?.to_path_buf();
                out.push((path, rel));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    rec(dir, dir, &mut out)?;
    Ok(out)
}

pub fn copy_one(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    fs::copy(src, dst)
        .map(|_| ())
        .map_err(|e| format!("cannot copy {} -> {}: {e}", src.display(), dst.display()))
}

/// Copy all (src, dst) pairs across a pool of worker threads.
pub fn parallel_copy(jobs: &[(PathBuf, PathBuf)]) -> Result<(), String> {
    if jobs.is_empty() {
        return Ok(());
    }
    if jobs.len() == 1 {
        return copy_one(&jobs[0].0, &jobs[0].1);
    }
    let workers = thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(jobs.len());
    let chunk = jobs.len().div_ceil(workers);
    thread::scope(|s| {
        let handles: Vec<_> = jobs
            .chunks(chunk)
            .map(|c| {
                s.spawn(move || {
                    let mut first_err = None;
                    for (src, dst) in c {
                        if let Err(e) = copy_one(src, dst) {
                            first_err.get_or_insert(e);
                        }
                    }
                    first_err
                })
            })
            .collect();
        let mut result: Result<(), String> = Ok(());
        for h in handles {
            if let Ok(Some(e)) = h.join() {
                if result.is_ok() {
                    result = Err(e);
                }
            }
        }
        result
    })
}

pub fn format_time(t: std::io::Result<SystemTime>) -> String {
    match t {
        Ok(t) => chrono::DateTime::<chrono::Local>::from(t)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
        Err(_) => "unknown".to_string(),
    }
}
