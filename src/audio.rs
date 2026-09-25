//! Audio capture: one child process per source, kept running for the whole
//! life of the app so the meters work before and after a recording too.
//!
//! The microphone comes from the `momr-audio` helper's `mic`, which follows
//! default-device changes, or from ffmpeg's avfoundation input when the helper
//! is missing. The computer audio comes from the helper's `system` process
//! tap, falling back to a BlackHole loopback device captured with ffmpeg, and
//! otherwise staying idle with a note for the ready page. Every path honours
//! the same contract: raw interleaved s16le at `RATE` and `CHANNELS` on
//! stdout, until killed.
//!
//! The loopback is found by name through the helper's `list`; without the
//! helper the stock "BlackHole 2ch" name is tried. A source's note says why
//! it is not capturing and stays until audio flows again; the helper's exit
//! codes (`EXIT_*`, shared with helpers/momr-audio/…/main.swift) pick the
//! note, and its last stderr line is kept for the ones that need a reason.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub const RATE: u32 = 48_000;
pub const CHANNELS: u32 = 2;
/// 20 ms of s16le audio.
const CHUNK_BYTES: usize = (RATE / 50 * 2 * CHANNELS) as usize;
/// Three seconds of 20 ms peaks.
pub const HISTORY: usize = 150;
const FLOOR_DB: f64 = -60.0;
/// The loopback name BlackHole installs with, tried when no helper can list devices.
const BLACKHOLE_DEFAULT: &str = "BlackHole 2ch";

// The helper's exit codes (helpers/momr-audio/Sources/momr-audio/main.swift).
/// macOS older than 14.2: no process taps.
const EXIT_TAP_UNSUPPORTED: i32 = 3;
/// Permission refused: Microphone for `mic`, System Audio Recording for `system`.
const EXIT_PERMISSION: i32 = 4;
/// No input device, or the microphone would not start.
const EXIT_NO_DEVICE: i32 = 5;
/// The tap was created but Core Audio failed after that.
const EXIT_TAP_FAILED: i32 = 6;
/// Audio conversion kept failing.
const EXIT_CONVERSION: i32 = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    /// The default microphone.
    Mic,
    /// What the computer plays.
    Computer,
}

struct Inner {
    levels: VecDeque<f32>,
    file: Option<BufWriter<File>>,
    /// While paused the meters keep running but nothing is written.
    paused: bool,
    /// Why this source is not capturing, for the banner under the meters.
    /// Cleared when audio flows again, unless it describes the way it flows.
    note: Option<String>,
    /// A failed write to the recording file, kept until the recording stops.
    write_error: Option<String>,
}

#[derive(Clone)]
pub struct Source {
    inner: Arc<Mutex<Inner>>,
}

impl Source {
    /// Starts capturing `device` on a thread that restarts the child whenever
    /// it exits, so a device that goes away comes back on its own.
    pub fn spawn(device: Device) -> Self {
        let inner = Arc::new(Mutex::new(Inner {
            levels: VecDeque::from(vec![0.0; HISTORY]),
            file: None,
            paused: false,
            note: None,
            write_error: None,
        }));
        let source = Source {
            inner: inner.clone(),
        };
        thread::spawn(move || match device {
            Device::Mic => mic_loop(&inner),
            Device::Computer => computer_loop(&inner),
        });
        source
    }

    /// Tees the raw stream (s16le, RATE, CHANNELS) into `path` from now on.
    pub fn start_recording(&self, path: &Path) -> std::io::Result<()> {
        let file = BufWriter::new(File::create(path)?);
        let mut inner = self.inner.lock().unwrap();
        inner.file = Some(file);
        inner.paused = false;
        inner.write_error = None;
        Ok(())
    }

    pub fn set_paused(&self, paused: bool) {
        self.inner.lock().unwrap().paused = paused;
    }

    /// Stops the tee. Returns why audio was lost, when a write or the final
    /// flush failed (a full disk, say), so the app can say so.
    pub fn stop_recording(&self) -> Option<String> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(mut file) = inner.file.take()
            && let Err(e) = file.flush()
            && inner.write_error.is_none()
        {
            inner.write_error = Some(e.to_string());
        }
        inner.write_error.take()
    }

    pub fn levels(&self) -> Vec<f32> {
        self.inner.lock().unwrap().levels.iter().copied().collect()
    }

    /// The loudest of the last `n` peaks, so a short burst is not missed by a slower reader.
    pub fn recent_peak(&self, n: usize) -> f32 {
        let inner = self.inner.lock().unwrap();
        inner
            .levels
            .iter()
            .rev()
            .take(n)
            .copied()
            .fold(0.0, f32::max)
    }

    /// Why this source is not capturing or recording, if anything.
    pub fn note(&self) -> Option<String> {
        let inner = self.inner.lock().unwrap();
        match &inner.write_error {
            Some(e) => Some(crate::locales::tf("banner.audio_write_failed", &[e])),
            None => inner.note.clone(),
        }
    }
}

/// (program, args) for one capture child, so tests can check it without spawning.
pub fn capture_args(device: Device, helper: Option<&Path>) -> (String, Vec<String>) {
    let rate = RATE.to_string();
    let channels = CHANNELS.to_string();
    match (device, helper) {
        (Device::Mic, Some(helper)) => (
            helper.display().to_string(),
            vec![
                "mic".into(),
                "--rate".into(),
                rate,
                "--channels".into(),
                channels,
            ],
        ),
        (Device::Mic, None) => (
            "ffmpeg".into(),
            vec![
                "-hide_banner".into(),
                "-loglevel".into(),
                "error".into(),
                "-nostdin".into(),
                "-f".into(),
                "avfoundation".into(),
                "-i".into(),
                ":default".into(),
                "-f".into(),
                "s16le".into(),
                "-ar".into(),
                rate,
                "-ac".into(),
                channels,
                "-".into(),
            ],
        ),
        (Device::Computer, Some(helper)) => (
            helper.display().to_string(),
            vec![
                "system".into(),
                "--rate".into(),
                rate,
                "--channels".into(),
                channels,
            ],
        ),
        // No helper: the loopback is the only computer source left.
        (Device::Computer, None) => blackhole_args(BLACKHOLE_DEFAULT),
    }
}

/// ffmpeg reading a BlackHole loopback device by name.
pub fn blackhole_args(device: &str) -> (String, Vec<String>) {
    let rate = RATE.to_string();
    let channels = CHANNELS.to_string();
    (
        "ffmpeg".into(),
        vec![
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-nostdin".into(),
            "-f".into(),
            "avfoundation".into(),
            "-i".into(),
            format!(":{device}"),
            "-f".into(),
            "s16le".into(),
            "-ar".into(),
            rate,
            "-ac".into(),
            channels,
            "-".into(),
        ],
    )
}

/// What the computer source captures from, with what it needs to do so.
/// The loop below only moves between these; `next_mode` decides the moves so
/// tests can cover them.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ComputerMode {
    /// The helper's process tap, through the helper at this path.
    Tap(PathBuf),
    /// A BlackHole loopback device, by name, through ffmpeg.
    BlackHole(String),
    /// Nothing captures; the note explains why.
    Idle,
}

/// The next mode and the note for it, from the current mode, the helper when
/// present, the loopback device name when one is known, and the exit code of
/// the child that just ended (None on the first run or after a signal). A
/// None note leaves the current one as it is.
fn next_mode(
    current: &ComputerMode,
    helper: Option<&Path>,
    blackhole: Option<&str>,
    exit: Option<i32>,
) -> (ComputerMode, Option<String>) {
    use crate::locales::t;
    let install = t("banner.audio_install_blackhole");
    // Why the tap cannot be used, when the helper said it cannot.
    let tap_problem = match exit {
        Some(EXIT_TAP_UNSUPPORTED) => Some(t("banner.audio_tap_unsupported")),
        Some(EXIT_PERMISSION) => Some(t("banner.audio_tap_permission")),
        Some(EXIT_TAP_FAILED) => Some(t("banner.audio_tap_failed")),
        Some(EXIT_CONVERSION) => Some(t("banner.audio_conversion")),
        _ => None,
    };
    let missing_helper = || format!("{} {install}", t("banner.audio_helper_missing"));
    let tap = |helper: &Path| (ComputerMode::Tap(helper.to_path_buf()), None);
    match current {
        ComputerMode::Tap(_) => match (helper, tap_problem) {
            // No helper any more: the loopback, by its known or stock name.
            (None, _) => (
                ComputerMode::BlackHole(blackhole.unwrap_or(BLACKHOLE_DEFAULT).to_owned()),
                None,
            ),
            // First run, a clean end, or a crash: the tap is worth retrying.
            (Some(helper), None) => tap(helper),
            // The tap cannot work: the loopback if there is one, saying that
            // it only hears what is routed to it.
            (Some(_), Some(problem)) => match blackhole {
                Some(device) => (
                    ComputerMode::BlackHole(device.to_owned()),
                    Some(format!("{problem} {}", t("banner.audio_tap_fell_back"))),
                ),
                None => (ComputerMode::Idle, Some(format!("{problem} {install}"))),
            },
        },
        ComputerMode::BlackHole(device) => match (exit, helper) {
            // Just moved here, or a clean end: capture, preferring the tap again.
            (None | Some(0), Some(helper)) if blackhole.is_none() => tap(helper),
            (None | Some(0), _) => (ComputerMode::BlackHole(device.clone()), None),
            // The loopback went away or never existed. With a helper the tap
            // may work; without one, idle and say what is missing.
            (_, Some(helper)) => tap(helper),
            (_, None) => (ComputerMode::Idle, Some(missing_helper())),
        },
        // Something changed while idle (a device installed, the helper back):
        // re-evaluate from scratch.
        ComputerMode::Idle => match helper {
            Some(helper) => tap(helper),
            None => (
                ComputerMode::BlackHole(blackhole.unwrap_or(BLACKHOLE_DEFAULT).to_owned()),
                None,
            ),
        },
    }
}

fn set_note(shared: &Mutex<Inner>, note: Option<String>) {
    shared.lock().unwrap().note = note;
}

/// The mic's note after its capture child ended, from the helper's exit code
/// (ffmpeg only ever exits 1) and its last stderr line.
fn mic_note(exit: Option<i32>, reason: &str) -> String {
    use crate::locales::{t, tf};
    match exit {
        Some(EXIT_PERMISSION) => t("banner.audio_permission").to_owned(),
        Some(EXIT_NO_DEVICE) => t("banner.audio_no_mic").to_owned(),
        Some(EXIT_CONVERSION) => t("banner.audio_conversion").to_owned(),
        code => {
            let code = code.map_or_else(|| "signal".to_owned(), |c| c.to_string());
            let note = tf("banner.audio_mic_failed", &[&code]);
            if reason.is_empty() {
                note
            } else {
                format!("{note} ({reason})")
            }
        }
    }
}

fn mic_loop(shared: &Mutex<Inner>) {
    loop {
        let helper = crate::helper::path();
        let (program, args) = capture_args(Device::Mic, helper.as_deref());
        let note = match capture_from(&program, &args, shared, false) {
            Ok((exit, reason)) => mic_note(exit, &reason),
            Err(_) => crate::locales::t("banner.audio_no_ffmpeg").to_owned(),
        };
        set_note(shared, Some(note));
        // ffmpeg exits when the device goes away; the helper when it has no
        // device. Either way, try again.
        thread::sleep(Duration::from_secs(1));
    }
}

fn computer_loop(shared: &Mutex<Inner>) {
    let mut mode = ComputerMode::Idle;
    let mut exit: Option<i32> = None;
    let mut blackhole: Option<String> = None;
    let mut keep_note = false;
    loop {
        let helper = crate::helper::path();
        // The loopback can be installed or removed at any time; ask again
        // whenever the last choice did not capture.
        if exit.is_some_and(|code| code != 0) || blackhole.is_none() {
            blackhole = helper.as_deref().and_then(crate::helper::blackhole_name);
        }
        let (next, note) = next_mode(&mode, helper.as_deref(), blackhole.as_deref(), exit);
        // A fall-back note describes the capture itself, so it stays while
        // the loopback runs, restarts included; any other note goes once
        // audio flows.
        let on_loopback = matches!(next, ComputerMode::BlackHole(_));
        keep_note = on_loopback && (keep_note || note.is_some());
        if note.is_some() {
            set_note(shared, note);
        }
        mode = next;
        let (program, args) = match &mode {
            ComputerMode::Tap(helper) => capture_args(Device::Computer, Some(helper)),
            ComputerMode::BlackHole(device) => blackhole_args(device),
            ComputerMode::Idle => {
                thread::sleep(Duration::from_secs(5));
                exit = None;
                continue;
            }
        };
        exit = match capture_from(&program, &args, shared, keep_note) {
            Ok((code, reason)) => {
                if !reason.is_empty() {
                    eprintln!("{}: computer audio: {reason}", crate::APP_NAME);
                }
                // A signal leaves no code; count it as a crash, not a first run.
                Some(code.unwrap_or(-1))
            }
            Err(_) => {
                // The program itself would not start. For ffmpeg that means it
                // is missing; for the helper it should not happen, since its
                // path was checked.
                set_note(
                    shared,
                    Some(crate::locales::tf("banner.audio_no_program", &[&program])),
                );
                Some(-1)
            }
        };
        // Whatever ended the child, do not restart it at full speed: a helper
        // that crashes at once, or a loopback name ffmpeg cannot open, would
        // otherwise spin a core for the life of the app.
        thread::sleep(Duration::from_secs(1));
    }
}

/// Runs one capture child to EOF, keeping the level history and the recording
/// file as it goes. Returns the child's exit code with the last line it wrote
/// to stderr, or an error when it would not start at all. The note is cleared
/// on the first audio unless `keep_note`.
fn capture_from(
    program: &str,
    args: &[String],
    shared: &Mutex<Inner>,
    keep_note: bool,
) -> std::io::Result<(Option<i32>, String)> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    // Stderr is drained on its own thread (a full pipe would stall the
    // child), keeping only the last line: the helper's reason for exiting.
    let stderr = child.stderr.take().map(|pipe| {
        thread::spawn(move || {
            use std::io::BufRead;
            std::io::BufReader::new(pipe)
                .lines()
                .map_while(Result::ok)
                .filter(|line| !line.trim().is_empty())
                .last()
                .unwrap_or_default()
        })
    });
    let mut buf = vec![0u8; CHUNK_BYTES];
    // So a crash loses at most a second: flush every second, and push it to
    // the disk itself every half minute in case the machine goes down too.
    let mut chunks: u64 = 0;
    while stdout.read_exact(&mut buf).is_ok() {
        chunks += 1;
        if chunks == 1 && !keep_note {
            set_note(shared, None);
        }
        let peak = buf
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes([b[0], b[1]]).unsigned_abs())
            .max()
            .unwrap_or(0) as f32
            / 32768.0;
        let mut inner = shared.lock().unwrap();
        inner.levels.pop_front();
        inner.levels.push_back(peak);
        if !inner.paused
            && let Some(file) = inner.file.as_mut()
        {
            let written = file
                .write_all(&buf)
                .and_then(|()| {
                    if chunks.is_multiple_of(50) {
                        file.flush()
                    } else {
                        Ok(())
                    }
                })
                .and_then(|()| {
                    if chunks.is_multiple_of(1500) {
                        file.get_ref().sync_data()
                    } else {
                        Ok(())
                    }
                });
            // The first failure is what the user needs to hear about (a full
            // disk, a removed drive); later ones repeat it.
            if let Err(e) = written
                && inner.write_error.is_none()
            {
                inner.write_error = Some(e.to_string());
            }
        }
    }
    let _ = child.kill();
    let code = child.wait().ok().and_then(|status| status.code());
    let reason = stderr
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    Ok((code, reason.trim().to_owned()))
}

/// Maps a linear peak to 0..1 on a -60 dB..0 dB scale.
pub fn to_meter(peak: f32) -> f64 {
    if peak <= 0.0 {
        return 0.0;
    }
    (1.0 - 20.0 * f64::from(peak).log10() / FLOOR_DB).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn helper() -> PathBuf {
        PathBuf::from("/Applications/MOM Recorder.app/Contents/MacOS/momr-audio")
    }

    #[test]
    fn mic_prefers_the_helper() {
        let (program, args) = capture_args(Device::Mic, Some(helper().as_path()));
        assert_eq!(program, helper().display().to_string());
        assert_eq!(args[0], "mic");
        assert!(args.windows(2).any(|w| w == ["--rate", "48000"]));
        assert!(args.windows(2).any(|w| w == ["--channels", "2"]));
    }

    #[test]
    fn mic_falls_back_to_the_default_ffmpeg_input() {
        let (program, args) = capture_args(Device::Mic, None);
        assert_eq!(program, "ffmpeg");
        assert!(args.windows(2).any(|w| w == ["-i", ":default"]));
        assert!(args.windows(2).any(|w| w == ["-ar", "48000"]));
    }

    #[test]
    fn computer_uses_the_tap_when_the_helper_is_there() {
        let (program, args) = capture_args(Device::Computer, Some(helper().as_path()));
        assert_eq!(program, helper().display().to_string());
        assert_eq!(args[0], "system");
    }

    #[test]
    fn computer_without_a_helper_names_blackhole() {
        let (program, args) = capture_args(Device::Computer, None);
        assert_eq!(program, "ffmpeg");
        assert!(args.windows(2).any(|w| w == ["-i", ":BlackHole 2ch"]));
    }

    fn tap() -> ComputerMode {
        ComputerMode::Tap(helper())
    }

    fn loopback(name: &str) -> ComputerMode {
        ComputerMode::BlackHole(name.to_owned())
    }

    #[test]
    fn tap_retry_then_loopback_then_idle() {
        let h = helper();
        let h = Some(h.as_path());
        // First run and crashes retry the tap, leaving the note alone.
        assert_eq!(next_mode(&ComputerMode::Idle, h, None, None), (tap(), None));
        assert_eq!(next_mode(&tap(), h, None, Some(1)), (tap(), None));
        assert_eq!(next_mode(&tap(), h, None, Some(-1)), (tap(), None));
        // A refused tap moves to the loopback when one is installed, and says
        // that it only hears what is routed to it …
        let (mode, note) = next_mode(&tap(), h, Some("BlackHole 2ch"), Some(EXIT_PERMISSION));
        assert_eq!(mode, loopback("BlackHole 2ch"));
        assert!(note.unwrap().contains("Multi-Output"));
        // … and idles with a banner when none is.
        let (mode, note) = next_mode(&tap(), h, None, Some(EXIT_PERMISSION));
        assert_eq!(mode, ComputerMode::Idle);
        assert!(note.unwrap().contains("BlackHole"));
        // Each cause has its own words: old macOS, permission, Core Audio.
        let note = |code| next_mode(&tap(), h, None, Some(code)).1.unwrap();
        let expected = format!(
            "{} {}",
            crate::locales::t("banner.audio_tap_unsupported"),
            crate::locales::t("banner.audio_install_blackhole")
        );
        assert_eq!(note(EXIT_TAP_UNSUPPORTED), expected);
        assert_ne!(note(EXIT_PERMISSION), note(EXIT_TAP_FAILED));
        assert_ne!(note(EXIT_TAP_FAILED), note(EXIT_CONVERSION));
    }

    #[test]
    fn missing_helper_goes_straight_to_the_loopback() {
        assert_eq!(
            next_mode(&tap(), None, Some("BlackHole 16ch"), None).0,
            loopback("BlackHole 16ch")
        );
        // Without a helper to list devices, the stock name is tried …
        assert_eq!(
            next_mode(&ComputerMode::Idle, None, None, None).0,
            loopback("BlackHole 2ch")
        );
        // … and when it does not open, the source idles and says why.
        let (mode, note) = next_mode(&loopback("BlackHole 2ch"), None, None, Some(1));
        assert_eq!(mode, ComputerMode::Idle);
        assert!(note.unwrap().contains("momr-audio"));
    }

    #[test]
    fn lost_loopback_prefers_the_tap_again() {
        let h = helper();
        let h = Some(h.as_path());
        assert_eq!(
            next_mode(
                &loopback("BlackHole 2ch"),
                h,
                Some("BlackHole 2ch"),
                Some(1)
            )
            .0,
            tap()
        );
        // A loopback that works keeps going.
        assert_eq!(
            next_mode(
                &loopback("BlackHole 2ch"),
                h,
                Some("BlackHole 2ch"),
                Some(0)
            )
            .0,
            loopback("BlackHole 2ch")
        );
    }

    #[test]
    fn mic_notes_name_the_cause() {
        use crate::locales::t;
        assert_eq!(
            mic_note(Some(EXIT_PERMISSION), ""),
            t("banner.audio_permission")
        );
        assert_eq!(mic_note(Some(EXIT_NO_DEVICE), ""), t("banner.audio_no_mic"));
        let other = mic_note(Some(1), "Input/output error");
        assert!(other.contains('1') && other.contains("Input/output error"));
        assert!(mic_note(None, "").contains("signal"));
    }

    #[test]
    fn capture_keeps_the_last_stderr_line_and_the_exit_code() {
        let shared = Mutex::new(Inner {
            levels: VecDeque::from(vec![0.0; HISTORY]),
            file: None,
            paused: false,
            note: Some("old".into()),
            write_error: None,
        });
        let args: Vec<String> = ["-c", "echo first >&2; echo why >&2; exit 4"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (code, reason) = capture_from("sh", &args, &shared, false).unwrap();
        assert_eq!(code, Some(4));
        assert_eq!(reason, "why");
        // No audio came, so the old note stands.
        assert_eq!(shared.lock().unwrap().note.as_deref(), Some("old"));
        // Audio clears it.
        let args: Vec<String> = ["-c", "head -c 7680 /dev/zero"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        capture_from("sh", &args, &shared, false).unwrap();
        assert_eq!(shared.lock().unwrap().note, None);
    }
}
