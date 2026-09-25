//! Voice enhancement (plan 17): cleaner voices in the audio a person
//! listens to, never in what the transcriber reads (D26).
//!
//! Two stages, chosen in plans/research/voice-enhancement.md. First the
//! helper's `momr-audio enhance` runs each raw track through Apple's
//! AUSoundIsolation (noise, wind, traffic and hum out), writing an
//! `*.enhanced.raw` beside it in the staging folder. Then export shapes the
//! voice with `VOICE_CHAIN`, plain ffmpeg filters. The raw tracks stay as
//! they were: `.tracks/` is encoded from them and transcription reads them.
//!
//! Enhancement is best effort. A Mac without the unit, a helper that is
//! missing or a render that fails leaves the track unenhanced and says why;
//! the meeting is saved either way.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The helper's `enhance` exit codes (helpers/momr-audio/…/Enhance.swift).
const EXIT_UNAVAILABLE: i32 = 8;

/// After isolation: rumble and wind out below 80 Hz, a little mud out at
/// 250 Hz, presence up at 3.5 kHz, sibilance tamed, and a gentle compressor
/// so quiet words sit closer to loud ones, the way a close studio mic
/// sounds. Every filter is in the stock ffmpeg build the app bundles.
pub const VOICE_CHAIN: &str = "highpass=f=80,\
equalizer=f=250:t=q:w=1:g=-2,\
equalizer=f=3500:t=q:w=1.2:g=3,\
deesser,\
acompressor=threshold=0.1:ratio=2.5:attack=10:release=200";

/// Where the enhanced copy of `raw` goes: `mic.raw` → `mic.enhanced.raw`.
pub fn enhanced_path(raw: &Path) -> PathBuf {
    raw.with_extension("enhanced.raw")
}

/// Isolates the voice in `raw` through the helper at `helper`, returning the
/// enhanced copy. An empty track (a side not recorded) needs no work and
/// comes back as is. Err says why the track stays unenhanced.
pub fn track(raw: &Path, helper: Option<&Path>) -> Result<PathBuf, String> {
    track_with(raw, helper, |command| command.output())
}

/// `track` with the helper run by `run`, so tests can fake it.
fn track_with(
    raw: &Path,
    helper: Option<&Path>,
    run: impl FnOnce(&mut Command) -> std::io::Result<Output>,
) -> Result<PathBuf, String> {
    if std::fs::metadata(raw).map_or(0, |m| m.len()) == 0 {
        return Ok(raw.to_path_buf());
    }
    let helper = helper.ok_or(crate::locales::t("enhance.no_helper"))?;
    let out = enhanced_path(raw);
    let output =
        run(Command::new(helper).arg("enhance").arg(raw).arg(&out)).map_err(|e| e.to_string())?;
    if output.status.success() && out.is_file() {
        return Ok(out);
    }
    let _ = std::fs::remove_file(&out);
    if output.status.code() == Some(EXIT_UNAVAILABLE) {
        return Err(crate::locales::t("enhance.unavailable").to_owned());
    }
    let reason = String::from_utf8_lossy(&output.stderr);
    Err(reason
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map_or_else(
            || format!("momr-audio enhance exited with {}", output.status),
            |line| line.trim().to_owned(),
        ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("momr-enhance-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn exit(code: i32, stderr: &str) -> Output {
        Output {
            status: ExitStatus::from_raw(code << 8),
            stdout: Vec::new(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    #[test]
    fn the_helper_writes_the_enhanced_copy_beside_the_raw_track() {
        let dir = scratch("ok");
        let raw = dir.join("mic.raw");
        std::fs::write(&raw, [1u8; 8]).unwrap();
        let got = track_with(&raw, Some(Path::new("/h/momr-audio")), |command| {
            let args: Vec<_> = command.get_args().map(|a| a.to_owned()).collect();
            assert_eq!(command.get_program(), "/h/momr-audio");
            assert_eq!(args[0], "enhance");
            assert_eq!(args[1], raw.as_os_str());
            std::fs::write(&args[2], [2u8; 8]).unwrap();
            Ok(exit(0, ""))
        });
        assert_eq!(got, Ok(dir.join("mic.enhanced.raw")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_track_not_recorded_is_not_sent_to_the_helper() {
        let dir = scratch("empty");
        let raw = dir.join("system.raw");
        std::fs::write(&raw, []).unwrap();
        let got = track_with(&raw, None, |_| panic!("no helper run for an empty track"));
        assert_eq!(got, Ok(raw));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failures_leave_the_track_unenhanced_and_say_why() {
        let dir = scratch("fail");
        let raw = dir.join("mic.raw");
        std::fs::write(&raw, [1u8; 8]).unwrap();
        let helper = Some(Path::new("/h/momr-audio"));
        assert!(track_with(&raw, None, |_| unreachable!()).is_err());
        let unavailable = track_with(&raw, helper, |_| Ok(exit(EXIT_UNAVAILABLE, "")));
        assert_eq!(
            unavailable,
            Err(crate::locales::t("enhance.unavailable").into())
        );
        let failed = track_with(&raw, helper, |command| {
            let out: PathBuf = command.get_args().nth(2).unwrap().into();
            std::fs::write(&out, [0u8; 2]).unwrap();
            Ok(exit(9, "momr-audio: enhance: render stalled (1)\n"))
        });
        assert_eq!(
            failed,
            Err("momr-audio: enhance: render stalled (1)".into())
        );
        assert!(
            !enhanced_path(&raw).exists(),
            "a failed render leaves no half file"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
