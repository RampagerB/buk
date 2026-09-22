//! Benchmark: backup / restore / list throughput.
//!
//! Run with `cargo bench`. Std-only harness (no dev-dependencies): every
//! scenario runs the same fixture with untimed warmup iterations first, then
//! reports min / median / avg / max over the timed iterations.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use buk::backup::backup;
use buk::fsutil::{copy_one, walk_files};
use buk::list::collect_backups;
use buk::naming::suffix_name;
use buk::restore::restore;

const DIRS: usize = 8;
const FILES_PER_DIR: usize = 125; // DIRS * FILES_PER_DIR = 1000 files
const FILE_SIZE: usize = 16 * 1024; // 16 KiB
const WARMUP: u32 = 2;
const ITERS: u32 = 5;
/// Fixed suffix shaped like the default template output, so `-a/--at` can
/// parse the artifact names (and "latest" ordering still holds).
const SUFFIX: &str = "20260101_120000";
const SUFFIX_2: &str = "20260201_120000";

fn make_fixture(root: &Path) -> PathBuf {
    let src = root.join("src");
    let payload = vec![0u8; FILE_SIZE];
    for d in 0..DIRS {
        let dir = src.join(format!("d{d}"));
        fs::create_dir_all(&dir).unwrap();
        for f in 0..FILES_PER_DIR {
            fs::write(dir.join(format!("f{f:03}.bin")), &payload).unwrap();
        }
    }
    src
}

fn fresh(dir: &Path) {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).unwrap();
}

/// Warm up with `WARMUP` untimed calls, then collect `ITERS` timed runs.
/// `body` does its own (untimed) setup and returns the timed duration.
fn measure(mut body: impl FnMut() -> Duration) -> Vec<Duration> {
    for _ in 0..WARMUP {
        body();
    }
    (0..ITERS).map(|_| body()).collect()
}

/// `bytes = None` reports paths/s instead of files/s + MiB/s (metadata-only work).
fn report(name: &str, times: &[Duration], files: usize, bytes: Option<usize>) {
    let mut sorted = times.to_vec();
    sorted.sort();
    let n = sorted.len();
    let min = sorted[0];
    let max = sorted[n - 1];
    let median = sorted[n / 2];
    let avg = times.iter().sum::<Duration>() / n as u32;
    let secs = avg.as_secs_f64();
    let rate = match bytes {
        Some(b) => format!(
            "{:>10.0} files/s {:>9.1} MiB/s",
            files as f64 / secs,
            b as f64 / secs / (1024.0 * 1024.0)
        ),
        None => format!("{:>10.0} paths/s {:>9}", files as f64 / secs, "-"),
    };
    println!("{name:<30} {min:>9.2?} {median:>9.2?} {avg:>9.2?} {max:>9.2?}  {rate}");
}

/// The same job set `backup(..., recursive = true)` builds, copied sequentially.
fn sequential_recursive(src: &Path, dest: &Path) {
    let files = walk_files(src).unwrap();
    let base = dest.join(buk::naming::mirror_of(src).unwrap());
    for (p, rel) in &files {
        let mut new_rel = rel.clone();
        new_rel.set_file_name(suffix_name(rel.file_name().unwrap(), SUFFIX));
        copy_one(p, &base.join(new_rel)).unwrap();
    }
}

fn main() {
    let root = std::env::temp_dir().join(format!("buk_bench_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let src = make_fixture(&root);

    let files = DIRS * FILES_PER_DIR;
    let bytes = files * FILE_SIZE;
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    println!(
        "fixture: {files} files x {FILE_SIZE} B = {:.1} MiB, {WARMUP} warmup + {ITERS} timed iterations, {workers} CPUs\n",
        bytes as f64 / (1024.0 * 1024.0)
    );
    println!(
        "{:<30} {:>9} {:>9} {:>9} {:>9}   throughput",
        "scenario", "min", "median", "avg", "max"
    );

    // 1. Production path: recursive backup (parallel copy pool).
    let dest_par = root.join("dest_par");
    let times = measure(|| {
        fresh(&dest_par);
        let t = Instant::now();
        backup(&src, &dest_par, SUFFIX, true).unwrap();
        t.elapsed()
    });
    report("backup -r (parallel)", &times, files, Some(bytes));

    // 2. Baseline only (not a CLI code path): same jobs, one thread at a time.
    let dest_seq = root.join("dest_seq");
    let times = measure(|| {
        fresh(&dest_seq);
        let t = Instant::now();
        sequential_recursive(&src, &dest_seq);
        t.elapsed()
    });
    report("backup -r (1 thread, baseline)", &times, files, Some(bytes));

    // 3. Whole-folder backup (single suffixed directory).
    let dest_dir = root.join("dest_dir");
    let times = measure(|| {
        fresh(&dest_dir);
        let t = Instant::now();
        backup(&src, &dest_dir, SUFFIX, false).unwrap();
        t.elapsed()
    });
    report("backup dir (whole folder)", &times, files, Some(bytes));

    // 4. Restore of the whole-folder layout (dest_dir survives from step 3).
    let times = measure(|| {
        let t = Instant::now();
        restore(&src, &dest_dir, None, buk::DEFAULT_TEMPLATE).unwrap();
        t.elapsed()
    });
    report("restore dir (whole folder)", &times, files, Some(bytes));

    // 5. Restore of the recursive layout (merge-overwrite onto src).
    let times = measure(|| {
        let t = Instant::now();
        restore(&src, &dest_par, None, buk::DEFAULT_TEMPLATE).unwrap();
        t.elapsed()
    });
    report("restore -r (parallel)", &times, files, Some(bytes));

    // 6. Restore with -a/--at: two versions per file, nearest picked.
    //    Setup is untimed; the timed part includes suffix parsing + ranking.
    let dest_at = root.join("dest_at");
    fresh(&dest_at);
    backup(&src, &dest_at, SUFFIX, true).unwrap();
    backup(&src, &dest_at, SUFFIX_2, true).unwrap();
    let at = buk::restore::parse_target_datetime("2026-01-15", buk::DEFAULT_TEMPLATE).unwrap();
    let times = measure(|| {
        let t = Instant::now();
        restore(&src, &dest_at, Some(at), buk::DEFAULT_TEMPLATE).unwrap();
        t.elapsed()
    });
    report("restore -r (--at nearest)", &times, files, Some(bytes));

    // 7. List collection over the -r layout (walk + stat, no copying).
    let times = measure(|| {
        let t = Instant::now();
        let found = collect_backups(&src, &dest_par).unwrap().len();
        assert_eq!(found, files, "list must see every artifact");
        t.elapsed()
    });
    report("list -r (collect)", &times, files, None);

    let _ = fs::remove_dir_all(&root);
}
