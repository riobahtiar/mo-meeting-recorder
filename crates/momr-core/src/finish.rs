//! Finishing a recording from its staging folder: the meeting folder, the
//! audio in the chosen format, the kept tracks, the manifest and the
//! transcript. The step after Stop, and after a crash the step recovery
//! runs.
//!
//! The staging folder (`<cache>/<started-at>/`) holds `mic.raw` and
//! `system.raw` in the D03 byte contract plus `recording.json`, the note of
//! what is known about the recording (title, start, format, language), so a
//! recording can be finished by either shell after the other one crashed.
//! The GTK shell finishes in-process with its own progress animation; the
//! AppKit shell runs `momr finish` (`cli`), so the meeting format stays
//! written by one piece of code instead of a copy per shell.

use std::path::{Path, PathBuf};

use crate::export::{self, Format, RATE};
use crate::meeting::{self, Manifest};
use crate::transcribe::{self, Transcript};
use momr_platform::APP_NAME;

/// The staging note's file name.
pub const NOTE: &str = "recording.json";

/// What is known about a recording in progress, next to its audio, so it
/// can be finished after a crash. A shell that does not choose a format or
/// language (the AppKit shell today) leaves them out, and the reader falls
/// back to the saved settings.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordingNote {
    pub title: String,
    pub started_at: i64,
    pub format: Option<Format>,
    pub language: Option<String>,
    /// Voice enhancement for the saved audio (plan 17).
    pub enhance: Option<bool>,
}

impl RecordingNote {
    /// The format to finish in: the note's, else the one in Settings.
    pub fn format(&self) -> Format {
        self.format.unwrap_or_else(crate::settings::load_format)
    }

    /// Whether to enhance the saved audio: the note's, else Settings'.
    pub fn enhance(&self) -> bool {
        self.enhance.unwrap_or_else(crate::settings::load_enhance)
    }

    /// The transcript language: the note's, else the one in Settings.
    pub fn language(&self) -> String {
        self.language
            .clone()
            .unwrap_or_else(|| crate::settings::load_language().to_owned())
    }
}

/// Writes the note into `staging`. Best effort, as before: a missing note
/// only costs a recovered recording its title.
pub fn write_note(staging: &Path, note: &RecordingNote) {
    let mut value = serde_json::json!({
        "title": note.title,
        "started_at": note.started_at,
    });
    if let Some(format) = note.format {
        value["format"] = format.key().into();
    }
    if let Some(language) = &note.language {
        value["language"] = language.as_str().into();
    }
    if let Some(enhance) = note.enhance {
        value["enhance"] = enhance.into();
    }
    let _ = std::fs::write(staging.join(NOTE), value.to_string());
}

/// Reads the note in `staging`; None without one or with no title.
pub fn read_note(staging: &Path) -> Option<RecordingNote> {
    let text = std::fs::read_to_string(staging.join(NOTE)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(RecordingNote {
        title: value["title"]
            .as_str()
            .filter(|t| !t.is_empty())?
            .to_owned(),
        started_at: value["started_at"].as_i64()?,
        format: value["format"].as_str().map(Format::from_key),
        language: value["language"].as_str().map(str::to_owned),
        enhance: value["enhance"].as_bool(),
    })
}

/// What saving a staging folder's audio produced.
#[derive(Debug, Default, PartialEq)]
pub struct Exported {
    /// The listening files (`audio.ogg`, or the pair) were written.
    pub audio: bool,
    /// `.tracks/` was written from the raw tracks.
    pub tracks: bool,
    /// The listening files are enhanced.
    pub enhanced: bool,
    /// Why enhancement was asked for but not applied.
    pub enhance_problem: Option<String>,
}

/// Writes a staging folder's audio into `out`: `.tracks/` from the raw
/// tracks, always (they are what the transcriber and transcribe-again read,
/// D26), and the listening files in `format`, enhanced when `enhance` and
/// the enhancement works. Enhancement is both sides or neither, so the two
/// sides of a meeting never sound like two different processes. The
/// enhanced copies are removed once encoded; the raw tracks stay.
pub fn export(staging: &Path, out: &Path, format: Format, enhance: bool) -> Exported {
    let (mic, system) = (staging.join("mic.raw"), staging.join("system.raw"));
    if std::fs::create_dir_all(out).is_err() {
        return Exported::default();
    }
    // Padding first, so the kept tracks and the listening files agree in
    // length whichever copy the listening files come from.
    for path in [&mic, &system] {
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path);
    }
    let padded = export::pad_to_same_length([&mic, &system]).is_ok();
    let tracks = padded && export::export_tracks(&mic, &system, out);
    let mut exported = Exported {
        tracks,
        ..Exported::default()
    };
    let (mut listen_mic, mut listen_system) = (mic.clone(), system.clone());
    if enhance {
        let helper = crate::helper::path();
        match (
            crate::enhance::track(&mic, helper.as_deref()),
            crate::enhance::track(&system, helper.as_deref()),
        ) {
            (Ok(m), Ok(s)) => {
                (listen_mic, listen_system) = (m, s);
                exported.enhanced = true;
            }
            (Err(why), other) | (other, Err(why)) => {
                if let Ok(copy) = other
                    && copy != mic
                    && copy != system
                {
                    let _ = std::fs::remove_file(copy);
                }
                exported.enhance_problem =
                    Some(crate::locales::t("enhance.failed").replace("{}", &why));
            }
        }
    }
    exported.audio =
        export::export_audio(&listen_mic, &listen_system, out, format, exported.enhanced);
    for (copy, raw) in [(&listen_mic, &mic), (&listen_system, &system)] {
        if copy != raw {
            let _ = std::fs::remove_file(copy);
        }
    }
    exported
}

/// Recorded seconds in a staging folder, from the size of the longer raw track.
pub fn raw_duration(staging: &Path) -> i64 {
    let bytes = ["mic.raw", "system.raw"]
        .iter()
        .map(|name| std::fs::metadata(staging.join(name)).map_or(0, |m| m.len()))
        .max()
        .unwrap_or(0);
    (bytes / u64::from(RATE * export::CHANNELS * 2)) as i64
}

/// A recording's manifest before its transcript: the speakers are your
/// name and Remote until the transcript says how many voices there were.
pub fn manifest(note: &RecordingNote, duration_secs: i64) -> Manifest {
    Manifest {
        title: note.title.clone(),
        started_at: note.started_at,
        duration_secs,
        format: note.format(),
        language: note.language(),
        speakers: vec![
            crate::settings::load_your_name(),
            meeting::default_remote().to_owned(),
        ],
        imported: None,
        speaker_count: None,
        model: None,
        provider: None,
        chapters: Vec::new(),
        chapters_by: None,
        enhanced: false,
    }
}

/// Fills in what a finished transcript knows: the fitted speakers, the
/// language, the engine and model. Returns the Markdown with this meeting's
/// names, ready for `transcript.md`.
pub fn complete(
    manifest: &mut Manifest,
    transcript: &Transcript,
    provider: crate::provider::Provider,
) -> String {
    let date = meeting::date_line(manifest.started_at);
    let markdown = transcribe::to_markdown(&manifest.title, &date, transcript);
    let markdown = meeting::fit_speakers(manifest, &markdown);
    // The model only means something for a local transcript.
    manifest.model = (provider == crate::provider::Provider::Local).then(crate::models::configured);
    manifest.provider = Some(provider.id().to_owned());
    // Chapters of a previous transcript would point at lines that are gone.
    manifest.chapters.clear();
    manifest.chapters_by = None;
    markdown
}

/// What `momr finish` was asked to do.
#[derive(Debug, PartialEq)]
struct Args {
    staging: PathBuf,
    title: Option<String>,
}

/// Parses `<staging> [--title T]`. Err means print the usage. Pure, so the
/// flags are tested without running anything.
fn parse_args(args: &[String]) -> Result<Args, ()> {
    let mut staging = None;
    let mut title = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--title" | "-t" => title = Some(iter.next().ok_or(())?.clone()),
            flag if flag.starts_with('-') && flag.len() > 1 => return Err(()),
            _ if staging.is_none() => staging = Some(PathBuf::from(arg)),
            _ => return Err(()),
        }
    }
    Ok(Args {
        staging: staging.ok_or(())?,
        title,
    })
}

/// `momr finish <staging> [--title T]`: finishes a stopped recording into
/// its meeting folder with the saved settings. Stages and live lines go to
/// stderr; the meeting folder is printed on stdout as soon as it exists, so
/// a caller can offer it even when the transcript then fails. Exits 0 when
/// audio and transcript are saved, 1 when anything failed (the reason is
/// the last stderr line), 2 for a usage error. The staging folder is removed
/// only when both tracks and the transcript reached the meeting folder.
pub fn cli(args: &[String]) -> u8 {
    let Ok(args) = parse_args(args) else {
        eprintln!("Usage: {APP_NAME} finish <staging folder> [--title T]");
        return 2;
    };
    match run(&args) {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("{APP_NAME}: {message}");
            1
        }
    }
}

fn run(args: &Args) -> Result<(), String> {
    let staging = &args.staging;
    let duration = raw_duration(staging);
    if duration == 0 {
        return Err(format!(
            "{}: {}",
            staging.display(),
            crate::locales::t("help.no_audio")
        ));
    }
    let mut note = read_note(staging).unwrap_or_else(|| RecordingNote {
        title: crate::locales::t("done.recovered_title").to_owned(),
        started_at: crate::ipc::now() - duration,
        format: None,
        language: None,
        enhance: None,
    });
    if let Some(title) = &args.title {
        note.title = title.clone();
    }
    let out = meeting::unused_folder_for(note.started_at, &note.title);
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let (mic, system) = (staging.join("mic.raw"), staging.join("system.raw"));
    eprintln!("{}", crate::locales::t("help.stages_saving"));
    let exported = export(staging, &out, note.format(), note.enhance());
    if let Some(problem) = &exported.enhance_problem {
        // Said, not fatal: the meeting is saved without enhancement.
        eprintln!("{APP_NAME}: {problem}");
    }
    let (audio, tracks) = (exported.audio, exported.tracks);
    let mut manifest = manifest(&note, duration);
    manifest.enhanced = exported.enhanced;
    meeting::write(&out, &manifest).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("{}", out.display());

    let provider = crate::provider::configured()?;
    let language = manifest.language.clone();
    let transcript = transcribe::run_reporting(|events, abort| {
        let mic = transcribe::load_track(&mic)?;
        let computer = transcribe::load_track(&system)?;
        transcribe::transcribe(&mic, &computer, &language, provider, events, abort)
    })
    .map_err(|e| crate::locales::t("help.failed").replace("{}", &e))?;
    let markdown = complete(&mut manifest, &transcript, provider);
    meeting::write(&out, &manifest).map_err(|e| format!("{}: {e}", out.display()))?;
    std::fs::write(out.join("transcript.md"), markdown)
        .map_err(|e| crate::locales::t("help.write_failed").replace("{}", &e.to_string()))?;
    if !audio || !tracks {
        // The transcript is saved, but the raw tracks stay for another try.
        return Err(crate::locales::t("help.no_audio").to_owned());
    }
    // The kept tracks are enough to transcribe again; the raw files can go.
    let _ = std::fs::remove_dir_all(staging);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_note_round_trips_and_may_leave_format_out() {
        let dir = std::env::temp_dir().join(format!("momr-note-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let full = RecordingNote {
            title: "Weekly".into(),
            started_at: 1_790_000_000,
            format: Some(Format::Separate),
            language: Some("id".into()),
            enhance: Some(true),
        };
        write_note(&dir, &full);
        assert_eq!(read_note(&dir), Some(full));
        // The AppKit shell writes only what it knows.
        std::fs::write(dir.join(NOTE), r#"{"title":"Sync","started_at":5}"#).unwrap();
        let bare = read_note(&dir).unwrap();
        assert_eq!(
            (bare.format, bare.language, bare.enhance),
            (None, None, None)
        );
        std::fs::write(dir.join(NOTE), r#"{"title":"","started_at":5}"#).unwrap();
        assert_eq!(read_note(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn raw_duration_reads_the_longer_track() {
        let dir = std::env::temp_dir().join(format!("momr-raw-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let second = (RATE * export::CHANNELS * 2) as usize;
        std::fs::write(dir.join("mic.raw"), vec![0u8; second * 3]).unwrap();
        std::fs::write(dir.join("system.raw"), vec![0u8; second]).unwrap();
        assert_eq!(raw_duration(&dir), 3);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(raw_duration(&dir), 0);
    }

    #[test]
    fn finish_takes_a_staging_folder_and_an_optional_title() {
        assert_eq!(
            parse_args(&strings(&["/tmp/1790000000", "--title", "Weekly"])),
            Ok(Args {
                staging: "/tmp/1790000000".into(),
                title: Some("Weekly".into()),
            })
        );
        assert_eq!(parse_args(&strings(&["/tmp/1"])).map(|a| a.title), Ok(None));
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&strings(&["/tmp/1", "/tmp/2"])).is_err());
        assert!(parse_args(&strings(&["/tmp/1", "--title"])).is_err());
        assert!(parse_args(&strings(&["/tmp/1", "--format", "mono"])).is_err());
    }
}
