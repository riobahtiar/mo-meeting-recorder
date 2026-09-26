//! Where playback sounds: the ffmpeg output that reaches the speakers.
//!
//! The core builds the whole ffmpeg command except its output, which is the
//! one part that differs per OS: macOS has an `audiotoolbox` muxer, while
//! ffmpeg has no WASAPI output at all, so Windows will need a different
//! pipeline (plan 16) rather than a different string. Until a target has
//! one, `OUTPUT` is None and the core reports that playback is not there yet.

/// The ffmpeg output arguments for this target, None where there is no
/// playback yet.
#[cfg(target_os = "macos")]
pub const OUTPUT: Option<&[&str]> = Some(&["-f", "audiotoolbox", "-"]);
#[cfg(not(target_os = "macos"))]
pub const OUTPUT: Option<&[&str]> = None;
