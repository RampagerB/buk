//! Backup root and suffix-template resolution (all env handling lives here).

use std::ffi::OsString;
use std::path::PathBuf;

pub const ENV_DEST: &str = "BUK_PATH";
pub const ENV_TEMPLATE: &str = "BUK_SUFFIX_TEMPLATE";
pub const DEFAULT_TEMPLATE: &str = "%Y%m%d_%H%M%S";

/// Home-directory variables, most specific first. Native Windows usually has
/// `USERPROFILE` but no `HOME`; Git Bash on Windows sets `HOME`; Unix and
/// macOS use `HOME`.
#[cfg(windows)]
const HOME_VARS: [&str; 2] = ["USERPROFILE", "HOME"];
#[cfg(not(windows))]
const HOME_VARS: [&str; 1] = ["HOME"];

/// Backup root: `$BUK_PATH` if set and non-empty, else `<home>/.buk`
/// (`~/.buk` on Unix/macOS, `%USERPROFILE%\.buk` on Windows).
pub fn backup_root() -> Result<PathBuf, String> {
    resolve_root(std::env::var_os(ENV_DEST), home_dir())
}

fn home_dir() -> Option<OsString> {
    for var in HOME_VARS {
        if let Some(v) = std::env::var_os(var).filter(|v| !v.is_empty()) {
            return Some(v);
        }
    }
    None
}

/// Pure resolution so tests never mutate process env.
pub fn resolve_root(buk_path: Option<OsString>, home: Option<OsString>) -> Result<PathBuf, String> {
    if let Some(v) = buk_path.filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(v));
    }
    match home.filter(|v| !v.is_empty()) {
        Some(h) => Ok(PathBuf::from(h).join(".buk")),
        None => Err(format!(
            "cannot determine backup root: {ENV_DEST} is not set and the home directory is unknown"
        )),
    }
}

/// Template precedence: `-t` arg > `$BUK_SUFFIX_TEMPLATE` > default.
pub fn resolve_template(arg: Option<&str>) -> String {
    if let Some(t) = arg {
        return t.to_string();
    }
    match std::env::var(ENV_TEMPLATE) {
        Ok(t) if !t.is_empty() => t,
        _ => DEFAULT_TEMPLATE.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_arg_wins() {
        assert_eq!(resolve_template(Some("%Y")), "%Y");
    }

    #[test]
    fn backup_root_defaults_to_home_dot_buk() {
        let home = Some(OsString::from("/home/u"));
        assert_eq!(
            resolve_root(Some(OsString::from("/custom")), home.clone()).unwrap(),
            PathBuf::from("/custom")
        );
        // expected value built the same way as the code → platform-independent
        let expected = PathBuf::from("/home/u").join(".buk");
        assert_eq!(resolve_root(Some(OsString::new()), home.clone()).unwrap(), expected);
        assert_eq!(resolve_root(None, home).unwrap(), expected);
        assert!(resolve_root(None, Some(OsString::new())).is_err());
        assert!(resolve_root(None, None).is_err());
    }
}
