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

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;
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
    /// Why this source is not capturing, for the ready-page banner. None while capturing.
    note: Option<String>,
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
        Ok(())
    }

    pub fn set_paused(&self, paused: bool) {
        self.inner.lock().unwrap().paused = paused;
    }

    pub fn stop_recording(&self) {
        if let Some(mut file) = self.inner.lock().unwrap().file.take() {
            let _ = file.flush();
        }
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

    /// Why this source is not capturing, if anything, for the ready page.
    pub fn note(&self) -> Option<String> {
        self.inner.lock().unwrap().note.clone()
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
        (Device::Computer, None) => blackhole_args("BlackHole 2ch"),
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

/// What the computer source captures from. The loop below only moves between
/// these; `next_mode` decides the moves so tests can cover them.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ComputerMode {
    /// The helper's process tap.
    Tap,
    /// A BlackHole loopback device through ffmpeg.
    BlackHole,
    /// Nothing captures; the ready page explains why.
    Idle,
}

/// The next mode and the banner note for it, from the current mode, whether
/// the helper is present, the loopback device name when one is installed, and
/// the exit code of the child that just ended (None on the first run).
fn next_mode(
    current: &ComputerMode,
    helper: bool,
    blackhole: Option<&str>,
    exit: Option<i32>,
) -> (ComputerMode, Option<String>) {
    // The helper's exit codes for an unavailable tap (see helpers/momr-audio).
    let tap_dead = matches!(exit, Some(3) | Some(4));
    let tap_note = |code| match code {
        Some(4) => crate::locales::t("banner.audio_tap_permission"),
        _ => crate::locales::t("banner.audio_tap_unsupported"),
    };
    let install = crate::locales::t("banner.audio_install_blackhole");
    let loopback_or_idle = |reason: &str| match blackhole {
        Some(_) => (ComputerMode::BlackHole, None),
        None => (ComputerMode::Idle, Some(format!("{reason} {install}"))),
    };
    let missing_helper = || {
        format!(
            "{} {}",
            crate::locales::t("banner.audio_helper_missing"),
            install
        )
    };
    match current {
        ComputerMode::Tap => {
            if !helper {
                return match blackhole {
                    Some(_) => (ComputerMode::BlackHole, None),
                    None => (ComputerMode::Idle, Some(missing_helper())),
                };
            }
            match exit {
                // First run, a clean end, or a crash: the tap is worth retrying.
                None | Some(0) | Some(_) if !tap_dead => (ComputerMode::Tap, None),
                _ => loopback_or_idle(tap_note(exit)),
            }
        }
        ComputerMode::BlackHole => match exit {
            // Just moved here, or a clean end: capture, preferring the tap again.
            None | Some(0) if helper => (ComputerMode::Tap, None),
            None | Some(0) => (ComputerMode::BlackHole, None),
            // The loopback went away: the tap may work; without a helper keep
            // retrying the loopback and say so.
            _ if helper => (ComputerMode::Tap, None),
            _ => (
                ComputerMode::BlackHole,
                Some(crate::locales::t("banner.audio_device_gone").to_owned()),
            ),
        },
        // Something changed while idle (a device installed, the helper back):
        // re-evaluate from scratch.
        ComputerMode::Idle => {
            if helper {
                (ComputerMode::Tap, None)
            } else {
                match blackhole {
                    Some(_) => (ComputerMode::BlackHole, None),
                    None => (ComputerMode::Idle, Some(missing_helper())),
                }
            }
        }
    }
}

fn set_note(shared: &Mutex<Inner>, note: Option<String>) {
    shared.lock().unwrap().note = note;
}

fn mic_loop(shared: &Mutex<Inner>) {
    loop {
        let helper = crate::helper::path();
        let (program, args) = capture_args(Device::Mic, helper.as_deref());
        match capture_from(&program, &args, shared) {
            Ok(code) => set_note(
                shared,
                Some(
                    crate::locales::t("banner.audio_mic_failed")
                        .replace("{}", &code.unwrap_or(-1).to_string()),
                ),
            ),
            Err(_) => set_note(
                shared,
                Some(crate::locales::t("banner.audio_no_ffmpeg").to_owned()),
            ),
        }
        // ffmpeg exits when the device goes away; the helper when it has no
        // device. Either way, try again.
        thread::sleep(Duration::from_secs(1));
    }
}

fn computer_loop(shared: &Mutex<Inner>) {
    let mut mode = ComputerMode::Tap;
    let mut exit: Option<i32> = None;
    let mut blackhole: Option<String> = None;
    loop {
        let helper = crate::helper::path();
        if blackhole.is_none()
            && let Some(helper) = helper.as_ref()
        {
            blackhole = crate::helper::blackhole_name(helper);
        }
        let (program, args, idle_secs);
        (mode, program, args, idle_secs) =
            match next_mode(&mode, helper.is_some(), blackhole.as_deref(), exit) {
                (ComputerMode::Tap, note) => {
                    set_note(shared, note);
                    let helper = helper.expect("Tap mode needs the helper");
                    let (program, args) = capture_args(Device::Computer, Some(&helper));
                    (ComputerMode::Tap, program, args, 1)
                }
                (ComputerMode::BlackHole, note) => {
                    set_note(shared, note);
                    let device = blackhole.clone().unwrap_or_else(|| "BlackHole 2ch".into());
                    let (program, args) = blackhole_args(&device);
                    (ComputerMode::BlackHole, program, args, 5)
                }
                (ComputerMode::Idle, note) => {
                    set_note(shared, note);
                    thread::sleep(Duration::from_secs(5));
                    exit = None;
                    continue;
                }
            };
        exit = match capture_from(&program, &args, shared) {
            Ok(code) => code,
            Err(_) => {
                // The program itself would not start. For ffmpeg that means it
                // is missing; for the helper it should not happen, since its
                // path was checked. Back off and re-evaluate.
                set_note(
                    shared,
                    Some(crate::locales::t("banner.audio_no_program").replace("{}", &program)),
                );
                thread::sleep(Duration::from_secs(idle_secs));
                None
            }
        };
    }
}

/// Runs one capture child to EOF, keeping the level history and the recording
/// file as it goes. Returns the child's exit code, or an error when it would
/// not start at all.
fn capture_from(
    program: &str,
    args: &[String],
    shared: &Mutex<Inner>,
) -> std::io::Result<Option<i32>> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut buf = vec![0u8; CHUNK_BYTES];
    // So a crash loses at most a second: flush every second, and push it to
    // the disk itself every half minute in case the machine goes down too.
    let mut chunks: u64 = 0;
    while stdout.read_exact(&mut buf).is_ok() {
        chunks += 1;
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
            let _ = file.write_all(&buf);
            if chunks.is_multiple_of(50) {
                let _ = file.flush();
            }
            if chunks.is_multiple_of(1500) {
                let _ = file.get_ref().sync_data();
            }
        }
    }
    let _ = child.kill();
    Ok(child.wait().ok().and_then(|status| status.code()))
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

    #[test]
    fn tap_retry_then_loopback_then_idle() {
        // First run and crashes retry the tap.
        assert_eq!(
            next_mode(&ComputerMode::Tap, true, None, None).0,
            ComputerMode::Tap
        );
        assert_eq!(
            next_mode(&ComputerMode::Tap, true, None, Some(1)).0,
            ComputerMode::Tap
        );
        // A refused tap moves to the loopback when one is installed …
        let (mode, note) = next_mode(&ComputerMode::Tap, true, Some("BlackHole 2ch"), Some(4));
        assert_eq!(mode, ComputerMode::BlackHole);
        assert_eq!(note, None);
        // … and idles with a banner when none is.
        let (mode, note) = next_mode(&ComputerMode::Tap, true, None, Some(4));
        assert_eq!(mode, ComputerMode::Idle);
        assert!(note.unwrap().contains("BlackHole"));
        // An old macOS names the cause, not the permission page.
        let (_, old) = next_mode(&ComputerMode::Tap, true, None, Some(3));
        let (_, refused) = next_mode(&ComputerMode::Tap, true, None, Some(4));
        assert_ne!(old, refused);
        let expected = format!(
            "{} {}",
            crate::locales::t("banner.audio_tap_unsupported"),
            crate::locales::t("banner.audio_install_blackhole")
        );
        assert_eq!(old.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn missing_helper_goes_straight_to_the_loopback() {
        let (mode, _) = next_mode(&ComputerMode::Tap, false, Some("BlackHole 16ch"), None);
        assert_eq!(mode, ComputerMode::BlackHole);
        let (mode, note) = next_mode(&ComputerMode::Tap, false, None, None);
        assert_eq!(mode, ComputerMode::Idle);
        assert!(note.unwrap().contains("momr-audio"));
    }

    #[test]
    fn lost_loopback_prefers_the_tap_again() {
        let (mode, _) = next_mode(
            &ComputerMode::BlackHole,
            true,
            Some("BlackHole 2ch"),
            Some(1),
        );
        assert_eq!(mode, ComputerMode::Tap);
        // Alone with no helper, it retries the loopback and says so.
        let (mode, note) = next_mode(&ComputerMode::BlackHole, false, None, Some(1));
        assert_eq!(mode, ComputerMode::BlackHole);
        assert_eq!(
            note.as_deref(),
            Some(crate::locales::t("banner.audio_device_gone"))
        );
    }

    #[test]
    fn idle_reevaluates_when_something_returns() {
        assert_eq!(
            next_mode(&ComputerMode::Idle, true, None, None).0,
            ComputerMode::Tap
        );
        assert_eq!(
            next_mode(&ComputerMode::Idle, false, Some("BlackHole 2ch"), None).0,
            ComputerMode::BlackHole
        );
    }

    #[test]
    fn capture_commands_name_no_pulseaudio_tool() {
        // The forbidden name is spelled apart so this file stays free of it.
        let forbidden = ["par", "ec"].concat();
        for device in [Device::Mic, Device::Computer] {
            for helper in [None, Some(helper().as_path())] {
                let (program, args) = capture_args(device, helper);
                assert!(!program.contains(&forbidden));
                assert!(args.iter().all(|a| !a.contains(&forbidden)));
            }
        }
        let (program, _) = blackhole_args("BlackHole 2ch");
        assert_eq!(program, "ffmpeg");
    }
}
