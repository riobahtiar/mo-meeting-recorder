//! Playing a meeting: one ffmpeg decoding to the speakers, stopped when
//! dropped. The input side (seek, mix) is built here; the output comes from
//! `momr_platform::playback::OUTPUT`, the one part that differs per OS, so no
//! shell names a sink. Position is the start offset plus the wall clock
//! times the speed; seeking restarts the process at the new offset.
//!
//! Speed is ffmpeg's `atempo` (pitch kept, the range it takes, 0.5–2×) and
//! volume its `volume` filter, both after the mix and always in the graph,
//! so they can be changed while playing: ffmpeg reads commands on stdin
//! (`c<target> -1 <command> <value>`), which is how a volume slider moves
//! without restarting the process and without a stutter. Measured: a
//! `volume 0.1` command took the level down 20 dB mid-stream, `ret:0`.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Waveform bins per track. The shell draws this many columns per lane.
pub const BINS: usize = 1000;

/// How long a stopped playback gets to end on SIGTERM before SIGKILL.
const STOP_GRACE: Duration = Duration::from_millis(500);

/// The playback speeds the shells offer, slowest first.
pub const SPEEDS: [f64; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];
/// The loudest playback volume, as a gain: 150 % helps a quiet meeting.
pub const MAX_VOLUME: f64 = 1.5;

/// How a playback sounds: speed (0.5–2×) and volume (0–`MAX_VOLUME`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sound {
    pub speed: f64,
    pub volume: f64,
}

impl Default for Sound {
    fn default() -> Self {
        Sound {
            speed: 1.0,
            volume: 1.0,
        }
    }
}

impl Sound {
    /// Clamped to what ffmpeg's filters take and the shells offer.
    pub fn clamped(self) -> Sound {
        Sound {
            speed: self.speed.clamp(SPEEDS[0], SPEEDS[SPEEDS.len() - 1]),
            volume: self.volume.clamp(0.0, MAX_VOLUME),
        }
    }

    /// The filters after the mix. Both are always there, at 1× and 100 %
    /// too, because a live command needs a filter to reach.
    fn filters(self) -> String {
        format!("atempo={:.3},volume={:.3}", self.speed, self.volume)
    }
}

/// The stdin line that sets `command` on every filter that takes it:
/// ffmpeg's interactive `c` command, target `-1` meaning now.
fn command_line(filter: &str, command: &str, value: f64) -> String {
    format!("c{filter} -1 {command} {value:.3}\n")
}

/// Whether a stderr line is ffmpeg answering a command rather than a
/// reason to stop, so the last-line reason is never "Command reply".
fn is_command_echo(line: &str) -> bool {
    line.starts_with("Enter command") || line.starts_with("Command reply")
}

/// A running ffmpeg playback, stopped when dropped.
pub struct Playback {
    ffmpeg: Child,
    started: Instant,
    from_us: i64,
    speed: f64,
    /// ffmpeg's stdin, for the live speed and volume commands.
    commands: Option<std::process::ChildStdin>,
    /// The last line ffmpeg wrote to stderr, read on its own thread so a
    /// chatty ffmpeg never fills the pipe.
    last_error: std::sync::Arc<std::sync::Mutex<String>>,
    /// That thread, so `ended` can wait for the last line instead of guessing.
    reader: Option<std::thread::JoinHandle<()>>,
}

/// ffmpeg's arguments for `files` mixed from `from_us` into `output`: a
/// `-ss` before each `-i` (so every input seeks, not the mix), `amix` only
/// when there is more than one, then the `sound` filters. Pure, so the
/// shape is tested.
fn args(files: &[PathBuf], from_us: i64, sound: Sound, output: &[&str]) -> Vec<std::ffi::OsString> {
    let at = format!("{:.3}", from_us.max(0) as f64 / 1_000_000.0);
    // No `-nostdin`: stdin carries the live commands.
    let mut args: Vec<std::ffi::OsString> = ["-v", "error"].map(Into::into).into();
    for file in files {
        args.extend(["-ss".into(), at.clone().into(), "-i".into(), file.into()]);
    }
    let after = sound.clamped().filters();
    if files.len() > 1 {
        let mix = format!("amix=inputs={}:normalize=0", files.len());
        args.push("-filter_complex".into());
        args.push(format!("{mix},{after}").into());
    } else {
        args.push("-af".into());
        args.push(after.into());
    }
    args.extend(output.iter().map(Into::into));
    args
}

impl Playback {
    /// Starts `files` mixed together at `from_us` (clamped to the start),
    /// sounding as `sound`.
    pub fn start(files: &[PathBuf], from_us: i64, sound: Sound) -> Result<Playback, String> {
        if files.is_empty() {
            return Err("no audio to play".into());
        }
        let output = momr_platform::playback::OUTPUT
            .ok_or("playback is not available on this platform yet (plan 16)")?;
        let from_us = from_us.max(0);
        let sound = sound.clamped();
        let mut ffmpeg = guarded("ffmpeg", crate::helper::path().as_deref())
            .args(args(files, from_us, sound, output))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        let commands = ffmpeg.stdin.take();
        let last_error = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let reader = ffmpeg.stderr.take().map(|pipe| {
            let last_error = last_error.clone();
            std::thread::spawn(move || {
                use std::io::BufRead;
                for line in std::io::BufReader::new(pipe).lines().map_while(Result::ok) {
                    let line = line.trim();
                    if !line.is_empty() && !is_command_echo(line) {
                        *last_error.lock().unwrap() = line.to_owned();
                    }
                }
            })
        });
        Ok(Playback {
            ffmpeg,
            started: Instant::now(),
            from_us,
            speed: sound.speed,
            commands,
            last_error,
            reader,
        })
    }

    /// Sets the volume while playing. Err when ffmpeg no longer listens,
    /// and the caller restarts it instead.
    pub fn set_volume(&mut self, volume: f64) -> Result<(), String> {
        let volume = volume.clamp(0.0, MAX_VOLUME);
        self.send(&command_line("volume", "volume", volume))
    }

    /// Sets the speed while playing. The clock is rebased first, so the
    /// position stays right across the change.
    pub fn set_speed(&mut self, speed: f64) -> Result<(), String> {
        let speed = speed.clamp(SPEEDS[0], SPEEDS[SPEEDS.len() - 1]);
        self.send(&command_line("atempo", "tempo", speed))?;
        self.from_us = self.position_us();
        self.started = Instant::now();
        self.speed = speed;
        Ok(())
    }

    fn send(&mut self, line: &str) -> Result<(), String> {
        use std::io::Write;
        let stdin = self.commands.as_mut().ok_or("ffmpeg has no stdin")?;
        stdin
            .write_all(line.as_bytes())
            .and_then(|()| stdin.flush())
            .map_err(|e| e.to_string())
    }

    /// Where the playhead is, if the process is still the one started.
    pub fn position_us(&self) -> i64 {
        self.from_us + (self.started.elapsed().as_micros() as f64 * self.speed) as i64
    }

    /// None while playing; once ended, Ok for the end of the file or Err
    /// with ffmpeg's reason (no audio output, an unreadable file).
    pub fn ended(&mut self) -> Option<Result<(), String>> {
        let status = self.ffmpeg.try_wait().ok()??;
        if status.success() {
            return Some(Ok(()));
        }
        // The reader ends at EOF, which follows the exit; wait for it, but
        // never long: a grandchild holding the pipe must not stall the UI.
        let deadline = Instant::now() + Duration::from_millis(50);
        while self.reader.as_ref().is_some_and(|r| !r.is_finished()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        let reason = self.last_error.lock().unwrap().clone();
        Some(Err(if reason.is_empty() {
            format!("ffmpeg exit {}", status.code().unwrap_or(-1))
        } else {
            reason
        }))
    }
}

impl Drop for Playback {
    /// SIGTERM first: with the helper, `ffmpeg` is really the `momr-audio run`
    /// wrapper, which forwards SIGTERM to the real ffmpeg but cannot catch
    /// SIGKILL, so a bare kill would leave the meeting playing. SIGKILL only
    /// follows when the grace period runs out.
    fn drop(&mut self) {
        // Stdin goes first; the signals below are what stop ffmpeg.
        self.commands = None;
        stop(&mut self.ffmpeg, STOP_GRACE);
    }
}

/// Ends `child` with SIGTERM, then SIGKILL after `grace`, and reaps it.
fn stop(child: &mut Child, grace: Duration) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    if let Err(e) = momr_platform::process::terminate(child.id())
        && !momr_platform::process::already_gone(&e)
    {
        // The SIGKILL below still ends the wrapper, but not what it runs.
        eprintln!("{}: stop playback: {e}", momr_platform::APP_NAME);
    }
    let deadline = Instant::now() + grace;
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// A command for `program` whose process dies when this app does, even after
/// a crash: through the `momr-audio run` wrapper, which watches the parent
/// pid and kills the child when it goes away. Without a helper there is no
/// watchdog; `Drop` still kills on a clean exit.
fn guarded(program: &str, helper: Option<&Path>) -> Command {
    match helper {
        Some(helper) => {
            let mut command = Command::new(helper);
            command.args(["run", "--", program]);
            command
        }
        None => Command::new(program),
    }
}

/// Length of an audio file in microseconds, from ffprobe; None when ffprobe
/// cannot read it, which is not the same as an empty file.
pub fn probe_duration_us(path: &Path) -> Option<i64> {
    Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .and_then(|text| text.trim().parse::<f64>().ok())
        .map(|secs| (secs * 1_000_000.0) as i64)
}

/// How fast `peaks` decodes: enough for the loudest sample per bin.
const PEAK_RATE: usize = 4000;

/// Decodes `path` at a low rate and keeps the loudest sample per bin, scaled
/// to 0..1 with a gentle curve so quiet speech still shows. Also returns the
/// decoded length in microseconds, the player's duration when ffprobe
/// cannot give one.
pub fn peaks(path: &Path) -> Option<(Vec<f32>, i64)> {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args([
            "-ac",
            "1",
            "-ar",
            &PEAK_RATE.to_string(),
            "-f",
            "s16le",
            "-",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let samples: Vec<f32> = output
        .stdout
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| f32::from(i16::from_le_bytes(*b)).abs() / 32768.0)
        .collect();
    let length_us = (samples.len() as i64) * 1_000_000 / PEAK_RATE as i64;
    if samples.is_empty() {
        return Some((vec![0.0; BINS], 0));
    }
    let per_bin = samples.len().div_ceil(BINS);
    let mut bins: Vec<f32> = samples
        .chunks(per_bin)
        .map(|chunk| chunk.iter().copied().fold(0.0, f32::max))
        .collect();
    bins.resize(BINS, 0.0);
    let loudest = bins.iter().copied().fold(0.0, f32::max).max(1e-4);
    Some((
        bins.into_iter().map(|v| (v / loudest).sqrt()).collect(),
        length_us,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_input_seeks_and_only_several_are_mixed() {
        let files = [PathBuf::from("a.ogg"), PathBuf::from("b.ogg")];
        let out = ["-f", "audiotoolbox", "-"];
        let two: Vec<String> = args(&files, 1_500_000, Sound::default(), &out)
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            two,
            [
                "-v",
                "error",
                "-ss",
                "1.500",
                "-i",
                "a.ogg",
                "-ss",
                "1.500",
                "-i",
                "b.ogg",
                "-filter_complex",
                "amix=inputs=2:normalize=0,atempo=1.000,volume=1.000",
                "-f",
                "audiotoolbox",
                "-"
            ]
        );
        let one: Vec<String> = args(&files[..1], -5, Sound::default(), &out)
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(!one.iter().any(|a| a == "-filter_complex"));
        assert_eq!(one[2..4], ["-ss", "0.000"], "a negative start plays from 0");
        assert!(
            !one.iter().any(|a| a == "-nostdin"),
            "stdin carries the live commands"
        );
    }

    /// Speed and volume ride after the mix, pitch kept, clamped to the
    /// range the shells offer, and the live commands address those filters.
    #[test]
    fn speed_and_volume_follow_the_mix_and_change_live() {
        let files = [PathBuf::from("a.ogg")];
        let out = ["-f", "audiotoolbox", "-"];
        let sound = Sound {
            speed: 1.5,
            volume: 0.5,
        };
        let one: Vec<String> = args(&files, 0, sound, &out)
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let at = one.iter().position(|a| a == "-af").unwrap();
        assert_eq!(one[at + 1], "atempo=1.500,volume=0.500");
        let wild = Sound {
            speed: 9.0,
            volume: 7.0,
        };
        assert_eq!(
            wild.clamped(),
            Sound {
                speed: 2.0,
                volume: MAX_VOLUME
            }
        );
        assert_eq!(
            command_line("volume", "volume", 0.25),
            "cvolume -1 volume 0.250\n"
        );
        assert_eq!(
            command_line("atempo", "tempo", 1.5),
            "catempo -1 tempo 1.500\n"
        );
        assert!(is_command_echo("Command reply for stream -1: ret:0 res:"));
        assert!(!is_command_echo("audiotoolbox: no output device"));
    }

    #[test]
    fn nothing_to_play_is_an_error() {
        assert!(Playback::start(&[], 0, Sound::default()).is_err());
    }

    #[test]
    fn guarded_wraps_in_the_helper() {
        let helper = Path::new("/Applications/MOM Recorder.app/Contents/MacOS/momr-audio");
        let command = guarded("ffmpeg", Some(helper));
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            command.get_program().to_string_lossy(),
            helper.display().to_string()
        );
        assert_eq!(args, ["run", "--", "ffmpeg"]);
    }

    /// The wrapper cannot pass on a SIGKILL, so stopping must reach the real
    /// child through SIGTERM. Unix-only: it spawns `sleep` and reads it back
    /// with `pgrep`.
    #[cfg(unix)]
    #[test]
    fn stopping_a_guarded_child_stops_what_it_runs() {
        // The helper as scripts/build-macos.sh or `swift build` leaves it,
        // found from the workspace root: tests run with the package dir as
        // CWD, where a relative path would never match and the test would
        // skip without anyone noticing.
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let built = ["release", "debug"]
            .iter()
            .map(|profile| root.join(format!("helpers/momr-audio/.build/{profile}/momr-audio")))
            .find(|p| p.is_file());
        let Some(helper) = crate::helper::path().or(built) else {
            eprintln!("skipped: no momr-audio helper built");
            return;
        };
        let marker = format!("{}", 424_200 + std::process::id() % 1000);
        let mut command = guarded("sleep", Some(&helper));
        let mut child = command.arg(&marker).spawn().expect("wrapper starts");
        std::thread::sleep(Duration::from_millis(300));
        stop(&mut child, STOP_GRACE);
        std::thread::sleep(Duration::from_millis(700));
        let survivors = Command::new("pgrep")
            .args(["-f", &format!("^sleep {marker}$")])
            .output()
            .expect("pgrep runs");
        assert!(survivors.stdout.is_empty(), "sleep outlived its wrapper");
    }

    #[test]
    fn guarded_without_a_helper_runs_bare() {
        let command = guarded("ffmpeg", None);
        assert_eq!(command.get_program().to_string_lossy(), "ffmpeg");
        assert!(command.get_args().next().is_none());
    }
}
