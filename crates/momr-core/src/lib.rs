//! momr-core: the meeting engine without a window.
//!
//! The crate compiles for Windows 11+, macOS and Linux (D25): no GTK, no
//! glib, no `cfg(target_os)`, and `std::os::unix` only in the one leaf D25
//! allows (`helper::is_executable`). Its dependencies are the portable ones
//! the engine needs: whisper-rs and ort for speech, chrono for the clock,
//! ureq for the cloud providers, serde_json for the formats. Paths, process
//! trees, private files and sockets go through `momr-platform`.
//!
//! Compiling everywhere is not yet behaving everywhere: capture (`audio`
//! runs `momr-audio` and ffmpeg's avfoundation), Keychain secrets
//! (`provider` runs `/usr/bin/security`), the system language (`settings`
//! runs `/usr/bin/defaults`) and the die-with-parent playback wrapper are
//! still macOS commands in here, waiting to move into `momr-platform`
//! (plan 16 step 2).
//!
//! The GTK binary links this crate; the AppKit shell runs the `momr` binary
//! (`finish`, `transcribe`) instead of linking it. Either way the on-disk
//! meeting format this crate reads and writes is the contract between them.

pub mod agent;
pub mod audio;
pub mod chapters;
pub mod cleanup;
pub mod diarize;
pub mod export;
pub mod finish;
pub mod helper;
pub mod ipc;
pub mod locales;
pub mod meeting;
pub mod models;
pub mod nemotron;
pub mod playback;
pub mod provider;
pub mod settings;
pub mod theme;
pub mod timer;
pub mod transcribe;
