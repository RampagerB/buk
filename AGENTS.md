# AGENTS.md

## What this is

- `buk`: CLI that backs up files/directories to `$BUK_PATH` with a dated suffix, and lists/restores them. Single Rust **binary crate** with a lib target.
- Structure: `src/main.rs` is a thin binary (args → `backup_root()` → `buk::run()` → exit codes); all behavior is in `src/lib.rs` + modules `config` (env/root/template), `naming` (`~` separator + path normalization/mirroring, Windows volume sanitization), `fsutil` (walk/copy/parallel pool), `backup`, `restore`, `list`, `cleanup`. Keep new logic in the matching module, not `main.rs`.
- `edition = "2024"` → requires Rust ≥ 1.85 (dev environment has 1.98.1).
- Git repo has **no commits yet** on `master`.

## Behavior (as implemented — keep tests green when changing)

- Backup root: `$BUK_PATH` if set and non-empty, else **`~/.buk`** (from `$HOME`). Exit code **2** only if neither resolves (`BUK_PATH` unset/empty *and* `$HOME` unset/empty). Backup target dir is created on demand. Other env: `BUK_SUFFIX_TEMPLATE`.
- Suffix template precedence: `-t/--template` arg > `$BUK_SUFFIX_TEMPLATE` > default `%Y%m%d_%H%M%S`.
- Backup separator is **`~`**: original name + `~` + formatted suffix. Never use `~` inside a template (suffix stripping splits at the last `~`).
- Every source is identified by its **absolute path mirrored under the backup root** (leading `/` dropped) — this is what keeps same-basename files from different dirs from colliding, merging version series, or cross-restoring. Resolution happens once in `run()` via `naming::absolute_normalized` (relative args joined with cwd, `.`/`..` collapsed lexically, excess `..` clamped at `/`, **no symlink resolution** — symlinked and real paths are distinct identities). Operation fns require an absolute path (`naming::mirror_of` enforces it). Flat artifacts live in `dest/<mirror parent>/<name>~<suffix>`; the `-r` layout root is exactly `dest/<mirror>`; lookups are scoped to those locations, never a scan of `dest` top level.
- Backup layouts (suffix computed **once per execution**, before any copying; source `/home/u/proj`):
  - file → `dest/home/u/proj/f.txt~<suffix>` (per file in its mirrored dir)
  - dir (default) → `dest/home/u/proj~<suffix>/` — whole tree as-is, inner files not individually suffixed; the target dir is created explicitly, so an **empty source dir still produces a backup**
  - dir with `-r/--recursive` → `dest/home/u/proj/<relpath>/<file>~<suffix>` — one suffixed copy per file
- All multi-file copies (backup **and** restore) run in parallel via std scoped threads; worker count = `available_parallelism()`. Do not add a crate (rayon etc.) for this.
- `-l/--list` and `-s/--restore` take the **original** path, not a backup path (they still work if the source was deleted — only its name is matched).
- Restore picks the "latest" backup as the **lexicographically greatest suffix** — only meaningful for time-sortable templates like the default. Restore overwrites existing files but does **not** delete files missing from the backup. If both dir layouts exist, whole-folder wins over `-r`.
- `-a/--at <WHEN>` (requires `-s`; validated in `run()` before dispatch, errors otherwise) instead picks the candidate whose suffix timestamp is **nearest** to WHEN: `restore::pick()` ranks by `|suffix time − WHEN|` in seconds, ties prefer the greater suffix, candidates whose suffix doesn't parse are skipped, and all-unparseable → error. `restore::parse_target_datetime` parses WHEN (and the same `parse_when` parses suffixes): active template first, then common formats (`%Y-%m-%d %H:%M[:%S]`, `%Y/%m/%d …`, `%Y%m%d[_%H%M%S]`, ISO `T`); date-only = midnight; both sides are naive local wall-clock (suffixes were formatted from local time), so no timezone conversion. Unparseable WHEN → error listing example formats.
- `--list` + `--restore` together is an error; `-r` is ignored in list/restore/cleanup modes. `-t` is ignored there too, **except** that restore uses the active template to parse candidate suffixes when `-a/--at` is given.
- Cleanup (`-c/--cleanup`, `src/cleanup.rs`) takes the original path and needs **exactly one** criterion (enforced in `run_cleanup()`): `--days N` (delete artifacts with mtime older than N days — age is by **mtime**, never template parsing), `--keep [N]` (bare = default 5; keeps newest N versions ranked lexicographically like restore), `--orphan` (no-op if source exists, else delete all its backups). Criteria without `-c`, zero/two criteria, or cleanup combined with `-l`/`-s` are errors. Empty parent dirs after `-r`-layout deletions are pruned. `keep: Option<Option<u64>>` in `Opt` relies on structopt `min_values = 0, max_values = 1` for the bare `--keep` form.
- Platform rules (Linux/macOS/Windows): home lookup is `USERPROFILE`→`HOME` on Windows (`cfg(windows)`), `HOME` elsewhere. Windows volumes are sanitized to a lowercase alnum token in mirrors (`C:\x` → `c\x`) — a component containing `:` must never reach `dest.join()`, because `PathBuf::push` would *replace* the backup root with the drive path. `normalize`/`mirror_of` assemble paths by manual OsString joining, never `PathBuf::push`. Path tests are cfg-gated (`#[cfg(unix)]` + `#[cfg(windows)]` variants in `naming.rs`) — new path cases go into both. `list`/`restore`/`cleanup` reject non-UTF-8 names (backup handles them); identity comparison is case-sensitive even on case-insensitive filesystems.
- Exit codes: 0 ok, 1 operation error, 2 backup root unresolvable (see above).

## Dependencies

- `structopt 0.3` is the CLI parser — keep using it; do **not** switch to `clap` derive. structopt 0.3 wraps clap v2, so clap-derive examples do not apply. StructOpt does not read env vars here; env is read manually with `std::env`.
- `chrono 0.4` is for all date/time handling (suffix formatting + list timestamps); don't add another time crate.
- Keep the dependency list at these two unless a requirement forces otherwise.

## Build / verify

- `cargo build` and `cargo test` — the working verification pair. Tests = unit tests inside modules (`config`, `naming`) + end-to-end roundtrips in `tests/backup_restore.rs` (includes a same-basename collision regression and `--at` nearest-version/validation cases) + cleanup criteria in `tests/cleanup.rs`. They use unique temp dirs and never mutate process env (root/template resolution take their env values as parameters), so they are safe to run in parallel. Cleanup tests hand-craft backup artifacts in the mirrored dir (no sleeping, no mtime manipulation).
- `cargo bench` — std-only benchmark (`benches/backup.rs`, `[[bench]] harness = false`): 7 scenarios (parallel `-r` backup, a bench-only 1-thread baseline that quantifies the parallel speedup, whole-dir backup, whole-dir restore, `-r` restore plain and with `-a/--at`, list collection), untimed warmup + timed iterations, min/median/avg/max output. The fixture suffix is timestamp-shaped (`20260101_120000` + a second version `20260201_120000`) so `--at` can parse artifact names. Keep it dependency-free (no criterion/dev-deps); `README.md`'s Benchmark section records one run.
- Cross-platform check: this box's distro Rust has no extra targets — install user-locally (`curl -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal`), then `rustup target add x86_64-pc-windows-msvc aarch64-apple-darwin` and `cargo check --target <t> --all-targets` (`check` needs no linker; tests type-check but only run on their own OS). Verified clean for both targets 2026-09-23; the install was removed afterwards to restore the environment.
- `cargo clippy` and `cargo fmt` are **not installed** in the dev environment (rustup components missing) — don't plan verify steps around them unless they get installed.
- No CI workflows, pre-commit hooks, or rustfmt/clippy config exist.
- `README.md` is user-facing; keep its layout/semantics tables in sync with `src/` when behavior changes.

## Conventions

- License: **Apache-2.0** — `LICENSE` is verbatim from apache.org's `LICENSE-2.0.txt` except the appendix copyright line, filled as `Copyright 2026 Ray Bao` (year + `git config user.name`); `license = "Apache-2.0"` in `Cargo.toml`; "License" section at the end of `README.md`. Keep all three in sync; don't add headers to source files or a NOTICE file unless asked.
- No README, release process, or branch/PR workflow is defined. Don't assume one; ask before inventing commit/PR conventions.
