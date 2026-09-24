# 06 Paths and environment

## Goal

Models, config, settings and the cache live where macOS keeps them (`~/Library/Application Support/momr`, `~/Library/Caches/momr`), meetings land in `~/Documents/Meetings`, and an app launched from Finder finds `ffmpeg`, the helper and the agents.

## Done when

- [ ] `ls ~/Library/Application\ Support/momr/` shows `models/`, `config.toml` (when written) and `settings.json` after a run.
- [ ] The app started with `open target/release/momr` (Finder-like environment) records, plays back and finds the configured agent.
- [ ] `momr watch` works from a second terminal.
- [ ] `grep -n "XDG_" src/` matches only the `XDG_*` variables the agent runner sets *for the agent* (its private data and state dirs).

## Prerequisites

Plan 02.

## Background

Three modules compute directories by hand: `settings.rs` `path()` (state), `models.rs` `config_file()` (config) and `data_dir()` (data), `transcribe.rs` `data_dir()` (data). Each reads an `XDG_*` variable and falls back to `~/.local/share`, `~/.config` or `~/.local/state`. GLib's `g_get_user_data_dir()` and friends return `~/Library/Application Support` and `~/Library/Caches` on macOS (Homebrew's GLib is built with Cocoa support), still honouring `XDG_*` when set. `ipc.rs` and the staging code in `ui.rs` already use `glib::user_runtime_dir()` and `glib::user_cache_dir()`. `theme.rs` has its own state-dir lookup for Omarchy's theme folder; plan 07 deletes that module's reader, so leave it alone here.

`ui.rs` `output_dir()` hardcodes `~/Documents/Meetings`, which is right but should come from `glib::user_special_dir(Documents)` so a relocated Documents folder is respected.

A GUI app launched from Finder or Spotlight gets a minimal `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin`). Homebrew's `/opt/homebrew/bin` (Apple silicon) or `/usr/local/bin` (Intel) is not on it, nor are npm's or the user's bin directories. Every `Command::new("ffmpeg")` in the app would fail. This is the most common reason a GTK app "works from the terminal and not from the Dock".

Unix socket paths on macOS are limited to 104 bytes. `~/Library/Caches/momr.sock` is about 40 for a short user name; long names still fit, but check it.

## Steps

### 1. `src/paths.rs`

```rust
// sketch
//! Where the app keeps things, through GLib so macOS conventions are followed
//! (~/Library/Application Support, ~/Library/Caches). Every path is under APP_NAME.

use std::path::PathBuf;
use gtk::glib;
use crate::APP_NAME;

pub fn data() -> PathBuf { glib::user_data_dir().join(APP_NAME) }
pub fn config() -> PathBuf { glib::user_config_dir().join(APP_NAME) }
pub fn state() -> PathBuf { glib::user_state_dir().join(APP_NAME) }
pub fn cache() -> PathBuf { glib::user_cache_dir().join(APP_NAME) }
pub fn models() -> PathBuf { data().join("models") }
pub fn config_file() -> PathBuf { config().join("config.toml") }
pub fn settings_file() -> PathBuf { state().join("settings.json") }
pub fn meetings() -> PathBuf {
    glib::user_special_dir(glib::UserDirectory::Documents)
        .unwrap_or_else(glib::home_dir)   // no Documents folder registered: use home
        .join("Meetings")
}
```

On macOS, GLib's config and state dirs both resolve to `~/Library/Application Support`, so `config.toml` and `settings.json` end up side by side in `momr/`, which is what a Mac user expects. Replace the hand-rolled functions in the three modules with calls into `paths`. Keep the voxtype lookup in `models.rs` `find()` as `glib::user_data_dir().join("voxtype/models")`.

### 2. Meetings folder

`ui.rs` `output_dir()` uses `paths::meetings()`.

### 3. Socket path length

In `ipc.rs` `socket_path()`, after computing the path, if it is longer than 100 bytes fall back to `std::env::temp_dir().join(…)` (`$TMPDIR` is a private per-user directory). Add a test that a long fake base dir triggers the fallback (make the base a parameter).

### 4. `PATH` for GUI launches

At the top of `main()`, before GTK initialises and before any thread exists:

```rust
// sketch
fn extend_path() {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut extra: Vec<PathBuf> = vec!["/opt/homebrew/bin".into(), "/usr/local/bin".into()];
    if let Some(h) = home {
        for p in [".local/bin", ".cargo/bin", ".npm-global/bin", ".bun/bin", ".volta/bin"] { extra.push(h.join(p)); }
    }
    // The helper next to this executable, and the bundle's MacOS dir (plan 08).
    if let Ok(exe) = std::env::current_exe() && let Some(dir) = exe.parent() { extra.insert(0, dir.to_path_buf()); }
    let current = std::env::var_os("PATH").unwrap_or_default();
    let joined = std::env::join_paths(extra.into_iter().filter(|p| p.is_dir()).chain(std::env::split_paths(&current))).unwrap();
    // SAFETY: called first thing in main, before any other thread exists; the
    // environment is not read concurrently. (set_var is unsafe in edition 2024.)
    unsafe { std::env::set_var("PATH", joined) };
}
```

Optionally, when `TERM` is unset (a GUI launch), also ask the login shell once: `$SHELL -lc 'printf %s "$PATH"'` with a 3-second timeout, and prepend the result. That catches `nvm` and other shell-managed tools. Keep it best-effort and silent.

### 5. Document the paths

Update the "Where things go" paragraph of the root README to the `~/Library` locations.

### 6. Tests

- `paths` functions end with the expected suffixes and are absolute.
- With `XDG_DATA_HOME=/tmp/x` set in a test (guarded by a mutex since env is process-global), `paths::data()` is `/tmp/x/momr`.
- `extend_path` puts the exe directory first and keeps the original entries.

## Verify

1. `rm -rf ~/Library/Application\ Support/momr` (a fresh state), run the app, change the language: `settings.json` appears there.
2. Download the tiny model through the banner: `models/ggml-tiny.bin` is there.
3. `open target/release/momr` (Launch Services, minimal PATH): meters move, a meeting exports (needs `ffmpeg`), chapters find the agent.
4. `momr watch` from another terminal prints state lines.

## Risks and notes

- `set_var` in edition 2024 is `unsafe` for a reason: it must happen before threads. `main()` is the only place.
- If Homebrew GLib were ever built without Cocoa support, `user_data_dir()` would return `~/.local/share`. The formula (plan 08) pins GLib from Homebrew, which is built with it.

## Status

- [ ] Step 1 `paths.rs` and callers
- [ ] Step 2 meetings folder
- [ ] Step 3 socket length fallback
- [ ] Step 4 `PATH`
- [ ] Step 5 README paths
- [ ] Step 6 tests
