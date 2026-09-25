# 06 Paths and environment

## Goal

Models, config, settings and the cache live where macOS keeps them (`~/Library/Application Support/momr`, `~/Library/Caches/momr`), meetings land in `~/Documents/Meetings`, and an app launched from Finder finds `ffmpeg`, the helper and the agents.

## Done when

- [x] `ls ~/Library/Application\ Support/momr/` shows `models/`, `config.toml` (when written) and `settings.json` after a run.
- [x] The app started with `open target/release/momr` (Finder-like environment) records, plays back and finds the configured agent.
- [x] `momr watch` works from a second terminal.
- [x] No module hand-rolls an `XDG_*` lookup any more. `grep -n "XDG_" src/` matches `paths.rs`, which honours an absolute `XDG_*` on purpose (D22) so power users and tests stay hermetic; `agent.rs`, which sets private data and state dirs *for the agent* and puts the agent's working directory under `$XDG_RUNTIME_DIR` when it exists (else `$TMPDIR`); and `main.rs`, which inside the app bundle points `XDG_DATA_DIRS` at `Contents/Resources/share` so GTK finds the bundled schemas, icons and data.

Observed 2026-09-25: fresh `~/Library/Caches/momr` is created on first run
(the socket bind made its own parent dir first); staging, socket and watch
all live there; `transcribe-file --model tiny` finds the migrated models with
no download. `open` launches, serves `watch`, and spawns both `momr-audio`
children, so `extend_path` works — but both meters read flat 0.0 there: an
unsigned dev binary gets no TCC microphone/tap grant (plan 08 gives the app
its own identity). `settings.json` appears on the first Preferences change;
the save path is the same `env_or` mechanism the tests cover.

Socket fix 2026-09-25: `momr start/stop` were intermittently ignored because
macOS refuses `SO_SNDTIMEO` once a fire-and-forget client has closed, and the
accept loop dropped the connection then. The timeout is best-effort now;
five start/stop cycles in a row all land.

## Prerequisites

Plan 02.

## Background

Three modules compute directories by hand: `settings.rs` `path()` (state), `models.rs` `config_file()` (config) and `data_dir()` (data), `transcribe.rs` `data_dir()` (data). Each reads an `XDG_*` variable and falls back to `~/.local/share`, `~/.config` or `~/.local/state`. GLib's `g_get_user_data_dir()` and friends do not help: Homebrew's GLib has no Cocoa support and returns those same `~/.local` paths on macOS (measured, D22), so `paths.rs` builds the `~/Library` locations from the home directory itself and still honours an absolute `XDG_*` when set. `ipc.rs` and the staging code in `ui.rs` used `glib::user_runtime_dir()` and `glib::user_cache_dir()` and move to `paths::cache()` for the same reason. `theme.rs` has its own state-dir lookup for Omarchy's theme folder; plan 07 deletes that module's reader, so leave it alone here.

`ui.rs` `output_dir()` hardcodes `~/Documents/Meetings`, which is right but should come from `glib::user_special_dir(Documents)` so a relocated Documents folder is respected.

A GUI app launched from Finder or Spotlight gets a minimal `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin`). Homebrew's `/opt/homebrew/bin` (Apple silicon) or `/usr/local/bin` (Intel) is not on it, nor are npm's or the user's bin directories. Every `Command::new("ffmpeg")` in the app would fail. This is the most common reason a GTK app "works from the terminal and not from the Dock".

Unix socket paths on macOS are limited to 104 bytes. `~/Library/Caches/momr/momr.sock` is about 45 for a short user name; long names still fit, but check it.

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

The sketch above is the first shape; D22 replaced the GLib calls, because Homebrew's GLib returns `~/.local` paths on macOS. The shipped `paths.rs` resolves config, data and state to `~/Library/Application Support` and the cache to `~/Library/Caches`, each overridable by an absolute `XDG_*`, so `config.toml` and `settings.json` end up side by side in `momr/`, which is what a Mac user expects. Replace the hand-rolled functions in the three modules with calls into `paths`. Keep the voxtype lookup in `models.rs` `find()` as `glib::user_data_dir().join("voxtype/models")`.

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
- Measured 2026-09-25: Homebrew GLib 2.90 has no Cocoa support (`user_data_dir()` gives `~/.local/share`), which is why D22 builds the `~/Library` defaults directly. Only `user_special_dir(Documents)` is trusted from GLib.

## Status

- [x] Step 1 `paths.rs` and callers
- [x] Step 2 meetings folder
- [x] Step 3 socket length fallback
- [x] Step 4 `PATH`
- [x] Step 5 README paths
- [x] Step 6 tests
