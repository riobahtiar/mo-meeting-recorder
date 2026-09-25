//! Transcription after the meeting, in-process with whisper.cpp (whisper-rs),
//! or through a cloud provider when one is chosen (see `provider.rs`).
//!
//! Each side of a recording is transcribed on its own, so the side of a line
//! is its track: the mic is "You", the computer audio "Remote" (numbered when
//! several voices share it). Whisper follows one voice at a time, so mixing
//! the tracks would lose whoever is quieter when two people talk at once. The
//! lines of both sides are then put in the order they were said, and a line
//! of yours that repeats the other side at the same moment is their voice
//! leaking into your mic (no headset), and goes. Speaker labels and the
//! language line stay English: `transcript.md` is read by scripts.

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use chrono::Local;
use whisper_rs::{
    DtwMode, DtwParameters, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
};

use crate::export::{CHANNELS, RATE};
use crate::provider::{Cloud, Provider, Word as CloudWord};
use momr_platform::APP_NAME;

pub const WHISPER_RATE: usize = 16_000;

/// Whisper language codes in dropdown order. Labels come from
/// `language_label()`, so the interface language applies.
pub const LANGUAGE_CODES: [&str; 9] = ["auto", "en", "id", "nl", "de", "fr", "es", "it", "pt"];

/// The localized language name for dropdowns, menus and the transcript header.
pub fn language_label(code: &str) -> &'static str {
    match code {
        "auto" => crate::locales::t("lang.auto"),
        "en" => crate::locales::t("lang.en"),
        "id" => crate::locales::t("lang.id"),
        "nl" => crate::locales::t("lang.nl"),
        "de" => crate::locales::t("lang.de"),
        "fr" => crate::locales::t("lang.fr"),
        "es" => crate::locales::t("lang.es"),
        "it" => crate::locales::t("lang.it"),
        "pt" => crate::locales::t("lang.pt"),
        _ => crate::locales::t("lang.auto"),
    }
}

/// What the transcription reports while it runs.
#[derive(Debug, Clone)]
pub enum Event {
    Stage(String),
    /// Overall progress over both tracks, 0.0 to 1.0.
    Progress(f64),
    /// A freshly transcribed line.
    Segment(String),
    /// Always the last event. whisper-rs leaks the boxed callbacks that hold a
    /// sender, so the channel never closes on its own; wait for this instead.
    Finished,
}

pub type Events = async_channel::Sender<Event>;
/// Set to true to stop a running transcription.
pub type Abort = Arc<AtomicBool>;

pub const CANCELLED: &str = "transcription cancelled";

#[derive(Debug, Clone)]
pub struct Segment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker: String,
    pub text: String,
}

pub struct Transcript {
    pub segments: Vec<Segment>,
    /// The language used or detected, as a whisper code.
    pub language: String,
    pub duration_secs: i64,
}

fn emit(events: &Events, event: Event) {
    let _ = events.send_blocking(event);
}

// ---------------------------------------------------------------------------
// Audio loading

/// Loads a track as 16 kHz mono f32. A `.raw` file is the app's own staging
/// format (s16le, 48 kHz, stereo); anything else is decoded by ffmpeg.
pub fn load_track(path: &Path) -> Result<Vec<f32>, String> {
    if path.extension().is_some_and(|e| e == "raw") {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let frame = 2 * CHANNELS as usize;
        let mono: Vec<f32> = bytes
            .chunks_exact(frame)
            .map(|f| {
                let sum: f32 = f
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|s| f32::from(i16::from_le_bytes(*s)))
                    .sum();
                sum / CHANNELS as f32 / 32768.0
            })
            .collect();
        Ok(downsample(&mono, RATE as usize / WHISPER_RATE))
    } else {
        decode_with_ffmpeg(path)
    }
}

fn decode_with_ffmpeg(path: &Path) -> Result<Vec<f32>, String> {
    let mut child = Command::new("ffmpeg")
        .args(["-nostdin", "-loglevel", "error", "-i"])
        .arg(path)
        .args([
            "-f",
            "f32le",
            "-ac",
            "1",
            "-ar",
            &WHISPER_RATE.to_string(),
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run ffmpeg: {e}"))?;
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .expect("piped stdout")
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "ffmpeg could not decode {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect())
}

/// Decimates by an integer `factor` behind a windowed-sinc low-pass filter.
fn downsample(input: &[f32], factor: usize) -> Vec<f32> {
    if factor <= 1 {
        return input.to_vec();
    }
    const TAPS: usize = 63;
    // Cut off a little below the new Nyquist frequency.
    let cutoff = 0.9 / (2.0 * factor as f64);
    let mid = (TAPS / 2) as f64;
    let mut kernel: Vec<f32> = (0..TAPS)
        .map(|i| {
            let x = i as f64 - mid;
            let sinc = if x == 0.0 {
                2.0 * cutoff
            } else {
                (2.0 * std::f64::consts::PI * cutoff * x).sin() / (std::f64::consts::PI * x)
            };
            let t = 2.0 * std::f64::consts::PI * i as f64 / (TAPS - 1) as f64;
            let blackman = 0.42 - 0.5 * t.cos() + 0.08 * (2.0 * t).cos();
            (sinc * blackman) as f32
        })
        .collect();
    let sum: f32 = kernel.iter().sum();
    kernel.iter_mut().for_each(|k| *k /= sum);

    let half = TAPS / 2;
    (0..input.len() / factor)
        .map(|n| {
            let center = n * factor;
            kernel
                .iter()
                .enumerate()
                .map(|(k, weight)| {
                    let i = center as isize + k as isize - half as isize;
                    if i < 0 || i as usize >= input.len() {
                        0.0
                    } else {
                        input[i as usize] * weight
                    }
                })
                .sum()
        })
        .collect()
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

fn ms_to_sample(ms: i64) -> usize {
    ms.max(0) as usize * WHISPER_RATE / 1000
}

fn sample_to_ms(sample: usize) -> i64 {
    (sample * 1000 / WHISPER_RATE) as i64
}

/// Below about -50 dBFS there is no speech to find, only hallucinations.
fn is_silent(samples: &[f32]) -> bool {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs())) < 0.003
}

/// How loud a track is while something is said: the 95th percentile of its
/// 30 ms frame levels, so pauses do not drag it down.
fn active_level(samples: &[f32]) -> f32 {
    let mut levels: Vec<f32> = samples.chunks(WHISPER_RATE * 30 / 1000).map(rms).collect();
    if levels.is_empty() {
        return 0.0;
    }
    levels.sort_by(f32::total_cmp);
    levels[levels.len() * 95 / 100]
}

/// Leaves everything below 0.8 alone and bends what is above it towards 1.0.
fn soft_clip(x: f32) -> f32 {
    const KNEE: f32 = 0.8;
    if x.abs() <= KNEE {
        x
    } else {
        x.signum() * (KNEE + (1.0 - KNEE) * ((x.abs() - KNEE) / (1.0 - KNEE)).tanh())
    }
}

/// Mixes the two tracks for whisper. Each is brought to a similar speaking
/// level first, so a quiet mic is not drowned by loud computer audio; a track
/// that is only noise is left as it is rather than boosted.
fn mix(mic: &[f32], computer: &[f32]) -> Vec<f32> {
    const TARGET: f32 = 0.1;
    let gain = |track: &[f32]| {
        let level = active_level(track);
        if is_silent(track) || level < 0.003 {
            1.0
        } else {
            (TARGET / level).clamp(0.25, 8.0)
        }
    };
    let (mic_gain, computer_gain) = (gain(mic), gain(computer));
    (0..mic.len().max(computer.len()))
        .map(|i| {
            let m = mic.get(i).copied().unwrap_or(0.0) * mic_gain;
            let c = computer.get(i).copied().unwrap_or(0.0) * computer_gain;
            soft_clip(m + c)
        })
        .collect()
}

/// Who speaks when: the mic side of a recording (always you), the computer
/// side (Remote, or Remote 1, 2, ... when several voices share it), or the
/// voices found in a single imported file.
#[derive(Clone)]
enum Speakers {
    Mic,
    Remote(Vec<crate::diarize::Turn>),
    Turns(Vec<crate::diarize::Turn>),
}

impl Speakers {
    fn speaker(&self, start_ms: i64, end_ms: i64) -> String {
        use crate::meeting;
        match self {
            Speakers::Mic => meeting::DEFAULT_YOU.to_owned(),
            Speakers::Remote(turns) if turns.is_empty() => meeting::DEFAULT_REMOTE.to_owned(),
            Speakers::Remote(turns) => {
                meeting::remote_n(crate::diarize::speaker_at(turns, start_ms, end_ms) + 1)
            }
            Speakers::Turns(turns) => {
                meeting::speaker_n(crate::diarize::speaker_at(turns, start_ms, end_ms) + 1)
            }
        }
    }

    /// Where `speaker` starts talking near `around_ms`, if that can be told
    /// more precisely than whisper's word times.
    fn takeover_ms(&self, speaker: &str, around_ms: i64) -> Option<i64> {
        let (turns, index) = match self {
            Speakers::Mic => return None,
            Speakers::Remote(turns) => (turns, crate::meeting::parse_remote_n(speaker)?),
            Speakers::Turns(turns) => (turns, crate::meeting::parse_speaker_n(speaker)?),
        };
        crate::diarize::turn_start_near(turns, index.checked_sub(1)?, around_ms)
    }

    /// With diarization a whisper segment can hold two voices without a
    /// sentence end between them, so a line is also cut at a pause where the
    /// speaker changes. Never inside a run of words: a sentence stays whole.
    fn cuts_at_pauses(&self) -> bool {
        match self {
            Speakers::Mic => false,
            Speakers::Remote(turns) => !turns.is_empty(),
            Speakers::Turns(_) => true,
        }
    }
}

/// A stretch with sound in it, in samples of the mix.
#[derive(Debug, Clone, Copy)]
struct Region {
    /// Where the region starts, including some padding before the sound.
    start: usize,
    /// Where the sound itself starts.
    onset: usize,
    end: usize,
}

const FRAME: usize = WHISPER_RATE * 30 / 1000;

/// Frames of `track` with sound in them: above four times its own noise floor.
fn active_frames(track: &[f32], frames: usize) -> Vec<bool> {
    let energies: Vec<f32> = track.chunks(FRAME).map(rms).collect();
    let mut active = vec![false; frames];
    if energies.is_empty() {
        return active;
    }
    let mut sorted = energies.clone();
    sorted.sort_by(f32::total_cmp);
    let floor = sorted[sorted.len() / 10];
    let threshold = (floor * 4.0).max(0.002);
    for (i, energy) in energies.iter().enumerate().take(frames) {
        if *energy >= threshold {
            active[i] = true;
        }
    }
    active
}

/// Finds the parts with sound in them, by frame energy against the noise floor.
/// Generous padding keeps word edges intact.
fn speech_regions(tracks: &[&[f32]], len: usize) -> Vec<Region> {
    let frames = len.div_ceil(FRAME);
    // Each track gets its own threshold: steady sound on one side (music, a
    // fan, a noisy line) must not hide the speech on the other side.
    let mut active = vec![false; frames];
    for track in tracks {
        for (a, on) in active.iter_mut().zip(active_frames(track, frames)) {
            *a |= on;
        }
    }
    regions_from(&active, len)
}

/// The parts of the (levelled) mic with your own voice in them. Through
/// speakers the other side leaks into the mic, delayed a little and always
/// quieter than on its own track. A mic frame only counts when it is at least
/// half as loud as the loudest computer audio around it (echo trails behind),
/// and only in runs of a few frames, so the gaps between their words do not
/// let the echo through either.
fn own_speech_regions(mic: &[f32], computer: &[f32]) -> Vec<Region> {
    const AROUND: usize = 3;
    const RUN: usize = 3;
    let frames = mic.len().div_ceil(FRAME);
    let level = |t: &[f32]| -> Vec<f32> {
        (0..frames)
            .map(|i| rms(&t[(i * FRAME).min(t.len())..((i + 1) * FRAME).min(t.len())]))
            .collect()
    };
    let (own, other) = (level(mic), level(computer));
    let mut active = active_frames(mic, frames);
    for (i, a) in active.iter_mut().enumerate() {
        let loudest = other[i.saturating_sub(AROUND)..(i + AROUND + 1).min(frames)]
            .iter()
            .fold(0.0f32, |m, v| m.max(*v));
        if own[i] * 2.0 < loudest {
            *a = false;
        }
    }
    // Drop runs shorter than RUN frames.
    let mut i = 0;
    while i < frames {
        if !active[i] {
            i += 1;
            continue;
        }
        let start = i;
        while i < frames && active[i] {
            i += 1;
        }
        if i - start < RUN {
            active[start..i].iter_mut().for_each(|a| *a = false);
        }
    }
    regions_from(&active, mic.len())
}

fn regions_from(active: &[bool], len: usize) -> Vec<Region> {
    const PAD: usize = WHISPER_RATE * 300 / 1000;
    const MERGE_GAP: usize = WHISPER_RATE * 800 / 1000;
    let mut regions: Vec<Region> = Vec::new();
    for (i, _) in active.iter().enumerate().filter(|(_, a)| **a) {
        let onset = i * FRAME;
        let start = onset.saturating_sub(PAD);
        let end = ((i + 1) * FRAME + PAD).min(len);
        match regions.last_mut() {
            Some(last) if start <= last.end + MERGE_GAP => last.end = last.end.max(end),
            _ => regions.push(Region { start, onset, end }),
        }
    }
    regions
}

/// The regions glued together with a pause in between, and where each one
/// starts in the glued buffer. Whisper invents text in long silences and
/// places timestamps badly after them, so it only gets the parts with sound;
/// the map puts every timestamp back on the real timeline.
struct Glued {
    samples: Vec<f32>,
    /// (offset in `samples`, region)
    map: Vec<(usize, Region)>,
}

impl Glued {
    fn new(mix: &[f32], regions: &[Region]) -> Self {
        // Long enough for whisper to start a new segment at every join.
        const GAP: usize = WHISPER_RATE * 700 / 1000;
        let mut samples = Vec::new();
        let mut map = Vec::new();
        for region in regions {
            if !samples.is_empty() {
                samples.extend(std::iter::repeat_n(0.0, GAP));
            }
            map.push((samples.len(), *region));
            samples.extend_from_slice(&mix[region.start..region.end]);
        }
        Glued { samples, map }
    }

    /// A time in the glued buffer as (ms on the real timeline, region index).
    fn locate(&self, glued_ms: i64) -> (i64, usize) {
        locate(&self.map, glued_ms)
    }
}

fn locate(map: &[(usize, Region)], glued_ms: i64) -> (i64, usize) {
    let at = ms_to_sample(glued_ms);
    let index = map
        .iter()
        .rposition(|(offset, _)| *offset <= at)
        .unwrap_or(0);
    let Some((offset, region)) = map.get(index).copied() else {
        return (glued_ms, 0);
    };
    let sample = region.start + (at - offset.min(at)).min(region.end - region.start);
    (sample_to_ms(sample), index)
}

// ---------------------------------------------------------------------------
// Model

/// Where downloaded models live: `~/Library/Application Support/momr/models`.
pub fn models_dir() -> PathBuf {
    momr_platform::paths::models()
}

/// Downloads `url` to `target` through a `.part` file, reporting progress as
/// "`label` 42%". Refuses a result smaller than `min_bytes`.
pub fn download(
    url: &str,
    target: &Path,
    label: &str,
    min_bytes: u64,
    events: &Events,
    abort: &Abort,
) -> Result<(), String> {
    let dir = target.parent().expect("model path has a parent");
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut part = target.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    emit(events, Event::Stage(label.to_owned()));

    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("could not download {url}: {e}"))?;
    let total = response.body().content_length();
    let mut reader = response.into_body().into_reader();
    let mut file = BufWriter::new(File::create(&part).map_err(|e| e.to_string())?);
    let mut buf = vec![0u8; 1 << 16];
    let (mut done, mut last_pct) = (0u64, u64::MAX);
    loop {
        if abort.load(Ordering::Relaxed) {
            drop(file);
            let _ = std::fs::remove_file(&part);
            return Err(CANCELLED.into());
        }
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("download interrupted: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if let Some(total) = total.filter(|t| *t > 0) {
            let pct = done * 100 / total;
            if pct != last_pct {
                last_pct = pct;
                emit(events, Event::Stage(format!("{label} {pct}%")));
                emit(events, Event::Progress(done as f64 / total as f64));
            }
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    if total.is_some_and(|t| t != done) || done < min_bytes {
        let _ = std::fs::remove_file(&part);
        return Err(format!("the download of {url} was incomplete"));
    }
    std::fs::rename(&part, target).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Transcription

/// Transcribes the meeting. `language` is a whisper code or "auto".
pub fn transcribe(
    mic: &[f32],
    computer: &[f32],
    language: &str,
    provider: Provider,
    events: &Events,
    abort: &Abort,
) -> Result<Transcript, String> {
    let duration_secs = (mic.len().max(computer.len()) / WHISPER_RATE) as i64;
    let empty = |language: &str| Transcript {
        segments: Vec::new(),
        language: if language == "auto" {
            "unknown".into()
        } else {
            language.to_owned()
        },
        duration_secs,
    };
    if is_silent(mic) && is_silent(computer) {
        emit(events, Event::Progress(1.0));
        return Ok(empty(language));
    }
    if let Provider::Cloud(cloud) = provider {
        return transcribe_provider(cloud, mic, computer, language, duration_secs, events, abort);
    }
    // Each side goes through whisper on its own: whisper follows one voice at
    // a time, so two people talking at once, or a song under someone, would
    // otherwise lose the quieter one. The side of a line is then its track.
    let (mic, computer) = (mix(mic, &[]), mix(computer, &[]));
    let mic_regions = own_speech_regions(&mic, &computer);
    let computer_regions = speech_regions(&[&computer], computer.len());
    if mic_regions.is_empty() && computer_regions.is_empty() {
        emit(events, Event::Progress(1.0));
        return Ok(empty(language));
    }
    let remote = remote_voices(&computer, events, abort)?;
    let context = load_whisper(events, abort)?;

    let length = |regions: &[Region]| regions.iter().map(|r| r.end - r.start).sum::<usize>();
    let total = (length(&mic_regions) + length(&computer_regions)).max(1) as f64;
    let mut sides = [
        (&mic, &mic_regions, Speakers::Mic),
        (&computer, &computer_regions, Speakers::Remote(remote)),
    ];
    // The side with the most sound first: with "auto" its language counts for both.
    sides.sort_by_key(|(_, regions, _)| std::cmp::Reverse(length(regions)));
    let mut language = language.to_owned();
    let mut detected = None;
    let mut segments = Vec::new();
    let mut done = 0.0;
    for (track, regions, speakers) in &sides {
        if regions.is_empty() {
            continue;
        }
        let share = length(regions) as f64 / total;
        let (lines, found) = side_pass(
            &context,
            track,
            regions,
            speakers,
            &language,
            (done, done + share),
            false,
            events,
            abort,
        )?;
        if language == "auto"
            && let Some(found) = found
        {
            language = found.clone();
            detected = Some(found);
        }
        segments.extend(lines);
        done += share;
    }
    emit(events, Event::Progress(1.0));
    Ok(Transcript {
        segments: interleave(segments),
        language: resolved_language(&language, detected),
        duration_secs,
    })
}

/// The language a transcript records: the chosen one, else what was
/// detected, else "unknown".
fn resolved_language(language: &str, detected: Option<String>) -> String {
    if language == "auto" {
        detected.unwrap_or_else(|| "unknown".into())
    } else {
        language.to_owned()
    }
}

/// Both sides through a cloud provider instead of whisper. Whole sides go up
/// in chunks (silence transcribes to nothing, so no VAD is needed); every mic
/// word is You, computer voices number Remote 1, 2, … when the provider hears
/// several, and the same echo-drop and paragraphing below merge the lines.
fn transcribe_provider(
    cloud: Cloud,
    mic: &[f32],
    computer: &[f32],
    language: &str,
    duration_secs: i64,
    events: &Events,
    abort: &Abort,
) -> Result<Transcript, String> {
    let (tracks, detected) = cloud_run(cloud, &[mic, computer], language, events, abort)?;
    let mut segments = side_segments(&tracks[0], Side::Mic);
    segments.extend(side_segments(&tracks[1], Side::Computer));
    for segment in &segments {
        emit(events, Event::Segment(segment.text.clone()));
    }
    Ok(Transcript {
        segments: interleave(segments),
        language: resolved_language(language, detected),
        duration_secs,
    })
}

/// One imported file through a cloud provider. Diarized voices become
/// Speaker 1, 2, … in the order they first speak; `speakers == Some(1)` puts
/// every line on Speaker 1, any other count is the provider's to find.
fn transcribe_single_provider(
    cloud: Cloud,
    track: &[f32],
    language: &str,
    speakers: Option<usize>,
    duration_secs: i64,
    events: &Events,
    abort: &Abort,
) -> Result<Transcript, String> {
    let (tracks, detected) = cloud_run(cloud, &[track], language, events, abort)?;
    let segments = single_segments(&tracks[0], speakers);
    for segment in &segments {
        emit(events, Event::Segment(segment.text.clone()));
    }
    Ok(Transcript {
        segments: interleave(segments),
        language: resolved_language(language, detected),
        duration_secs,
    })
}

/// The real network run: the key, a scratch folder, ffmpeg for the cuts and
/// the provider for each chunk. The folder goes whatever happens.
fn cloud_run(
    cloud: Cloud,
    tracks: &[&[f32]],
    language: &str,
    events: &Events,
    abort: &Abort,
) -> Result<CloudWords, String> {
    use crate::provider as P;
    P::check_language(cloud, language)?;
    let key = P::api_key(cloud).map_err(|e| e.to_string())?;
    let dir = P::workdir()
        .map_err(|e| crate::locales::tf("provider.workdir_failed", &[&e.to_string()]))?;
    emit(
        events,
        Event::Stage(crate::locales::tf("provider.stage", &[cloud.name()])),
    );
    let mut cut = |index: usize, track: &[f32], ranges: &[(u64, u64)]| {
        let whole = dir.join(format!("track-{index}.wav"));
        P::write_wav_mono(&whole, track, WHISPER_RATE as u32)
            .map_err(|e| crate::locales::tf("provider.stage_failed", &[&e.to_string()]))?;
        ranges
            .iter()
            .enumerate()
            .map(|(i, range)| {
                let out = dir.join(format!("track-{index}-{i:03}.wav"));
                P::write_wav_chunk(&whole, *range, &out, WHISPER_RATE as u32).map(|()| out)
            })
            .collect::<Result<Vec<_>, String>>()
    };
    let aborted = || abort.load(Ordering::Relaxed);
    let mut send = |chunk: &Path| P::transcribe_chunk(cloud, &key, chunk, language, &aborted);
    let result = cloud_words(tracks, cloud.chunk_ms(), &mut cut, &mut send, events, abort);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Every track's words at absolute times, and the first language a chunk
/// reported.
type CloudWords = (Vec<Vec<CloudWord>>, Option<String>);

/// Cuts track `index` into one chunk file per range.
type Cutter<'a> = dyn FnMut(usize, &[f32], &[(u64, u64)]) -> Result<Vec<PathBuf>, String> + 'a;

/// The chunk loop: cuts each track into `chunk_ms` pieces, sends each piece,
/// and returns every track's words at absolute times with the first language
/// a chunk reported. Cutting and sending are passed in, so tests run the loop
/// without ffmpeg or the network. A reply without word times is spread over
/// its chunk rather than stacked at the chunk's start.
fn cloud_words(
    tracks: &[&[f32]],
    chunk_ms: u64,
    cut: &mut Cutter,
    send: &mut dyn FnMut(&Path) -> Result<crate::provider::Chunk, String>,
    events: &Events,
    abort: &Abort,
) -> Result<CloudWords, String> {
    let ranges: Vec<Vec<(u64, u64)>> = tracks
        .iter()
        .map(|track| {
            let total_ms = track.len() as u64 * 1000 / WHISPER_RATE as u64;
            crate::provider::chunk_ranges(total_ms, chunk_ms)
        })
        .collect();
    let total = ranges.iter().map(Vec::len).sum::<usize>().max(1);
    let mut done = 0;
    let mut detected = None;
    let mut out = Vec::with_capacity(tracks.len());
    for (index, (track, ranges)) in tracks.iter().zip(&ranges).enumerate() {
        let files = cut(index, track, ranges)?;
        let mut words = Vec::new();
        for (file, &(start, end)) in files.iter().zip(ranges) {
            if abort.load(Ordering::Relaxed) {
                return Err(CANCELLED.into());
            }
            let chunk = send(file).map_err(|e| {
                crate::locales::tf(
                    "provider.chunk_failed",
                    &[&(done + 1).to_string(), &total.to_string(), &e],
                )
            })?;
            if detected.is_none() {
                detected = chunk.language;
            }
            for mut word in chunk.words {
                if chunk.timed {
                    word.start_ms += start;
                    word.end_ms += start;
                } else {
                    (word.start_ms, word.end_ms) = (start, end);
                }
                words.push(word);
            }
            done += 1;
            emit(events, Event::Progress(done as f64 / total as f64));
        }
        out.push(words);
    }
    emit(events, Event::Progress(1.0));
    Ok((out, detected))
}

/// Which side of a recording a track is.
#[derive(Clone, Copy, PartialEq)]
enum Side {
    Mic,
    Computer,
}

/// A side's cloud words as transcript lines. The mic side is one speaker;
/// the computer side numbers the voices the provider told apart, in the
/// order they first speak, and stays plain Remote when there is one.
fn side_segments(words: &[CloudWord], side: Side) -> Vec<Segment> {
    let voices = first_heard(words);
    crate::provider::group_words(words, PROVIDER_GAP_MS)
        .into_iter()
        .map(|line| {
            let speaker = match side {
                Side::Mic => crate::meeting::DEFAULT_YOU.to_owned(),
                Side::Computer => match voices.iter().position(|v| *v == line.speaker) {
                    Some(n) if voices.len() > 1 => crate::meeting::remote_n(n + 1),
                    _ => crate::meeting::DEFAULT_REMOTE.to_owned(),
                },
            };
            segment(line, speaker)
        })
        .collect()
}

/// An imported file's cloud words as lines on Speaker 1, 2, ...
fn single_segments(words: &[CloudWord], speakers: Option<usize>) -> Vec<Segment> {
    let voices = first_heard(words);
    crate::provider::group_words(words, PROVIDER_GAP_MS)
        .into_iter()
        .map(|line| {
            let n = match speakers {
                Some(1) => 0,
                _ => voices.iter().position(|v| *v == line.speaker).unwrap_or(0),
            };
            segment(line, crate::meeting::speaker_n(n + 1))
        })
        .collect()
}

/// A pause longer than this starts a new line in a cloud transcript.
const PROVIDER_GAP_MS: u64 = 700;

/// The provider voice ids in the order they first speak.
fn first_heard(words: &[CloudWord]) -> Vec<Option<String>> {
    let mut voices: Vec<Option<String>> = Vec::new();
    for word in words {
        if !voices.contains(&word.speaker) {
            voices.push(word.speaker.clone());
        }
    }
    voices
}

fn segment(line: CloudWord, speaker: String) -> Segment {
    Segment {
        start_ms: line.start_ms as i64,
        end_ms: line.end_ms as i64,
        speaker,
        text: line.text,
    }
}

/// The sentences of both sides in the order they were said, joined into
/// paragraphs per speaker. A sentence of yours that repeats what the other
/// side said at the same moment is their voice leaking into your mic, and goes.
/// Your lines are the ones labelled `DEFAULT_YOU`: labels are fixed English
/// here, and the name you go by is applied after transcription.
fn interleave(mut sentences: Vec<Segment>) -> Vec<Segment> {
    let you = crate::meeting::DEFAULT_YOU;
    sentences.sort_by_key(|s| s.start_ms);
    let words = |text: &str| -> Vec<String> {
        text.split_whitespace()
            .map(|w| {
                w.trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase()
            })
            .filter(|w| !w.is_empty())
            .collect()
    };
    let trigrams = |w: &[String]| -> Vec<String> { w.windows(3).map(|t| t.join(" ")).collect() };
    let is_echo = |mine: &Segment| {
        let own_words = words(&mine.text);
        let near: Vec<Vec<String>> = sentences
            .iter()
            .filter(|s| {
                s.speaker != you
                    && s.start_ms < mine.end_ms + 2000
                    && mine.start_ms < s.end_ms + 2000
            })
            .map(|s| words(&s.text))
            .collect();
        let own = trigrams(&own_words);
        if own.is_empty() {
            // A few words: an echo when they come back word for word.
            return !own_words.is_empty()
                && near.iter().any(|theirs| {
                    theirs
                        .windows(own_words.len())
                        .any(|w| w == own_words.as_slice())
                });
        }
        let theirs: std::collections::HashSet<String> =
            near.iter().flat_map(|w| trigrams(w)).collect();
        own.iter().filter(|t| theirs.contains(*t)).count() * 2 >= own.len()
    };
    let keep: Vec<bool> = sentences
        .iter()
        .map(|s| s.speaker != you || !is_echo(s))
        .collect();
    let mut out: Vec<Segment> = Vec::new();
    for (sentence, keep) in sentences.into_iter().zip(keep) {
        if !keep {
            continue;
        }
        match out.last_mut() {
            Some(last)
                if last.speaker == sentence.speaker
                    && (sentence.start_ms - last.end_ms < PARAGRAPH_PAUSE_MS
                        || !ends_sentence(&last.text))
                    && sentence.end_ms - last.start_ms < PARAGRAPH_MAX_MS =>
            {
                last.text.push(' ');
                last.text.push_str(&sentence.text);
                last.end_ms = last.end_ms.max(sentence.end_ms);
            }
            _ => out.push(sentence),
        }
    }
    out
}

/// Who is who on the computer audio of a recording: the turns when more than
/// one voice is heard there, nothing when it is one person (then they are
/// simply Remote). A missing speaker model is no reason to fail the
/// transcript; the other side then stays one speaker.
fn remote_voices(
    computer: &[f32],
    events: &Events,
    abort: &Abort,
) -> Result<Vec<crate::diarize::Turn>, String> {
    if is_silent(computer) {
        return Ok(Vec::new());
    }
    match crate::diarize::turns(computer, None, events, abort) {
        Ok(turns) if turns.iter().any(|t| t.speaker > 0) => Ok(turns),
        Ok(_) => Ok(Vec::new()),
        Err(e) if e == CANCELLED => Err(e),
        Err(e) => {
            eprintln!(
                "{}: finding the voices on the computer audio: {e}",
                momr_platform::APP_NAME
            );
            Ok(Vec::new())
        }
    }
}

/// Transcribes one imported audio file (16 kHz mono) and tells the voices in
/// it apart. `speakers` fixes how many people speak; `None` lets the
/// clustering decide, `Some(1)` skips finding speakers altogether. The lines
/// are labelled "Speaker 1", "Speaker 2", ... in the order they first speak.
pub fn transcribe_single(
    track: &[f32],
    language: &str,
    speakers: Option<usize>,
    provider: Provider,
    events: &Events,
    abort: &Abort,
) -> Result<Transcript, String> {
    let duration_secs = (track.len() / WHISPER_RATE) as i64;
    let empty = || Transcript {
        segments: Vec::new(),
        language: if language == "auto" {
            "unknown".into()
        } else {
            language.to_owned()
        },
        duration_secs,
    };
    if is_silent(track) {
        emit(events, Event::Progress(1.0));
        return Ok(empty());
    }
    if let Provider::Cloud(cloud) = provider {
        return transcribe_single_provider(
            cloud,
            track,
            language,
            speakers,
            duration_secs,
            events,
            abort,
        );
    }
    let level = mix(track, &[]);
    let regions = speech_regions(&[track], level.len());
    if regions.is_empty() {
        emit(events, Event::Progress(1.0));
        return Ok(empty());
    }
    // Speakers first, so the live lines can already say who is talking.
    let turns = match speakers {
        Some(1) => crate::diarize::single(track),
        _ => crate::diarize::turns(track, speakers, events, abort)?,
    };
    let speakers = Speakers::Turns(turns);
    whisper_pass(
        &level,
        &regions,
        &speakers,
        language,
        duration_secs,
        events,
        abort,
    )
}

/// The shared part: whisper over the stretches with sound, then the lines.
fn whisper_pass(
    mixed: &[f32],
    regions: &[Region],
    speakers: &Speakers,
    language: &str,
    duration_secs: i64,
    events: &Events,
    abort: &Abort,
) -> Result<Transcript, String> {
    let context = load_whisper(events, abort)?;
    let (segments, detected) = side_pass(
        &context,
        mixed,
        regions,
        speakers,
        language,
        (0.0, 1.0),
        true,
        events,
        abort,
    )?;
    emit(events, Event::Progress(1.0));
    Ok(Transcript {
        segments,
        language: if language == "auto" {
            detected.unwrap_or_else(|| "unknown".into())
        } else {
            language.to_owned()
        },
        duration_secs,
    })
}

fn load_whisper(events: &Events, abort: &Abort) -> Result<WhisperContext, String> {
    let model = crate::models::ensure(events, abort)?;
    if abort.load(Ordering::Relaxed) {
        return Err(CANCELLED.into());
    }
    emit(
        events,
        Event::Stage(crate::locales::t("stage.loading_model").into()),
    );
    emit(events, Event::Progress(0.0));
    whisper_rs::install_logging_hooks();
    let mut context_params = WhisperContextParameters::default();
    context_params.use_gpu(cfg!(feature = "metal"));
    // Word times aligned on the attention heads (DTW): the plain token times
    // drift by up to a second, too much to tell where one speaker takes over.
    // A model file of unknown kind gets plain token times.
    if let Some(model_preset) = crate::models::dtw_preset() {
        context_params.dtw_parameters(DtwParameters {
            mode: DtwMode::ModelPreset { model_preset },
            ..Default::default()
        });
    }
    WhisperContext::new_with_params(&model, context_params)
        .map_err(|e| format!("could not load the model {}: {e}", model.display()))
}

/// Whisper over the stretches of `track` with sound in them, then the lines.
/// Progress runs from `progress.0` to `progress.1`.
#[allow(clippy::too_many_arguments)]
fn side_pass(
    context: &WhisperContext,
    track: &[f32],
    regions: &[Region],
    speakers: &Speakers,
    language: &str,
    progress: (f64, f64),
    paragraphs: bool,
    events: &Events,
    abort: &Abort,
) -> Result<(Vec<Segment>, Option<String>), String> {
    let glued = Glued::new(track, regions);
    emit(
        events,
        Event::Stage(crate::locales::t("stage.transcribing").into()),
    );
    let (words, detected) =
        run_whisper(context, &glued, speakers, language, progress, events, abort)?;
    Ok((
        phrases(&words, &glued, speakers, track, paragraphs),
        detected,
    ))
}

/// A word with its times in the glued buffer, and how sure whisper was that
/// its segment held speech at all.
struct Word {
    text: String,
    start_ms: i64,
    end_ms: i64,
    no_speech: f32,
    /// Index of the whisper segment it came from.
    segment: usize,
}

fn run_whisper(
    context: &WhisperContext,
    glued: &Glued,
    speakers: &Speakers,
    language: &str,
    progress: (f64, f64),
    events: &Events,
    abort: &Abort,
) -> Result<(Vec<Word>, Option<String>), String> {
    let mut state = context.create_state().map_err(|e| e.to_string())?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    params.set_n_threads(threads.min(16) as i32);
    params.set_language(Some(language));
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_suppress_blank(true);
    params.set_suppress_nst(true);
    params.set_no_speech_thold(0.6);
    // Word times, so a segment can be split where the speaker changes.
    params.set_token_timestamps(true);
    params.set_split_on_word(true);

    let progress_events = events.clone();
    params.set_progress_callback_safe(move |pct: i32| {
        let pct = f64::from(pct.clamp(0, 100)) / 100.0;
        emit(
            &progress_events,
            Event::Progress(progress.0 + (progress.1 - progress.0) * pct),
        );
    });

    // Live lines for the animation, with the speaker already worked out.
    let segment_events = events.clone();
    let map = glued.map.clone();
    let live_speakers = speakers.clone();
    params.set_segment_callback_safe(move |data: whisper_rs::SegmentCallbackData| {
        let text = data.text.trim();
        if text.is_empty() || is_noise_marker(text) {
            return;
        }
        let (start, _) = locate(&map, data.start_timestamp * 10);
        let (end, _) = locate(&map, data.end_timestamp * 10);
        let speaker = live_speakers.speaker(start, end);
        emit(
            &segment_events,
            Event::Segment(format!("{speaker}: {text}")),
        );
    });

    // Passed as a boxed trait object on purpose: whisper-rs 0.16 casts the user
    // data back to `F`, which only matches what it stored when F is this Box.
    let abort_flag = abort.clone();
    let should_abort: Box<dyn FnMut() -> bool> =
        Box::new(move || abort_flag.load(Ordering::Relaxed));
    params.set_abort_callback_safe::<_, Box<dyn FnMut() -> bool>>(Some(should_abort));

    let result = state.full(params, &glued.samples);
    if abort.load(Ordering::Relaxed) {
        return Err(CANCELLED.into());
    }
    result.map_err(|e| e.to_string())?;

    let detected = whisper_rs::get_lang_str(state.full_lang_id_from_state()).map(str::to_owned);
    let eot = context.token_eot();
    let mut words: Vec<Word> = Vec::new();
    for (index, segment) in state.as_iter().enumerate() {
        let no_speech = segment.no_speech_probability();
        // Bytes first: a character can be split over two tokens.
        let mut current: Option<(Vec<u8>, i64, i64)> = None;
        let flush = |current: &mut Option<(Vec<u8>, i64, i64)>, words: &mut Vec<Word>| {
            if let Some((bytes, start_ms, end_ms)) = current.take() {
                let text = String::from_utf8_lossy(&bytes).trim().to_owned();
                if !text.is_empty() {
                    words.push(Word {
                        text,
                        start_ms,
                        end_ms,
                        no_speech,
                        segment: index,
                    });
                }
            }
        };
        for i in 0..segment.n_tokens() {
            let Some(token) = segment.get_token(i) else {
                continue;
            };
            if token.token_id() >= eot {
                continue; // timestamps and other special tokens
            }
            let Ok(bytes) = token.to_bytes() else {
                continue;
            };
            let data = token.token_data();
            // DTW gives one moment per token; fall back to the plain times.
            let (t0, t1) = if data.t_dtw >= 0 {
                (data.t_dtw * 10, data.t_dtw * 10)
            } else {
                (data.t0 * 10, data.t1 * 10)
            };
            match current.as_mut() {
                Some((word, _, end)) if !bytes.starts_with(b" ") => {
                    word.extend_from_slice(bytes);
                    *end = t1.max(*end);
                }
                _ => {
                    flush(&mut current, &mut words);
                    current = Some((bytes.to_vec(), t0, t1));
                }
            }
        }
        flush(&mut current, &mut words);
    }
    Ok((words, detected))
}

/// A pause longer than this starts a new paragraph, even for the same speaker.
const PARAGRAPH_PAUSE_MS: i64 = 3000;
/// Very long turns are still split, so a line stays a useful place to jump to.
const PARAGRAPH_MAX_MS: i64 = 90_000;

/// Groups the words into the lines of the transcript: a new line where the
/// speaker changes, where a sentence ends on another speaker, and at every
/// stretch of silence. Timestamps are put back on the real timeline.
fn phrases(
    words: &[Word],
    glued: &Glued,
    speakers: &Speakers,
    mixed: &[f32],
    paragraphs: bool,
) -> Vec<Segment> {
    struct Phrase {
        words: Vec<String>,
        start_ms: i64,
        end_ms: i64,
        region: usize,
        no_speech: f32,
        segment: usize,
    }

    // A stock phrase is only suspicious when whisper heard nothing else around it.
    let mut per_segment = std::collections::HashMap::<usize, usize>::new();
    for word in words {
        *per_segment.entry(word.segment).or_default() += 1;
    }

    // First cut at sentence ends and region changes, so each piece has one voice.
    let mut pieces: Vec<Phrase> = Vec::new();
    for word in words {
        let (start, region) = glued.locate(word.start_ms);
        let (end, _) = glued.locate(word.end_ms.max(word.start_ms));
        let sentence_ended = pieces
            .last()
            .is_some_and(|p| p.words.last().is_some_and(|w| ends_sentence(w)));
        let turn_at_pause = speakers.cuts_at_pauses()
            && pieces.last().is_some_and(|p| {
                start - p.end_ms >= 250
                    && speakers.speaker(p.start_ms, p.end_ms)
                        != speakers.speaker(start, end.max(start))
            });
        match pieces.last_mut() {
            Some(p)
                if p.region == region
                    && p.segment == word.segment
                    && !sentence_ended
                    && !turn_at_pause =>
            {
                p.words.push(word.text.clone());
                p.end_ms = end.max(p.end_ms);
                p.no_speech = p.no_speech.max(word.no_speech);
            }
            _ => pieces.push(Phrase {
                words: vec![word.text.clone()],
                start_ms: start,
                end_ms: end.max(start),
                region,
                no_speech: word.no_speech,
                segment: word.segment,
            }),
        }
    }

    // The first words of a stretch of sound begin where the sound does; whisper
    // tends to put them at the start of the padding instead.
    let mut previous_region = usize::MAX;
    for piece in &mut pieces {
        if piece.region != previous_region
            && let Some((_, region)) = glued.map.get(piece.region)
        {
            let onset = sample_to_ms(region.onset);
            if (piece.start_ms - onset).abs() < 1500 {
                let shift = onset - piece.start_ms;
                piece.start_ms = onset;
                piece.end_ms += shift.max(0);
            }
        }
        previous_region = piece.region;
        // Token times can collapse to nothing; about 250 ms a word is a floor.
        let floor = piece.start_ms + 250 * piece.words.len() as i64;
        piece.end_ms = piece.end_ms.max(floor);
    }

    // A sentence is never split between speakers: a piece that stops without
    // a sentence end is joined to the next one when that follows within a
    // short pause, and the speaker is then chosen for the sentence as a whole.
    let mut sentences: Vec<Phrase> = Vec::new();
    for piece in pieces {
        match sentences.last_mut() {
            Some(previous)
                if !ends_sentence(previous.words.last().map_or("", String::as_str))
                    && piece.start_ms - previous.end_ms < 3000 =>
            {
                previous.words.extend(piece.words);
                previous.end_ms = piece.end_ms.max(previous.end_ms);
                previous.no_speech = previous.no_speech.max(piece.no_speech);
            }
            _ => sentences.push(piece),
        }
    }
    let pieces = sentences;

    // Who said it, then glue neighbours by the same speaker back together.
    let mut segments: Vec<Segment> = Vec::new();
    for piece in pieces {
        let text = piece.words.join(" ");
        let whole = per_segment.get(&piece.segment) == Some(&piece.words.len());
        if is_noise_marker(&text) || is_hallucination(&text, &piece, whole, mixed) {
            continue;
        }
        // The whole piece goes to the speaker it overlaps most.
        let speaker = speakers.speaker(piece.start_ms, piece.end_ms);
        // A new speaker starts where their voice takes over, not where whisper
        // guessed; keep the order of lines intact.
        let mut piece = piece;
        let changed = segments.last().is_none_or(|last| last.speaker != speaker);
        if changed && let Some(start) = speakers.takeover_ms(&speaker, piece.start_ms) {
            let floor = segments.last().map_or(0, |last| last.start_ms + 1);
            piece.start_ms = start.max(floor);
            piece.end_ms = piece.end_ms.max(piece.start_ms + 1);
        }
        // One paragraph per turn: short pauses ("So... Okay. Then...") stay
        // together, a long pause or a very long turn starts a new paragraph.
        match segments.last_mut() {
            Some(last)
                if paragraphs
                    && last.speaker == speaker
                    && (piece.start_ms - last.end_ms < PARAGRAPH_PAUSE_MS
                        || !ends_sentence(&last.text))
                    && piece.end_ms - last.start_ms < PARAGRAPH_MAX_MS =>
            {
                last.text.push(' ');
                last.text.push_str(&text);
                last.end_ms = piece.end_ms;
            }
            _ => segments.push(Segment {
                start_ms: piece.start_ms,
                end_ms: piece.end_ms,
                speaker,
                text,
            }),
        }
    }

    fn is_hallucination(text: &str, piece: &Phrase, whole: bool, mixed: &[f32]) -> bool {
        let span = &mixed[ms_to_sample(piece.start_ms).min(mixed.len())
            ..ms_to_sample(piece.end_ms).min(mixed.len())];
        let level = rms(span);
        if level < 0.004 || piece.no_speech > 0.85 {
            return true; // whisper talking over silence
        }
        // The things whisper says when it hears nothing, from its subtitle diet.
        whole && is_stock_phrase(text) && (piece.no_speech > 0.3 || level < 0.02)
    }

    segments
}

/// Whether `text` ends a sentence: a full stop, question or exclamation mark,
/// possibly followed by a closing quote or bracket.
fn ends_sentence(text: &str) -> bool {
    text.trim_end()
        .trim_end_matches(['"', '\'', ')', '\u{201d}', '\u{2019}'])
        .ends_with(['.', '?', '!', '\u{2026}'])
}

/// "Thank you.", "Bye.", "Thanks for watching!" and friends: what whisper
/// produces for silence or noise, learned from subtitles.
fn is_stock_phrase(text: &str) -> bool {
    const STOCK: &[&str] = &[
        "thank you",
        "thank you very much",
        "thanks",
        "thanks for watching",
        "thank you for watching",
        "bye",
        "bye bye",
        "you",
        "okay",
        "so",
        "subtitles by the amaraorg community",
        "subtitles by",
        "please subscribe",
        "dank je",
        "dank je wel",
        "bedankt",
        "bedankt voor het kijken",
        "ondertiteling",
        "ondertiteld door",
        "tot de volgende keer",
        "vielen dank",
        "untertitel im auftrag des zdf",
        "merci",
    ];
    let normalized: String = text
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    STOCK.contains(&normalized.as_str()) || normalized.starts_with("subtitles by")
}

/// "[BLANK_AUDIO]", "(music)", "*applause*" and friends.
fn is_noise_marker(text: &str) -> bool {
    let t = text.trim();
    let wrapped = |open: char, close: char| t.starts_with(open) && t.ends_with(close);
    wrapped('[', ']')
        || wrapped('(', ')')
        || wrapped('*', '*')
        || t.chars().all(|c| !c.is_alphanumeric())
}

// ---------------------------------------------------------------------------
// Output

fn clock(ms: i64) -> String {
    let secs = ms / 1000;
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// The language line of `transcript.md`, always in English: the transcript
/// is content that scripts read, not interface.
fn language_name(code: &str) -> String {
    if LANGUAGE_CODES.contains(&code) {
        return crate::locales::t_in(crate::locales::Lang::English, &format!("lang.{code}"))
            .to_owned();
    }
    whisper_rs::get_lang_id(code)
        .and_then(whisper_rs::get_lang_str_full)
        .map(|full| {
            let mut chars = full.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .unwrap_or_else(|| code.to_owned())
}
pub fn to_markdown(title: &str, date: &str, transcript: &Transcript) -> String {
    let mut out = format!("# {title}\n\n");
    out += &format!("- **Date:** {date}\n");
    out += &format!(
        "- **Duration:** {}\n",
        clock(transcript.duration_secs * 1000)
    );
    out += &format!(
        "- **Language:** {}\n\n",
        language_name(&transcript.language)
    );
    out += "## Transcript\n\n";
    if transcript.segments.is_empty() {
        out += "_No speech was recognized._\n";
    }
    for segment in &transcript.segments {
        out += &format!(
            "**[{}] {}:** {}\n\n",
            clock(segment.start_ms),
            segment.speaker,
            segment.text
        );
    }
    out
}

// ---------------------------------------------------------------------------
// CLI

/// What the transcription commands were asked for.
#[derive(Debug, PartialEq)]
struct FileArgs {
    files: Vec<PathBuf>,
    language: String,
    model: Option<String>,
    speakers: Option<usize>,
    /// `--provider` for this run; None follows config.toml.
    provider: Option<Provider>,
}

/// Parses the flags of `transcribe` and `transcribe-file`; `speakers` says
/// whether `--speakers` is allowed. Err means print the usage. Pure, so the
/// flags are tested without running anything.
fn parse_file_args(args: &[String], speakers_allowed: bool) -> Result<FileArgs, ()> {
    let mut parsed = FileArgs {
        files: Vec::new(),
        language: "auto".to_owned(),
        model: None,
        speakers: None,
        provider: None,
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--language" | "-l" => parsed.language = iter.next().ok_or(())?.clone(),
            "--model" | "-m" => parsed.model = Some(iter.next().ok_or(())?.clone()),
            "--speakers" | "-s" if speakers_allowed => {
                match iter.next().and_then(|n| n.parse::<usize>().ok()) {
                    Some(n) if n > 0 => parsed.speakers = Some(n),
                    _ => return Err(()),
                }
            }
            "--provider" | "-p" => {
                parsed.provider = Some(iter.next().and_then(|id| Provider::from_id(id)).ok_or(())?)
            }
            flag if flag.starts_with('-') && flag.len() > 1 => return Err(()),
            _ => parsed.files.push(PathBuf::from(arg)),
        }
    }
    Ok(parsed)
}

/// The provider for a command-line run: the flag, else config.toml.
fn cli_provider(parsed: &FileArgs) -> Result<Provider, String> {
    match parsed.provider {
        Some(provider) => Ok(provider),
        None => crate::provider::configured(),
    }
}

/// `momr transcribe <mic> <computer> [--language xx] [--model name] [--provider id]`
pub fn cli(args: &[String]) -> i32 {
    let Ok(parsed) = parse_file_args(args, false) else {
        return usage();
    };
    let [mic_path, computer_path] = parsed.files.as_slice() else {
        return usage();
    };
    if let Some(name) = &parsed.model {
        crate::models::set_override(name);
    }
    run_cli(|events, abort| {
        let provider = cli_provider(&parsed)?;
        let mic = load_track(mic_path)?;
        let computer = load_track(computer_path)?;
        transcribe(&mic, &computer, &parsed.language, provider, events, abort)
    })
}

/// `momr transcribe-file <audio> [--speakers N] [--language xx] [--model name] [--provider id]`
pub fn cli_file(args: &[String]) -> i32 {
    let Ok(parsed) = parse_file_args(args, true) else {
        return usage();
    };
    let [path] = parsed.files.as_slice() else {
        return usage();
    };
    if let Some(name) = &parsed.model {
        crate::models::set_override(name);
    }
    run_cli(|events, abort| {
        let provider = cli_provider(&parsed)?;
        let track = load_track(path)?;
        transcribe_single(
            &track,
            &parsed.language,
            parsed.speakers,
            provider,
            events,
            abort,
        )
    })
}

/// Runs a transcription for the command line: progress and live lines on
/// stderr, the Markdown on stdout.
fn run_cli(work: impl FnOnce(&Events, &Abort) -> Result<Transcript, String>) -> i32 {
    let (tx, rx) = async_channel::unbounded();
    let started = Instant::now();
    let reporter = std::thread::spawn(move || {
        let mut last_stage = String::new();
        while let Ok(event) = rx.recv_blocking() {
            match event {
                Event::Stage(stage) if stage != last_stage => {
                    // Downloads report every percent; show every tenth.
                    let percent = stage
                        .rsplit_once(' ')
                        .and_then(|(_, p)| p.strip_suffix('%'))
                        .and_then(|p| p.parse::<u32>().ok());
                    if percent.is_none_or(|p| p % 10 == 0) {
                        eprintln!("[{:6.1}s] {stage}", started.elapsed().as_secs_f64());
                    }
                    last_stage = stage;
                }
                Event::Segment(text) => eprintln!("  {text}"),
                Event::Finished => break,
                _ => {}
            }
        }
    });

    let abort = Abort::default();
    let result = work(&tx, &abort);
    emit(&tx, Event::Finished);
    let _ = reporter.join();

    match result {
        Ok(transcript) => {
            let date = Local::now().format("%Y-%m-%d %H:%M").to_string();
            print!("{}", to_markdown("Transcript", &date, &transcript));
            let secs = format!("{:.1}", started.elapsed().as_secs_f64());
            eprintln!("{}", crate::locales::tf("cli.done_in", &[&secs]));
            0
        }
        Err(message) => {
            eprintln!("{APP_NAME}: {message}");
            1
        }
    }
}

fn usage() -> i32 {
    eprintln!(
        "Usage: {APP_NAME} transcribe <mic> <computer> [--language auto|en|nl|...] [--model name] [--provider local|elevenlabs|google|openrouter]"
    );
    eprintln!(
        "       {APP_NAME} transcribe-file <audio> [--speakers N] [--language auto|en|nl|...] [--model name] [--provider local|elevenlabs|google|openrouter]"
    );
    2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(start_ms: i64, speaker: &str, text: &str) -> Segment {
        Segment {
            start_ms,
            end_ms: start_ms + 2000,
            speaker: speaker.into(),
            text: text.into(),
        }
    }

    #[test]
    fn both_sides_come_back_in_the_order_they_spoke() {
        let out = interleave(vec![
            line(0, "Remote", "Thanks, I can start with the release."),
            line(5000, "Remote", "So the beta went out on Monday."),
            line(2500, "You", "Sure, go ahead."),
        ]);
        let order: Vec<&str> = out.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(
            order,
            [
                "Thanks, I can start with the release.",
                "Sure, go ahead.",
                "So the beta went out on Monday."
            ]
        );
    }

    #[test]
    fn the_other_side_leaking_into_the_mic_is_dropped() {
        let out = interleave(vec![
            line(
                0,
                "Remote 1",
                "The review is still pending after four days.",
            ),
            line(300, "You", "review is still pending after four"),
            line(9000, "Remote 1", "Sounds good."),
            line(9100, "You", "Sounds good."),
            // The same words much later are yours.
            line(30_000, "You", "The review is still pending, I see."),
        ]);
        let yours: Vec<&str> = out
            .iter()
            .filter(|s| s.speaker == "You")
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(yours, ["The review is still pending, I see."]);
    }
    fn cloud_word(text: &str, start_ms: u64, speaker: Option<&str>) -> CloudWord {
        CloudWord {
            text: text.into(),
            start_ms,
            end_ms: start_ms + 400,
            speaker: speaker.map(str::to_owned),
        }
    }

    /// Runs the chunk loop over silent tracks of `secs` seconds each, with a
    /// fake cutter and a fake provider that answers `answer(chunk index)`.
    fn run_loop(
        secs: &[usize],
        chunk_ms: u64,
        abort: &Abort,
        mut answer: impl FnMut(usize) -> Result<crate::provider::Chunk, String>,
    ) -> (Result<CloudWords, String>, Vec<f64>) {
        let tracks: Vec<Vec<f32>> = secs.iter().map(|s| vec![0.0; s * WHISPER_RATE]).collect();
        let refs: Vec<&[f32]> = tracks.iter().map(Vec::as_slice).collect();
        let mut cut = |index: usize, _: &[f32], ranges: &[(u64, u64)]| {
            Ok((0..ranges.len())
                .map(|i| PathBuf::from(format!("{index}-{i}")))
                .collect())
        };
        let mut sent = 0;
        let mut send = |_: &Path| {
            sent += 1;
            answer(sent - 1)
        };
        let (tx, rx) = async_channel::unbounded();
        let result = cloud_words(&refs, chunk_ms, &mut cut, &mut send, &tx, abort);
        let progress = std::iter::from_fn(|| rx.try_recv().ok())
            .filter_map(|e| match e {
                Event::Progress(p) => Some(p),
                _ => None,
            })
            .collect();
        (result, progress)
    }

    #[test]
    fn chunk_words_get_absolute_times_and_first_language() {
        let (result, progress) = run_loop(&[30], 10_000, &Abort::default(), |i| {
            Ok(crate::provider::Chunk {
                words: vec![cloud_word("w", 1000, None)],
                language: (i > 0).then(|| format!("lang{i}")),
                timed: true,
            })
        });
        let (tracks, language) = result.unwrap();
        let starts: Vec<u64> = tracks[0].iter().map(|w| w.start_ms).collect();
        assert_eq!(starts, [1000, 11_000, 21_000]);
        assert_eq!(language.as_deref(), Some("lang1"));
        assert!(progress.windows(2).all(|p| p[0] <= p[1]));
        assert_eq!(progress.last(), Some(&1.0));
    }

    #[test]
    fn untimed_chunks_span_their_range() {
        let (result, _) = run_loop(&[25], 10_000, &Abort::default(), |_| {
            Ok(crate::provider::Chunk {
                words: vec![cloud_word("all of it", 0, None)],
                language: None,
                timed: false,
            })
        });
        let words = &result.unwrap().0[0];
        let spans: Vec<(u64, u64)> = words.iter().map(|w| (w.start_ms, w.end_ms)).collect();
        assert_eq!(spans, [(0, 10_000), (10_000, 20_000), (20_000, 25_000)]);
    }

    #[test]
    fn a_failed_chunk_names_where_it_stopped() {
        let (result, _) = run_loop(&[10, 10], 5000, &Abort::default(), |i| {
            if i == 2 {
                Err("boom".into())
            } else {
                Ok(crate::provider::Chunk {
                    words: Vec::new(),
                    language: None,
                    timed: true,
                })
            }
        });
        let error = result.unwrap_err();
        assert!(
            error.contains("3") && error.contains("4") && error.contains("boom"),
            "{error}"
        );
    }

    #[test]
    fn cancel_stops_between_chunks() {
        let abort = Abort::default();
        abort.store(true, Ordering::Relaxed);
        let (result, _) = run_loop(&[10], 5000, &abort, |_| {
            panic!("nothing is sent after Cancel")
        });
        assert_eq!(result.unwrap_err(), CANCELLED);
    }

    #[test]
    fn cloud_sides_are_labelled() {
        let mic = [
            cloud_word("hi", 0, Some("speaker_0")),
            cloud_word("there", 5000, Some("speaker_1")),
        ];
        assert!(
            side_segments(&mic, Side::Mic)
                .iter()
                .all(|s| s.speaker == "You")
        );
        let one = [cloud_word("a", 0, Some("speaker_3"))];
        // One diarized voice is plain Remote.
        let labels: Vec<String> = side_segments(&one, Side::Computer)
            .into_iter()
            .map(|s| s.speaker)
            .collect();
        assert_eq!(labels, ["Remote"]);
        let two = [
            cloud_word("a", 0, Some("speaker_7")),
            cloud_word("b", 5000, Some("speaker_2")),
            cloud_word("c", 10_000, Some("speaker_7")),
        ];
        let labels: Vec<String> = side_segments(&two, Side::Computer)
            .into_iter()
            .map(|s| s.speaker)
            .collect();
        assert_eq!(labels, ["Remote 1", "Remote 2", "Remote 1"]);
        let labels: Vec<String> = single_segments(&two, None)
            .into_iter()
            .map(|s| s.speaker)
            .collect();
        assert_eq!(labels, ["Speaker 1", "Speaker 2", "Speaker 1"]);
        // `--speakers 1` holds for a provider too.
        assert!(
            single_segments(&two, Some(1))
                .iter()
                .all(|s| s.speaker == "Speaker 1")
        );
    }

    #[test]
    fn file_flags_parse() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let parsed = parse_file_args(
            &args(&[
                "a.mp3",
                "-p",
                "openrouter",
                "--speakers",
                "2",
                "-l",
                "id",
                "-m",
                "tiny",
            ]),
            true,
        )
        .unwrap();
        assert_eq!(parsed.files, [PathBuf::from("a.mp3")]);
        assert_eq!(parsed.provider, Some(Provider::Cloud(Cloud::OpenRouter)));
        assert_eq!(parsed.speakers, Some(2));
        assert_eq!(parsed.language, "id");
        assert_eq!(parsed.model.as_deref(), Some("tiny"));
        assert_eq!(
            parse_file_args(&args(&["a.mp3"]), true).unwrap().provider,
            None
        );
        for bad in [
            &["a.mp3", "--provider"][..],
            &["a.mp3", "--provider", "bogus"],
            &["a.mp3", "--speakers", "0"],
            &["a.mp3", "--speakers", "x"],
            &["a.mp3", "--bogus"],
        ] {
            assert!(parse_file_args(&args(bad), true).is_err(), "{bad:?}");
        }
        // Two-track transcription has no --speakers.
        assert!(parse_file_args(&args(&["m.ogg", "c.ogg", "--speakers", "2"]), false).is_err());
    }

    #[test]
    fn transcript_header_stays_english() {
        assert_eq!(language_name("id"), "Indonesian");
        assert_eq!(language_name("nl"), "Dutch");
    }
}
