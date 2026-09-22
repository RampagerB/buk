# buk

A small CLI that backs up files and directories to a configurable backup root
using a dated suffix, and lists and restores them. No config file, no daemon —
just `$BUK_PATH` and a few flags.

- One date-time suffix **per execution** (every file in a run gets the same suffix)
- Multi-file backups run **in parallel** across CPU cores
- Backups **mirror the source's absolute path** under the backup root —
  same-named files from different directories never collide or cross-restore
- List and restore work even after the original source was deleted
- Two dependencies total (`structopt`, `chrono`)

## Requirements

Rust ≥ 1.85 (edition 2024).

## Build

```sh
cargo build --release   # → target/release/buk
cargo test
```

## Usage

```
buk <path> [-r] [-s [-a WHEN]] [-t TEMPLATE] [-l]
buk <path> -c (--days N | --keep [N] | --orphan)
```

| Flag | Meaning |
|------|---------|
| `-r`, `--recursive` | Directory path: back up each sub-file individually |
| `-s`, `--restore` | Restore the most recent backup for the given path |
| `-a`, `--at WHEN` | Restore: pick the backup **nearest** to this date-time instead of the latest (requires `-s`) |
| `-t`, `--template` | Date-time suffix template (chrono format string) |
| `-l`, `--list` | List backup file info (path, size, modified) for the given path |
| `-c`, `--cleanup` | Clean up backups; requires exactly one of the three criteria below |
| `--days N` | Cleanup: delete backups older than N days |
| `--keep [N]` | Cleanup: keep only the newest N versions (default: 5; bare `--keep` = 5) |
| `--orphan` | Cleanup: delete backups whose original source no longer exists |

Environment:

| Variable | Meaning |
|----------|---------|
| `BUK_PATH` | Backup root. Default: `~/.buk` |
| `BUK_SUFFIX_TEMPLATE` | Fallback suffix template when `-t` is not given |

Template precedence: `-t` > `$BUK_SUFFIX_TEMPLATE` > `%Y%m%d_%H%M%S`.
Exit codes: `0` success, `1` operation error, `2` backup root unresolvable.

### Examples

Assume `cwd = /home/u`:

```sh
buk notes.txt                          # → ~/.buk/home/u/notes.txt~20260922_232606
buk /srv/data/proj/                    # whole folder → ~/.buk/srv/data/proj~20260922_232606/
BUK_PATH=/mnt/backup buk /srv/data/proj/ -r
                                       # → /mnt/backup/srv/data/proj/d0/f000.bin~…
buk notes.txt -t '%Y%m%d'              # custom suffix template
buk notes.txt -l                       # list backups of notes.txt
buk notes.txt -s                       # restore latest backup over notes.txt
buk notes.txt -s -a "2026-09-20 14:30" # restore the backup closest to that moment
buk notes.txt -c --days 14             # drop backups older than 14 days
buk project/ -c --keep                 # keep the 5 newest versions per file
buk old.txt -c --orphan                # old.txt is gone → wipe its backups
```

## Backup layout

Every source is identified by its **absolute path mirrored under the backup
root** (leading `/` dropped), so `/a/notes.txt` and `/b/notes.txt` keep
separate version series. The separator between the original name and the
suffix is `~` (templates must not contain `~`).

| Source | Backup location |
|--------|-----------------|
| `/home/u/notes.txt` | `BUK_PATH/home/u/notes.txt~<suffix>` |
| `/srv/data/proj/` | `BUK_PATH/srv/data/proj~<suffix>/` (whole tree as-is) |
| `/srv/data/proj/ -r` | `BUK_PATH/srv/data/proj/<relpath>/<file>~<suffix>` (per file) |

Path resolution rules:

- Relative arguments are resolved against the current working directory at
  run time — run from a consistent cwd (or use absolute paths) so a source
  maps to the same backup location every time.
- `.` and `..` are normalized lexically; excess `..` clamps at `/`.
- Symlinks are **not** resolved: `/link/to/x` and `/real/x` are distinct
  backup identities.

Notes:

- The suffix is computed **once per run**, before any copying starts, so all
  files backed up together share an identical timestamp even though the copies
  run in parallel.
- `-l` / `-s` / `-c` take the **original** path, not a backup path.
- Restore picks the lexicographically greatest suffix as "latest" (correct for
  time-sortable templates like the default), overwrites existing files, and
  does **not** delete files missing from the backup. If both directory layouts
  exist, the whole-folder backup wins.
- `-a/--at WHEN` restores the backup **nearest** to that date-time instead:
  every candidate's suffix is parsed (active `-t` template first, then common
  formats) and the smallest `|time − WHEN|` wins; ties prefer the greater
  suffix. Accepted inputs: `2026-09-20 14:30(:05)`, `2026-09-20` (midnight),
  `20260920_143000`, or anything matching your template — all local
  wall-clock. Candidates with unparseable suffixes are skipped; if none parse
  (or `WHEN` itself doesn't), restore errors. `-a` requires `-s`.
- `-l` and `-s` cannot be combined; `-r` is ignored in those modes, and `-t`
  only matters for restore when `-a` is given (it parses the suffixes).

## Cleanup

`-c/--cleanup` takes the original path (like `-l`/`-s`) and needs **exactly one**
criterion — passing zero or several is an error:

| Criterion | Behavior |
|-----------|----------|
| `--days N` | Delete backup artifacts whose mtime is older than N days. Age uses artifact mtime (≈ backup creation time), not template parsing. `--days 0` removes everything. |
| `--keep [N]` | Keep only the newest N versions per item, delete the rest. Bare `--keep` means 5. Versions are ranked by suffix string — the same "latest = lexicographically greatest" rule restore uses. |
| `--orphan` | No-op if the source path still exists; otherwise delete all of its backups (the whole `-r` tree counts as one artifact). |

- Works against all three backup layouts; directories left empty by `-r`
  cleanups are pruned.
- Every removal is printed, followed by a `cleanup: removed N …` summary.
- Cannot be combined with `-l`/`-s`; criteria without `-c` are an error;
  `-r`/`-t` are ignored.

## Platform notes

Linux, macOS, and Windows are supported (all three type-checked: `cargo test`
on Linux, `cargo check --target x86_64-pc-windows-msvc /
aarch64-apple-darwin --all-targets`).

- Default root when `BUK_PATH` is unset: `~/.buk` from `$HOME`; on Windows
  `%USERPROFILE%\.buk` (a Git Bash `$HOME` is honored as fallback).
- Windows volumes are part of the mirrored identity as a sanitized lowercase
  token: `C:\docs\a.txt` → `BUK_PATH/c/docs/a.txt~<suffix>`; `C:` and `D:`
  never collide, and no `:` ever appears in a mirrored path (which would make
  `PathBuf::join` replace the backup root instead of appending).
- Backup identity is compared **case-sensitively** even on case-insensitive
  filesystems (Windows, default macOS): pass a path with consistent casing.
- Non-UTF-8 file names (possible on Linux/macOS) can be backed up, but
  `-l`/`-s`/`-c` reject them with an error.
- Mirrored paths are deeper than the source; very deep trees on Windows may
  exceed the legacy 260-character limit unless long paths are enabled.

## Benchmark

```sh
cargo bench
```

Fixture: 1000 files × 16 KiB (15.6 MiB), 2 warmup + 5 timed iterations, 16
CPUs, `/tmp` on the bench machine (warm page cache — treat as relative, not
absolute, numbers). Each scenario reports min / median / avg / max per run;
throughput is computed at the average (library progress lines elided):

```
fixture: 1000 files x 16384 B = 15.6 MiB, 2 warmup + 5 timed iterations, 16 CPUs

scenario                             min    median       avg       max   throughput
backup -r (parallel)              3.73ms    4.44ms    4.45ms    5.37ms      224838 files/s    3513.1 MiB/s
backup -r (1 thread, baseline)   27.58ms   27.96ms   28.16ms   29.40ms       35515 files/s     554.9 MiB/s
backup dir (whole folder)         3.93ms    4.41ms    4.46ms    4.86ms      224224 files/s    3503.5 MiB/s
restore dir (whole folder)        4.63ms    4.83ms    5.14ms    6.57ms      194382 files/s    3037.2 MiB/s
restore -r (parallel)             4.44ms    5.85ms    5.49ms    6.10ms      182218 files/s    2847.2 MiB/s
restore -r (--at nearest)         6.02ms    6.15ms    6.35ms    7.30ms      157537 files/s    2461.5 MiB/s
list -r (collect)                 1.02ms    1.03ms    1.06ms    1.18ms      939322 paths/s         -
```

Highlights:

- The parallel copy pool is ~6× faster than the 1-thread baseline. That row
  exists only to quantify the speedup — the CLI itself **always** copies in
  parallel (the sequential loop lives solely in `benches/backup.rs`).
- Restoring either layout costs about the same (~5 ms for 1000 files).
- `-a/--at` ranking — parsing two candidate suffixes per file — adds ~1 ms
  over a plain `-r` restore, still >150k files/s.
- Collecting a `-l` listing is metadata-only: ~1 ms for 1000 artifacts.

The benchmark uses a plain `harness = false` std-only target — no dev-dependencies.

## Development layout

```
src/main.rs     thin binary: arg parsing, backup-root resolution, exit codes
src/lib.rs      Opt + run() dispatch (all behavior, testable without a process)
src/config.rs   backup root + suffix template resolution (all env handling)
src/naming.rs   `~` suffix convention + absolute-path normalization/mirroring
src/fsutil.rs   tree walk, copy, parallel copy pool
src/backup.rs   backup operations
src/restore.rs  restore operations
src/list.rs     list operations
src/cleanup.rs  cleanup operations (days / keep / orphan)
tests/          end-to-end roundtrips + cleanup criteria tests
benches/        cargo bench target
```

## License

Apache License 2.0 — see [LICENSE](LICENSE).
