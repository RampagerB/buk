//! `buk` core library: backup, list, and restore operations.
//!
//! The binary (`main.rs`) only parses CLI args, resolves the backup root, and
//! maps errors to exit codes; all behavior lives here so it can be exercised
//! by integration tests and benchmarks.

pub mod backup;
pub mod cleanup;
pub mod config;
pub mod fsutil;
pub mod list;
pub mod naming;
pub mod restore;

use std::path::{Path, PathBuf};

use structopt::StructOpt;

pub use config::{backup_root, resolve_root, resolve_template, DEFAULT_TEMPLATE, ENV_DEST, ENV_TEMPLATE};

#[derive(Debug, StructOpt)]
#[structopt(
    name = "buk",
    about = "Back up files/directories to $BUK_PATH (default: ~/.buk), list and restore them"
)]
pub struct Opt {
    /// File or directory to back up, list, or restore
    pub path: PathBuf,

    /// When the path is a directory, back up each sub-file individually
    #[structopt(short = "r", long = "recursive")]
    pub recursive: bool,

    /// Restore the most recent backup for the given path
    #[structopt(short = "s", long = "restore")]
    pub restore: bool,

    /// Restore the backup nearest to this date-time (requires --restore)
    #[structopt(short = "a", long = "at", value_name = "WHEN")]
    pub at: Option<String>,

    /// Date-time template for the backup suffix (falls back to $BUK_SUFFIX_TEMPLATE, then %Y%m%d_%H%M%S)
    #[structopt(short = "t", long = "template")]
    pub template: Option<String>,

    /// List backup file information for the given path
    #[structopt(short = "l", long = "list")]
    pub list: bool,

    /// Clean up backups; requires exactly one of --days, --keep, --orphan
    #[structopt(short = "c", long = "cleanup")]
    pub cleanup: bool,

    /// Cleanup criterion: remove backups older than DAYS (by mtime)
    #[structopt(long = "days")]
    pub days: Option<u64>,

    /// Cleanup criterion: keep only the newest KEEP versions per item (default: 5)
    #[structopt(long = "keep", min_values = 0, max_values = 1)]
    pub keep: Option<Option<u64>>,

    /// Cleanup criterion: remove backups whose original source no longer exists
    #[structopt(long = "orphan")]
    pub orphan: bool,
}

/// Dispatch on flags. `dest` is the already-resolved backup root.
///
/// The source path is resolved once here — absolute, `.`/`..` normalized,
/// relative paths joined against the current directory — so every operation
/// keys off the same [`naming::mirror_of`] identity.
pub fn run(opt: &Opt, dest: &Path) -> Result<(), String> {
    if opt.list && opt.restore {
        return Err("--list and --restore cannot be combined".to_string());
    }
    if opt.at.is_some() && !opt.restore {
        return Err("--at requires --restore".to_string());
    }
    if opt.cleanup || opt.days.is_some() || opt.keep.is_some() || opt.orphan {
        return run_cleanup(opt, dest);
    }
    if opt.list {
        return list::cmd_list(&absolute_source(&opt.path)?, dest);
    }
    if opt.restore {
        let abs = absolute_source(&opt.path)?;
        let template = resolve_template(opt.template.as_deref());
        let at = match opt.at.as_deref() {
            Some(s) => Some(restore::parse_target_datetime(s, &template)?),
            None => None,
        };
        return restore::restore(&abs, dest, at, &template);
    }
    // Compute the suffix once per execution so every file backed up in this
    // run gets the identical date-time suffix, even though copies run in parallel.
    let template = resolve_template(opt.template.as_deref());
    let suffix = chrono::Local::now().format(&template).to_string();
    backup::backup(&absolute_source(&opt.path)?, dest, &suffix, opt.recursive)
}

fn absolute_source(path: &Path) -> Result<PathBuf, String> {
    let cwd =
        std::env::current_dir().map_err(|e| format!("cannot determine current directory: {e}"))?;
    naming::absolute_normalized(path, &cwd)
}

fn run_cleanup(opt: &Opt, dest: &Path) -> Result<(), String> {
    if !opt.cleanup {
        return Err("--days/--keep/--orphan require --cleanup".to_string());
    }
    if opt.list || opt.restore {
        return Err("--cleanup cannot be combined with --list/--restore".to_string());
    }
    let criterion = match (opt.days, opt.keep, opt.orphan) {
        (Some(d), None, false) => cleanup::Criterion::Days(d),
        (None, Some(k), false) => cleanup::Criterion::Keep(k.unwrap_or(cleanup::DEFAULT_KEEP)),
        (None, None, true) => cleanup::Criterion::Orphan,
        (None, None, false) => {
            return Err("cleanup requires a criterion: --days N, --keep [N], or --orphan".to_string())
        }
        _ => return Err("cleanup accepts only one of --days, --keep, --orphan".to_string()),
    };
    cleanup::cleanup(&absolute_source(&opt.path)?, dest, criterion)
}
