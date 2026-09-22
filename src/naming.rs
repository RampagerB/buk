//! Backup naming convention: `<original name>~<formatted suffix>`, plus the
//! absolute-path normalization and backup-root mirroring that give every
//! source a collision-free identity on Linux, macOS, and Windows.
//!
//! The separator is `~`; a suffix is stripped by splitting at the *last* `~`,
//! so templates must never contain `~`.

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

pub const SEP: &str = "~";

/// Absolute, lexically normalized path. Relative paths are joined against `cwd`.
/// Resolves `.`/`..` without touching the filesystem and **without** resolving
/// symlinks (a symlinked path is a distinct backup identity from its target).
/// On Windows the drive/UNC prefix is preserved, so the result stays absolute.
pub fn absolute_normalized(path: &Path, cwd: &Path) -> Result<PathBuf, String> {
    let joined = if path.is_absolute() { path.to_path_buf() } else { cwd.join(path) };
    normalize(&joined).ok_or_else(|| format!("{} has no usable absolute path", path.display()))
}

/// Backup-root-relative mirror of an absolute path:
/// `/home/u/a.txt` → `home/u/a.txt`, `C:\home\u\a.txt` → `c\home\u\a.txt`.
///
/// This is the path every operation (backup/list/restore/cleanup) uses to find
/// a source's artifacts inside the backup root — so same-basename files from
/// different directories never collide and different Windows volumes stay
/// distinct. Windows volumes are sanitized to a lowercase alphanumeric token
/// (never `:`), so joining the mirror can never escape or replace the backup
/// root on any platform.
pub fn mirror_of(abs: &Path) -> Result<PathBuf, String> {
    if !abs.is_absolute() {
        return Err(format!("{} is not absolute", abs.display()));
    }
    let sep = std::path::MAIN_SEPARATOR_STR;
    let mut out = OsString::new();
    let mut has_normal = false;
    for c in abs.components() {
        match c {
            Component::Prefix(p) => out.push(sanitize_volume(p.as_os_str())),
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                return Err(format!("{} is not normalized (contains ..)", abs.display()));
            }
            Component::Normal(s) => {
                if !out.is_empty() {
                    out.push(sep);
                }
                out.push(s);
                has_normal = true;
            }
        }
    }
    if !has_normal {
        return Err(format!("{} cannot be mirrored (filesystem root)", abs.display()));
    }
    Ok(PathBuf::from(out))
}

/// `C:` → `c`, `\\?\C:` → `c`, `\\server\share` → `servershare`.
fn sanitize_volume(p: &OsStr) -> OsString {
    let filtered: String = p
        .to_string_lossy()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    if filtered.is_empty() { OsString::from("vol") } else { OsString::from(filtered) }
}

fn normalize(path: &Path) -> Option<PathBuf> {
    let sep = std::path::MAIN_SEPARATOR_STR;
    let mut prefix: Option<OsString> = None;
    let mut root = false;
    let mut parts: Vec<&OsStr> = Vec::new();
    for c in path.components() {
        match c {
            Component::Prefix(p) => prefix = Some(p.as_os_str().to_os_string()),
            Component::RootDir => root = true,
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.pop().is_none() && !root {
                    return None; // relative path escaping above its start
                }
            }
            Component::Normal(s) => parts.push(s),
        }
    }
    // Assembled manually: PathBuf::push has subtle absolute/prefix replacement
    // rules on Windows that would mangle `C:`-style paths.
    let mut out = OsString::new();
    if let Some(p) = prefix {
        out.push(p);
    }
    if root {
        out.push(sep);
    }
    for p in parts {
        if !out.is_empty() && !out.as_encoded_bytes().ends_with(sep.as_bytes()) {
            out.push(sep);
        }
        out.push(p);
    }
    (!out.is_empty()).then(|| PathBuf::from(out))
}

pub fn file_name(p: &Path) -> Result<&OsStr, String> {
    p.file_name().ok_or_else(|| format!("{} has no file name", p.display()))
}

/// `note.txt` + `20260922` → `note.txt~20260922`
pub fn suffix_name(name: &OsStr, suffix: &str) -> OsString {
    let mut s = name.to_os_string();
    s.push(SEP);
    s.push(suffix);
    s
}

/// Inverse of [`suffix_name`] for a path component: `sub/a~b~2026` → `sub/a~b`.
/// Non-UTF-8 names and names without `~` are returned unchanged.
pub fn strip_rel_suffix(rel: &Path) -> PathBuf {
    if let Some(fname) = rel.file_name().and_then(OsStr::to_str) {
        if let Some((base, _)) = fname.rsplit_once(SEP) {
            let mut out = rel.to_path_buf();
            out.set_file_name(base);
            return out;
        }
    }
    rel.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffix_and_strip_roundtrip() {
        assert_eq!(suffix_name(OsStr::new("note.txt"), "20260922"), "note.txt~20260922");

        let backed = PathBuf::from("sub").join(suffix_name(OsStr::new("a~b.txt"), "S"));
        assert_eq!(strip_rel_suffix(&backed), PathBuf::from("sub").join("a~b.txt"));

        assert_eq!(strip_rel_suffix(Path::new("sub/plain.txt")), Path::new("sub/plain.txt"));
        assert_eq!(strip_rel_suffix(Path::new("empty~")), Path::new("empty"));
    }

    #[cfg(unix)]
    #[test]
    fn absolute_normalized_joins_and_collapses() {
        let cwd = Path::new("/home/u");
        assert_eq!(
            absolute_normalized(Path::new("a/./b.txt"), cwd).unwrap(),
            PathBuf::from("/home/u/a/b.txt")
        );
        assert_eq!(
            absolute_normalized(Path::new("a/../b.txt"), cwd).unwrap(),
            PathBuf::from("/home/u/b.txt")
        );
        assert_eq!(
            absolute_normalized(Path::new("/x/y/../z.txt"), cwd).unwrap(),
            PathBuf::from("/x/z.txt")
        );
        // excess `..` clamps at root instead of escaping
        assert_eq!(
            absolute_normalized(Path::new("/../../etc/passwd"), cwd).unwrap(),
            PathBuf::from("/etc/passwd")
        );
    }

    #[cfg(unix)]
    #[test]
    fn mirror_strips_root_only() {
        assert_eq!(mirror_of(Path::new("/home/u/a.txt")).unwrap(), PathBuf::from("home/u/a.txt"));
        assert_eq!(mirror_of(Path::new("/tmp")).unwrap(), PathBuf::from("tmp"));
        assert!(mirror_of(Path::new("/")).is_err());
        assert!(mirror_of(Path::new("relative")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn absolute_normalized_windows() {
        let cwd = Path::new(r"C:\home\u");
        assert_eq!(
            absolute_normalized(Path::new("a\\..\\b.txt"), cwd).unwrap(),
            PathBuf::from(r"C:\home\u\b.txt")
        );
        assert_eq!(
            absolute_normalized(Path::new(r"C:\x\y\..\z.txt"), cwd).unwrap(),
            PathBuf::from(r"C:\x\z.txt")
        );
        // excess `..` clamps at the volume root instead of escaping
        assert_eq!(
            absolute_normalized(Path::new(r"C:\..\..\windows"), cwd).unwrap(),
            PathBuf::from(r"C:\windows")
        );
        // a rooted (drive-less) path picks up the cwd's drive
        assert_eq!(
            absolute_normalized(Path::new(r"\data\f.txt"), cwd).unwrap(),
            PathBuf::from(r"C:\data\f.txt")
        );
    }

    #[cfg(windows)]
    #[test]
    fn mirror_sanitizes_volumes() {
        assert_eq!(
            mirror_of(Path::new(r"C:\home\u\a.txt")).unwrap(),
            PathBuf::from(r"c\home\u\a.txt")
        );
        assert_ne!(
            mirror_of(Path::new(r"C:\a")).unwrap(),
            mirror_of(Path::new(r"D:\a")).unwrap(),
            "different volumes must get different mirrors"
        );
        assert!(mirror_of(Path::new(r"C:\")).is_err());
        assert!(mirror_of(Path::new("relative")).is_err());
    }
}
