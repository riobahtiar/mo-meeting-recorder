//! Where the app keeps things: `~/Library/Application Support/momr` for config,
//! settings and models, `~/Library/Caches/momr` for staging and the socket,
//! `~/Documents/Meetings` for meetings (or the folder picked in Settings,
//! see the shell's `settings::meetings_dir`). Every path but the meetings is
//! under `APP_NAME`.
//!
//! The locations are built from environment variables with a home-directory
//! fallback, because Homebrew GLib's directory functions return Linux paths
//! here (measured, no Cocoa support: D22) and std has no home API. An
//! absolute `XDG_*` variable still wins when set, which keeps power users
//! and tests hermetic.

use std::path::PathBuf;

use crate::APP_NAME;

fn env_or(var: &str, fallback: PathBuf) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or(fallback)
}

/// `~/Library/Application Support/momr`.
pub fn data() -> PathBuf {
    env_or(
        "XDG_DATA_HOME",
        home_dir().join("Library/Application Support"),
    )
    .join(APP_NAME)
}

/// `~/Library/Application Support/momr` (macOS keeps config and state together).
pub fn config() -> PathBuf {
    env_or(
        "XDG_CONFIG_HOME",
        home_dir().join("Library/Application Support"),
    )
    .join(APP_NAME)
}

/// `~/Library/Application Support/momr`.
pub fn state() -> PathBuf {
    env_or(
        "XDG_STATE_HOME",
        home_dir().join("Library/Application Support"),
    )
    .join(APP_NAME)
}

/// `~/Library/Caches/momr`.
pub fn cache() -> PathBuf {
    env_or("XDG_CACHE_HOME", home_dir().join("Library/Caches")).join(APP_NAME)
}

/// The whisper models.
pub fn models() -> PathBuf {
    data().join("models")
}

/// `config.toml`: `model`, `agent`, `provider`, `openrouter_model` and
/// `menubar`, the settings users edit by hand.
pub fn config_file() -> PathBuf {
    config().join("config.toml")
}

/// Remembered preferences.
pub fn settings_file() -> PathBuf {
    state().join("settings.json")
}

/// The home directory: `$HOME` on macOS and Linux, `%USERPROFILE%` on
/// Windows. Both are always set for a launched app; without either there is
/// no home to build from, so the current directory is the last resort.
/// Shared with runners that keep dotfiles there (agents, shells).
pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// The meetings folder. macOS and Linux keep it at `~/Documents`, Windows at
/// `%USERPROFILE%\Documents`; all three flow through `home_dir`. A relocated
/// Windows Known Folder is the Windows shell's problem (plan 16), not this
/// function's: std has no Known-Folder API and this crate takes no dependency
/// to get one.
pub fn meetings() -> PathBuf {
    home_dir().join("Documents").join("Meetings")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn locations_end_with_momr_and_are_absolute() {
        let _guard = lock();
        for dir in [data(), config(), state(), cache(), models(), meetings()] {
            assert!(dir.is_absolute(), "{}", dir.display());
        }
        for dir in [data(), config(), state(), cache()] {
            assert_eq!(dir.file_name().unwrap(), APP_NAME);
        }
        assert_eq!(models().parent().unwrap(), data());
        assert_eq!(config_file().parent().unwrap(), config());
        assert_eq!(settings_file().parent().unwrap(), state());
    }

    #[test]
    fn xdg_overrides_win() {
        let _guard = lock();
        unsafe { std::env::set_var("XDG_DATA_HOME", "/tmp/xdg-test-data") };
        assert_eq!(data(), PathBuf::from("/tmp/xdg-test-data/momr"));
        unsafe { std::env::remove_var("XDG_DATA_HOME") };
    }
}
