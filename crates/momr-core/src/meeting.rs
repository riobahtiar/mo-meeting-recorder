//! The `.meeting-recorder` file: a small JSON manifest inside every meeting folder.
//!
//! The folder stays a plain folder, so the audio and the transcript are easy to
//! reach with any other tool. The manifest carries what the folder cannot say
//! for itself (the title, when it started, the chosen format and language) and
//! has its own MIME type, so double-clicking it opens the meeting in the app.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::export::Format;

pub const EXTENSION: &str = "meeting-recorder";
/// The default speaker labels. They are transcript content, not chrome:
/// `transcript.md` is read by users' scripts and shared with upstream, so
/// the labels stay English whatever the interface language, and a meeting
/// from either app renames the same way.
pub const DEFAULT_YOU: &str = "You";
pub const DEFAULT_REMOTE: &str = "Remote";
const REMOTE_PREFIX: &str = "Remote ";
const SPEAKER_PREFIX: &str = "Speaker ";

pub fn default_you() -> &'static str {
    DEFAULT_YOU
}

pub fn default_remote() -> &'static str {
    DEFAULT_REMOTE
}

/// "Remote N" for several voices on the computer audio.
pub fn remote_n(n: usize) -> String {
    format!("{REMOTE_PREFIX}{n}")
}

/// "Speaker N" for imports and fallbacks.
pub fn speaker_n(n: usize) -> String {
    format!("{SPEAKER_PREFIX}{n}")
}

/// The N of a `remote_n` label, the inverse kept next to the formatter so the
/// two cannot drift apart.
pub fn parse_remote_n(label: &str) -> Option<usize> {
    label.strip_prefix(REMOTE_PREFIX)?.parse().ok()
}

/// The N of a `speaker_n` label.
pub fn parse_speaker_n(label: &str) -> Option<usize> {
    label.strip_prefix(SPEAKER_PREFIX)?.parse().ok()
}

/// One chapter of a meeting: where it starts and what it is called. Stored
/// in the manifest and written into `transcript.md`, so it lives with the
/// meeting data rather than with the agent that made it.
#[derive(Clone, Debug, PartialEq)]
pub struct Chapter {
    pub start_ms: i64,
    pub title: String,
}

#[derive(Clone, Debug)]
pub struct Manifest {
    pub title: String,
    pub started_at: i64,
    pub duration_secs: i64,
    pub format: Format,
    /// The language picked for the transcript: a code from `LANGUAGES`, or "auto".
    pub language: String,
    /// What the speakers are called, in the order of their default labels:
    /// for a recording [microphone, computer audio], for an imported file
    /// [Speaker 1, Speaker 2, ...].
    pub speakers: Vec<String>,
    /// The original file name when the meeting was imported instead of recorded.
    pub imported: Option<String>,
    /// How many speakers were asked for on import; None means automatic.
    pub speaker_count: Option<usize>,
    /// The whisper model the transcript was made with, when it was local.
    pub model: Option<String>,
    /// Which engine made the transcript: a `provider::Provider` id such as
    /// "local" or "elevenlabs". Absent in meetings from upstream and from
    /// before providers existed; older readers ignore the key.
    pub provider: Option<String>,
    /// Chapters made by an agent, empty when there are none.
    pub chapters: Vec<Chapter>,
    /// Which agent made them, e.g. "claude".
    pub chapters_by: Option<String>,
}

impl Manifest {
    fn to_json(&self) -> Value {
        json!({
            "app": momr_platform::APP_NAME,
            "version": 1,
            "title": self.title,
            "started_at": self.started_at,
            "duration_secs": self.duration_secs,
            "format": self.format.key(),
            "language": self.language,
            "speakers": self.speakers,
            "imported": self.imported,
            "speaker_count": self.speaker_count,
            "model": self.model,
            "provider": self.provider,
            "chapters": self.chapters.iter()
                .map(|c| json!({ "start_ms": c.start_ms, "title": c.title }))
                .collect::<Vec<_>>(),
            "chapters_by": self.chapters_by,
        })
    }

    fn from_json(value: &Value) -> Option<Manifest> {
        Some(Manifest {
            title: value["title"].as_str()?.to_owned(),
            started_at: value["started_at"].as_i64()?,
            duration_secs: value["duration_secs"].as_i64().unwrap_or(0),
            format: Format::from_key(value["format"].as_str().unwrap_or("mono")),
            language: value["language"].as_str().unwrap_or("auto").to_owned(),
            speakers: match &value["speakers"] {
                // Before imports existed, a recording kept {"you", "remote"}.
                Value::Object(_) => vec![
                    name(&value["speakers"]["you"], default_you()),
                    name(&value["speakers"]["remote"], default_remote()),
                ],
                Value::Array(list) => list
                    .iter()
                    .enumerate()
                    .map(|(i, v)| name(v, &speaker_n(i + 1)))
                    .collect(),
                _ => vec![default_you().to_owned(), default_remote().to_owned()],
            },
            imported: value["imported"].as_str().map(str::to_owned),
            speaker_count: value["speaker_count"].as_u64().map(|n| n as usize),
            model: value["model"].as_str().map(str::to_owned),
            provider: value["provider"].as_str().map(str::to_owned),
            chapters: value["chapters"]
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|c| {
                            Some(Chapter {
                                start_ms: c["start_ms"].as_i64()?,
                                title: c["title"].as_str()?.to_owned(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
            chapters_by: value["chapters_by"].as_str().map(str::to_owned),
        })
    }
}

impl Manifest {
    /// The labels the transcription gives the speakers, in the order of
    /// `speakers`: You and Remote (or Remote 1, Remote 2, ... when the computer
    /// audio holds several voices) for a recording, Speaker N for an import.
    pub fn default_labels(&self) -> Vec<String> {
        if self.imported.is_some() {
            (1..=self.speakers.len().max(1)).map(speaker_n).collect()
        } else if self.speakers.len() > 2 {
            // Several voices on the computer audio: Remote 1, Remote 2, ...
            std::iter::once(default_you().to_owned())
                .chain((1..self.speakers.len()).map(remote_n))
                .collect()
        } else {
            vec![default_you().to_owned(), default_remote().to_owned()]
        }
    }
}

/// Renames several speakers at once, safe when names swap places: every old
/// name goes through a placeholder first.
pub fn relabel_all(markdown: &str, renames: &[(String, String)]) -> String {
    let mut text = markdown.to_owned();
    for (i, (from, _)) in renames.iter().enumerate() {
        text = relabel(&text, from, &format!("\u{1}{i}\u{1}"));
    }
    for (i, (_, to)) in renames.iter().enumerate() {
        text = relabel(&text, &format!("\u{1}{i}\u{1}"), to);
    }
    text
}

/// Splits `**[01:23] You:** text` into its time, speaker and text.
pub fn parse_segment(line: &str) -> Option<(&str, &str, &str)> {
    let rest = line.strip_prefix("**[")?;
    let (time, rest) = rest.split_once("] ")?;
    let (speaker, text) = rest.split_once(":** ")?;
    Some((time, speaker, text.trim()))
}

/// The speaker labels in transcript Markdown, in order of first appearance.
pub fn speakers_in(markdown: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for line in markdown.lines() {
        if let Some((_, speaker, _)) = parse_segment(line)
            && !found.iter().any(|f| f == speaker)
        {
            found.push(speaker.to_owned());
        }
    }
    found
}

/// Fits `manifest.speakers` to the voices a fresh transcript found, then
/// writes this meeting's names over the transcription's default labels.
/// Both shells finish a transcript through here, so a meeting's names come
/// out the same whichever shell recorded it.
pub fn fit_speakers(manifest: &mut Manifest, markdown: &str) -> String {
    if manifest.imported.is_some() {
        // An import finds its own number of speakers; keep names already
        // given and number the rest.
        let found = speakers_in(markdown);
        let mut names = manifest.speakers.clone();
        names.resize_with(found.len(), String::new);
        for (i, name) in names.iter_mut().enumerate() {
            if name.is_empty() {
                *name = speaker_n(i + 1);
            }
        }
        manifest.speakers = names;
    } else {
        // Several voices on the computer audio come out as Remote 1,
        // Remote 2, ...: one name each, after your own.
        let remotes = speakers_in(markdown)
            .iter()
            .filter_map(|s| parse_remote_n(s))
            .max()
            .unwrap_or(0);
        if remotes > 1 {
            let mut names = manifest.speakers.clone();
            if names.len() <= 2 {
                names.truncate(1);
            }
            while names.len() < remotes + 1 {
                let n = names.len();
                names.push(remote_n(n));
            }
            names.truncate(remotes + 1);
            manifest.speakers = names;
        } else if manifest.speakers.len() > 2 {
            manifest.speakers.truncate(2);
            manifest.speakers[1] = default_remote().to_owned();
        }
    }
    let renames: Vec<(String, String)> = manifest
        .default_labels()
        .into_iter()
        .zip(manifest.speakers.iter().cloned())
        .filter(|(label, name)| label != name)
        .collect();
    relabel_all(markdown, &renames)
}

/// The local time a meeting started, as `%Y-%m-%d %H:%M` for the
/// transcript's heading; empty for a time the clock cannot show.
pub fn date_line(started_at: i64) -> String {
    local_time(started_at, "%Y-%m-%d %H:%M")
}

/// `started_at` in local time with a chrono `format`, empty when the
/// timestamp is out of range.
fn local_time(started_at: i64, format: &str) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(started_at, 0)
        .earliest()
        .map(|t| t.format(format).to_string())
        .unwrap_or_default()
}

/// The folder a meeting started at `started_at` and called `title` gets
/// under the meetings folder: `202609241400 Weekly sync`. `from_folder`
/// reads the same shape back.
pub fn folder_for(started_at: i64, title: &str) -> PathBuf {
    let stamp = local_time(started_at, "%Y%m%d%H%M");
    crate::settings::meetings_dir().join(format!("{stamp} {}", crate::export::safe_name(title)))
}

/// `folder_for`, numbered (`… Weekly sync 2`) when that folder exists
/// already: two meetings in the same minute with the same title must not
/// share one folder and overwrite each other's audio.
pub fn unused_folder_for(started_at: i64, title: &str) -> PathBuf {
    let mut out = folder_for(started_at, title);
    let mut n = 2;
    while out.exists() {
        out = folder_for(started_at, &format!("{title} {n}"));
        n += 1;
    }
    out
}

fn name(value: &Value, fallback: &str) -> String {
    value
        .as_str()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(fallback)
        .to_owned()
}

/// Renames a speaker in transcript Markdown: `**[01:23] Old:**` becomes `**[01:23] New:**`.
pub fn relabel(markdown: &str, from: &str, to: &str) -> String {
    if from == to {
        return markdown.to_owned();
    }
    markdown
        .lines()
        .map(|line| {
            match line
                .strip_prefix("**[")
                .and_then(|rest| rest.split_once("] "))
            {
                Some((time, rest)) if rest.starts_with(&format!("{from}:** ")) => {
                    format!("**[{time}] {to}{}", &rest[from.len()..])
                }
                _ => line.to_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + if markdown.ends_with('\n') { "\n" } else { "" }
}

/// The manifest's path for `title` inside `dir`.
pub fn path_for(dir: &Path, title: &str) -> PathBuf {
    dir.join(format!("{}.{EXTENSION}", crate::export::safe_name(title)))
}

/// Finds the manifest in a meeting folder, whatever its name.
pub fn find(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|p| p.extension().is_some_and(|e| e == EXTENSION) && p.is_file())
}

/// Writes the manifest into `dir`, replacing one with another name (after a rename).
pub fn write(dir: &Path, manifest: &Manifest) -> std::io::Result<PathBuf> {
    let target = path_for(dir, &manifest.title);
    if let Some(old) = find(dir)
        && old != target
    {
        let _ = std::fs::remove_file(old);
    }
    let text = serde_json::to_string_pretty(&manifest.to_json()).unwrap_or_default() + "\n";
    std::fs::write(&target, text)?;
    Ok(target)
}

/// Reads a meeting from a `.meeting-recorder` file or from its folder. A folder without
/// a manifest (made before manifests existed) is read from its name and files.
pub fn open(path: &Path) -> Option<(PathBuf, Manifest)> {
    let (dir, file) = if path.is_dir() {
        (path.to_path_buf(), find(path))
    } else {
        (path.parent()?.to_path_buf(), Some(path.to_path_buf()))
    };
    let from_file = file
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| Manifest::from_json(&value));
    let manifest = from_file.or_else(|| from_folder(&dir))?;
    Some((dir, manifest))
}

/// `202609241400 Weekly sync` plus whatever audio files are there.
fn from_folder(dir: &Path) -> Option<Manifest> {
    let name = dir.file_name()?.to_str()?;
    let (stamp, title) = name.split_once(' ')?;
    if stamp.len() != 12 || !stamp.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let num = |range: std::ops::Range<usize>| stamp[range].parse::<i32>().ok();
    let date = chrono::NaiveDate::from_ymd_opt(num(0..4)?, num(4..6)? as u32, num(6..8)? as u32)?;
    let time = chrono::NaiveTime::from_hms_opt(num(8..10)? as u32, num(10..12)? as u32, 0)?;
    // The hour DST repeats reads as its first pass, as glib read it.
    let started = date
        .and_time(time)
        .and_local_timezone(chrono::Local)
        .earliest()?;
    let format = if dir.join("mic.ogg").exists() {
        Format::Separate
    } else {
        Format::Mono
    };
    Some(Manifest {
        title: title.to_owned(),
        started_at: started.timestamp(),
        duration_secs: 0,
        format,
        language: "auto".to_owned(),
        speakers: vec![default_you().to_owned(), default_remote().to_owned()],
        imported: None,
        speaker_count: None,
        model: None,
        provider: None,
        chapters: Vec::new(),
        chapters_by: None,
    })
}

#[cfg(test)]
mod tests {
    use super::{Chapter, Format, Manifest, json, relabel};

    const MD: &str = "# Weekly\n\n**[00:01] You:** Hi.\n\n**[00:03] Remote:** You: said hi.\n";

    #[test]
    fn renames_only_the_speaker_labels() {
        let out = relabel(MD, "You", "Jankees");
        assert!(out.contains("**[00:01] Jankees:** Hi."));
        assert!(out.contains("**[00:03] Remote:** You: said hi."));
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn swapping_two_names_through_a_placeholder() {
        let tmp = relabel(MD, "You", "\u{1}");
        let tmp = relabel(&tmp, "Remote", "You");
        let out = relabel(&tmp, "\u{1}", "Remote");
        assert!(out.contains("**[00:01] Remote:** Hi."));
        assert!(out.contains("**[00:03] You:** You: said hi."));
    }

    /// The checked-in invented meeting opens with its names and settings.
    #[test]
    fn fixture_folder_opens() {
        // Tests run with the package dir as CWD, so anchor at the workspace.
        // The fixtures stay at the root: the end-to-end test uses them too.
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/meeting");
        let (folder, manifest) = super::open(&dir).expect("fixture opens");
        assert_eq!(folder, dir);
        assert_eq!(manifest.title, "Demo");
        assert_eq!(manifest.speakers, vec!["Maya".to_owned(), "Tom".to_owned()]);
        assert_eq!(manifest.language, "en");
        let (mic, computer) = crate::export::tracks(&folder);
        assert!(mic.is_file() && computer.is_file());
    }

    /// An upstream manifest in the old `{"you", "remote"}` shape still reads.
    #[test]
    fn legacy_manifest_shape_reads() {
        let (_, manifest) = super::open(
            &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/legacy/Legacy.meeting-recorder"),
        )
        .expect("legacy opens");
        assert_eq!(manifest.title, "Legacy");
        assert_eq!(manifest.speakers, vec!["Maya".to_owned(), "Tom".to_owned()]);
        assert_eq!(manifest.provider, None);
    }

    /// Missing names fall back to the labels upstream wrote into the
    /// transcript, so renaming an old meeting finds them.
    #[test]
    fn missing_speakers_default_to_upstream_labels() {
        let manifest = Manifest::from_json(&json!({"title": "x", "started_at": 0})).expect("reads");
        assert_eq!(
            manifest.speakers,
            vec!["You".to_owned(), "Remote".to_owned()]
        );
        let half = Manifest::from_json(
            &json!({"title": "x", "started_at": 0, "speakers": {"you": "Maya"}}),
        )
        .expect("reads");
        assert_eq!(half.speakers, vec!["Maya".to_owned(), "Remote".to_owned()]);
    }

    #[test]
    fn manifest_round_trips() {
        let manifest = Manifest {
            title: "Weekly".into(),
            started_at: 1_767_225_600,
            duration_secs: 42,
            format: Format::Separate,
            language: "id".into(),
            speakers: vec!["Speaker 1".into(), "Speaker 2".into()],
            imported: Some("call.mp3".into()),
            speaker_count: Some(2),
            model: None,
            provider: Some("elevenlabs".into()),
            chapters: vec![Chapter {
                start_ms: 1000,
                title: "Intro".into(),
            }],
            chapters_by: Some("claude".into()),
        };
        let back = Manifest::from_json(&manifest.to_json()).expect("reads back");
        assert_eq!(back.title, manifest.title);
        assert_eq!(back.duration_secs, 42);
        assert_eq!(back.format.key(), Format::Separate.key());
        assert_eq!(back.language, "id");
        assert_eq!(back.speakers, manifest.speakers);
        assert_eq!(back.imported.as_deref(), Some("call.mp3"));
        assert_eq!(back.speaker_count, Some(2));
        assert_eq!(back.provider.as_deref(), Some("elevenlabs"));
        assert_eq!(back.chapters.len(), 1);
        assert_eq!(back.chapters[0].title, "Intro");
        assert_eq!(back.chapters_by.as_deref(), Some("claude"));
    }

    #[test]
    fn numbered_labels_parse_back() {
        assert_eq!(super::parse_remote_n(&super::remote_n(3)), Some(3));
        assert_eq!(super::parse_speaker_n(&super::speaker_n(12)), Some(12));
        assert_eq!(super::parse_remote_n("Remote"), None);
        assert_eq!(super::parse_speaker_n("Remote 2"), None);
    }
}
