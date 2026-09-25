//! Finding `momr-audio` and reading its `list` output.
//!
//! The helper ships next to the Rust binary (see `scripts/build-macos.sh`),
//! so it is looked up beside the running executable first, then on `PATH`.
//! `blackhole_name` runs `momr-audio list` once per lookup and returns the
//! loopback device the computer source falls back to.

use std::path::{Path, PathBuf};
use std::process::Command;

const HELPER: &str = "momr-audio";

/// The helper beside this executable, else on `PATH`, else None.
pub fn path() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        // Next to the binary, then the bundle's MacOS dir one level up.
        for dir in [dir, &dir.join("..").join("MacOS")] {
            let candidate = dir.join(HELPER);
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    which(HELPER)
}

/// The full `momr-audio list` picture: tap support, the BlackHole device when
/// one is installed, and the input/output device counts.
pub fn list_info(helper: &Path) -> Option<(bool, Option<String>, usize, usize)> {
    let output = Command::new(helper)
        .arg("list")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(parse_list(&String::from_utf8_lossy(&output.stdout)))
}

/// The BlackHole loopback device from `momr-audio list`, if one is installed.
pub fn blackhole_name(helper: &Path) -> Option<String> {
    list_info(helper).and_then(|(_, blackhole, _, _)| blackhole)
}

/// (tap supported, BlackHole device, inputs, outputs) from one `list` JSON
/// object. Hand-parsed with serde_json, the way the rest of the app reads
/// small JSON.
fn parse_list(text: &str) -> (bool, Option<String>, usize, usize) {
    let value: serde_json::Value = serde_json::from_str(text).unwrap_or(serde_json::Value::Null);
    let tap = value["tap"].as_bool().unwrap_or(false);
    let blackhole = value["blackhole"].as_str().map(str::to_owned);
    let count = |key: &str| value[key].as_array().map_or(0, Vec::len);
    (tap, blackhole, count("inputs"), count("outputs"))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|p| is_executable(p))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_parses_tap_blackhole_and_counts() {
        let (tap, blackhole, inputs, outputs) = parse_list(
            r#"{"blackhole":"BlackHole 2ch","inputs":[{"name":"Mic"}],"outputs":[],"tap":true}"#,
        );
        assert!(tap);
        assert_eq!(blackhole.as_deref(), Some("BlackHole 2ch"));
        assert_eq!((inputs, outputs), (1, 0));
    }

    #[test]
    fn list_without_a_loopback_leaves_it_empty() {
        let (tap, blackhole, _, _) = parse_list(r#"{"inputs":[],"outputs":[],"tap":true}"#);
        assert!(tap);
        assert_eq!(blackhole, None);
    }

    #[test]
    fn garbage_is_no_tap_and_no_loopback() {
        assert_eq!(parse_list("not json"), (false, None, 0, 0));
        assert_eq!(parse_list(""), (false, None, 0, 0));
    }

    #[test]
    fn found_helper_is_executable() {
        // Environment-dependent: only the shape is asserted.
        if let Some(path) = path() {
            assert!(is_executable(&path));
        }
    }
}
