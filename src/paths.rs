//! Where the app keeps things: `~/Library/Application Support/momr` for config,
//! settings and models, `~/Library/Caches/momr` for staging and the socket,
//! `~/Documents/Meetings` for meetings. Every path is under `APP_NAME`.
//!
//! GLib's `user_data_dir()` and friends return Linux-style `~/.local` paths
//! from Homebrew's build (measured, no Cocoa support), so the macOS locations
//! are built from the home directory here. An absolute `XDG_*` variable still
//! wins when set, which keeps power users and tests hermetic.

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
        gtk::glib::home_dir().join("Library/Application Support"),
    )
    .join(APP_NAME)
}

/// `~/Library/Application Support/momr` (macOS keeps config and state together).
pub fn config() -> PathBuf {
    env_or(
        "XDG_CONFIG_HOME",
        gtk::glib::home_dir().join("Library/Application Support"),
    )
    .join(APP_NAME)
}

/// `~/Library/Application Support/momr`.
pub fn state() -> PathBuf {
    env_or(
        "XDG_STATE_HOME",
        gtk::glib::home_dir().join("Library/Application Support"),
    )
    .join(APP_NAME)
}

/// `~/Library/Caches/momr`.
pub fn cache() -> PathBuf {
    env_or(
        "XDG_CACHE_HOME",
        gtk::glib::home_dir().join("Library/Caches"),
    )
    .join(APP_NAME)
}

/// The whisper models.
pub fn models() -> PathBuf {
    data().join("models")
}

/// `config.toml` with the `model` and `agent` keys.
pub fn config_file() -> PathBuf {
    config().join("config.toml")
}

/// Remembered preferences.
pub fn settings_file() -> PathBuf {
    state().join("settings.json")
}

/// The meetings folder, honouring a relocated Documents folder.
pub fn meetings() -> PathBuf {
    gtk::glib::user_special_dir(gtk::glib::UserDirectory::Documents)
        .unwrap_or_else(gtk::glib::home_dir)
        .join("Meetings")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{LazyLock, Mutex, MutexGuard};

    /// The environment is process-global, so serialise the tests that touch it.
    static LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn lock() -> MutexGuard<'static, ()> {
        LOCK.lock().unwrap()
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
