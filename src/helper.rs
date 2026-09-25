//! Finding `momr-audio` and reading its `list` output.
//!
//! The helper ships next to the Rust binary (see `scripts/build-macos.sh`),
//! so it is looked up beside the running executable first, then on `PATH`.
//! `list_info` runs `momr-audio list` for the Settings audio rows;
//! `blackhole_name` reads the loopback device the computer source falls back
//! to from the same output. `menubar_path` finds the status item shipped
//! beside the helper.

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

/// The `momr-menubar` status item next to the capture helper, if shipped.
pub fn menubar_path() -> Option<PathBuf> {
    let helper = path()?;
    let candidate = helper.parent()?.join("momr-menubar");
    is_executable(&candidate).then_some(candidate)
}

/// One input device from `list`, by the UID `momr-audio mic --device` takes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputDevice {
    pub name: String,
    pub uid: String,
}

/// One process Core Audio knows as an audio client, from `list`, by the
/// bundle identifier `momr-audio system --bundle` takes. `playing` means it
/// has an output stream running now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AudioProcess {
    pub pid: i64,
    pub bundle: String,
    pub name: String,
    pub playing: bool,
}

/// What `momr-audio list` reports.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AudioDevices {
    /// macOS is new enough for a process tap.
    pub tap: bool,
    /// TCC reports System Audio Recording as refused. Only a refusal is
    /// certain: "granted" and "unknown" both leave it to the tap, so they
    /// read as false here.
    pub tap_denied: bool,
    /// The BlackHole loopback device, when one is installed.
    pub blackhole: Option<String>,
    pub inputs: Vec<InputDevice>,
    pub outputs: usize,
    /// Running apps with audio, for the per-app computer source; an older
    /// helper lists none.
    pub processes: Vec<AudioProcess>,
}

/// The full `momr-audio list` picture, or why the helper could not give it:
/// "no microphones" would be wrong when the question simply failed.
pub fn list_info(helper: &Path) -> Result<AudioDevices, String> {
    let output = Command::new(helper)
        .arg("list")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = stderr.lines().rev().find(|l| !l.trim().is_empty());
        return Err(reason.map_or_else(
            || format!("exit {}", output.status.code().unwrap_or(-1)),
            |l| l.trim().to_owned(),
        ));
    }
    parse_list(&String::from_utf8_lossy(&output.stdout))
}

/// The BlackHole loopback device from `momr-audio list`, if one is installed.
pub fn blackhole_name(helper: &Path) -> Option<String> {
    list_info(helper).ok().and_then(|devices| devices.blackhole)
}

/// One `list` JSON object. Hand-parsed with serde_json, the way the rest of
/// the app reads small JSON.
fn parse_list(text: &str) -> Result<AudioDevices, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let count = |key: &str| value[key].as_array().map_or(0, Vec::len);
    let text = |v: &serde_json::Value, key: &str| v[key].as_str().unwrap_or("").to_owned();
    let inputs = value["inputs"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|d| InputDevice {
                    name: text(d, "name"),
                    uid: text(d, "uid"),
                })
                .collect()
        })
        .unwrap_or_default();
    let processes = value["processes"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|p| AudioProcess {
                    pid: p["pid"].as_i64().unwrap_or(0),
                    bundle: text(p, "bundle"),
                    name: text(p, "name"),
                    playing: p["playing"].as_bool().unwrap_or(false),
                })
                // A process without a bundle id cannot be chosen again
                // after it restarts, so it is not offered.
                .filter(|p| !p.bundle.is_empty())
                .collect()
        })
        .unwrap_or_default();
    Ok(AudioDevices {
        tap: value["tap"].as_bool().unwrap_or(false),
        tap_denied: value["tap_permission"].as_str() == Some("denied"),
        blackhole: value["blackhole"].as_str().map(str::to_owned),
        inputs,
        outputs: count("outputs"),
        processes,
    })
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
        let devices = parse_list(
            r#"{"blackhole":"BlackHole 2ch","inputs":[{"name":"Mic","uid":"AppleHDA:1"}],"outputs":[],"tap":true}"#,
        )
        .unwrap();
        assert_eq!(
            devices,
            AudioDevices {
                tap: true,
                tap_denied: false,
                blackhole: Some("BlackHole 2ch".into()),
                inputs: vec![InputDevice {
                    name: "Mic".into(),
                    uid: "AppleHDA:1".into()
                }],
                outputs: 0,
                processes: Vec::new(),
            }
        );
    }

    #[test]
    fn list_offers_processes_with_a_bundle_id_only() {
        let devices = parse_list(
            r#"{"tap":true,"processes":[{"pid":41,"bundle":"us.zoom.xos","name":"zoom.us","playing":true},{"pid":42,"bundle":"","name":"coreaudiod"}]}"#,
        )
        .unwrap();
        assert_eq!(
            devices.processes,
            vec![AudioProcess {
                pid: 41,
                bundle: "us.zoom.xos".into(),
                name: "zoom.us".into(),
                playing: true,
            }]
        );
        // An older helper without the key lists none.
        assert!(parse_list(r#"{"tap":true}"#).unwrap().processes.is_empty());
    }

    #[test]
    fn only_a_refusal_counts_as_denied() {
        let with = |permission: &str| {
            parse_list(&format!(
                r#"{{"tap":true,"tap_permission":"{permission}"}}"#
            ))
            .unwrap()
            .tap_denied
        };
        assert!(with("denied"));
        assert!(!with("granted"));
        assert!(!with("unknown"));
        // An older helper says nothing about permission.
        assert!(!parse_list(r#"{"tap":true}"#).unwrap().tap_denied);
    }

    #[test]
    fn list_without_a_loopback_leaves_it_empty() {
        let devices = parse_list(r#"{"inputs":[],"outputs":[],"tap":true}"#).unwrap();
        assert!(devices.tap);
        assert_eq!(devices.blackhole, None);
    }

    #[test]
    fn garbage_is_an_error_not_an_empty_list() {
        assert!(parse_list("not json").is_err());
        assert!(parse_list("").is_err());
    }

    #[test]
    fn only_executable_files_count() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("momr-helper-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("tool");
        std::fs::write(&file, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!is_executable(&file));
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(is_executable(&file));
        assert!(!is_executable(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
