//! momr-core: the meeting engine without a window.
//!
//! Everything here builds on Windows 11+, macOS and Linux (D25): std only,
//! no GTK, no glib, no `std::os::unix`, no `cfg(target_os)`. Platform touches
//! (capture commands, playback sinks, secrets, paths, sockets) stay in the
//! shells or, once cut, in `momr-platform`. The GTK binary and, later, the
//! AppKit shell both depend on this crate; the on-disk meeting format it
//! reads and writes is the contract between them.

pub mod agent;
pub mod audio;
pub mod chapters;
pub mod cleanup;
pub mod diarize;
pub mod export;
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
