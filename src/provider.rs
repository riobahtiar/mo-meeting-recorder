//! Optional cloud transcription providers: ElevenLabs speech-to-text, Google
//! Cloud Speech-to-Text, and any speech-to-text model on OpenRouter. Local
//! whisper stays the default; a provider only runs on explicit opt-in in
//! Preferences (or `--provider` on the command line), and only the audio
//! leaves the Mac (never the whole meeting folder). API keys live in the
//! macOS Keychain, never in config files; the config only names the provider.
//! The OpenRouter model is `openrouter_model` in config.toml, Whisper-1
//! unless set to another transcription-capable slug.
//!
//! Every provider takes audio files and returns words with times, so they
//! slot into `transcribe.rs` where whisper regions become segments: chunked
//! audio in (ffmpeg, offsets kept), word lists out, mapped to absolute times.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const ELEVEN_URL: &str = "https://api.elevenlabs.io/v1/speech-to-text";
pub const GOOGLE_URL: &str = "https://speech.googleapis.com/v1/speech:recognize";
pub const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/audio/transcriptions";
/// ElevenLabs takes long files; ten minutes keeps one request small.
pub const ELEVEN_CHUNK_MS: u64 = 10 * 60 * 1000;
/// Google's synchronous call caps audio at 60 seconds.
pub const GOOGLE_CHUNK_MS: u64 = 55 * 1000;
/// OpenRouter times out after about a minute upstream.
pub const OPENROUTER_CHUNK_MS: u64 = 55 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Local,
    ElevenLabs,
    Google,
    OpenRouter,
}

static OVERRIDE: std::sync::Mutex<Option<Provider>> = std::sync::Mutex::new(None);

/// The `transcribe-file --provider` flag for one run, without touching config.
pub fn set_override(provider: Provider) {
    *OVERRIDE.lock().unwrap() = Some(provider);
}

/// Parses the config/flag ids: `local`, `elevenlabs`, `google`, `openrouter`.
pub fn from_id(id: &str) -> Option<Provider> {
    match id.trim() {
        "local" => Some(Provider::Local),
        "elevenlabs" => Some(Provider::ElevenLabs),
        "google" => Some(Provider::Google),
        "openrouter" => Some(Provider::OpenRouter),
        _ => None,
    }
}

/// The configured provider: `elevenlabs`, `google` or `openrouter` in
/// config.toml, local whisper otherwise.
pub fn selected() -> Provider {
    if let Some(provider) = OVERRIDE.lock().unwrap().to_owned() {
        return provider;
    }
    crate::models::config_value("provider")
        .as_deref()
        .and_then(from_id)
        .unwrap_or(Provider::Local)
}

pub fn label(provider: Provider) -> &'static str {
    match provider {
        Provider::Local => crate::locales::t("provider.name_local"),
        Provider::ElevenLabs => crate::locales::t("provider.name_eleven"),
        Provider::Google => crate::locales::t("provider.name_google"),
        Provider::OpenRouter => crate::locales::t("provider.name_openrouter"),
    }
}

pub fn chunk_ms(provider: Provider) -> u64 {
    match provider {
        Provider::Local => u64::MAX,
        Provider::ElevenLabs => ELEVEN_CHUNK_MS,
        Provider::Google => GOOGLE_CHUNK_MS,
        Provider::OpenRouter => OPENROUTER_CHUNK_MS,
    }
}

fn service(provider: Provider) -> &'static str {
    match provider {
        Provider::Local => "momr-local",
        Provider::ElevenLabs => "momr-elevenlabs",
        Provider::Google => "momr-google",
        Provider::OpenRouter => "momr-openrouter",
    }
}

/// The API key from the Keychain, or why there is none, for the UI.
pub fn api_key(provider: Provider) -> Result<String, String> {
    api_key_with(provider, run_security)
}

fn api_key_with(
    provider: Provider,
    run: impl Fn(&str, &[&str]) -> Result<String, String>,
) -> Result<String, String> {
    let key = run(
        "security",
        &["find-generic-password", "-s", service(provider), "-w"],
    )
    .map_err(|_| {
        format!(
            "No API key saved for {}. Add one in Settings › Transcription.",
            label(provider)
        )
    })?;
    let key = key.trim().to_owned();
    if key.is_empty() {
        return Err(format!(
            "No API key saved for {}. Add one in Settings › Transcription.",
            label(provider)
        ));
    }
    Ok(key)
}

/// Stores the API key in the Keychain (updated when one exists).
pub fn save_api_key(provider: Provider, key: &str) -> Result<(), String> {
    save_api_key_with(provider, key, run_security)
}

fn save_api_key_with(
    provider: Provider,
    key: &str,
    run: impl Fn(&str, &[&str]) -> Result<String, String>,
) -> Result<(), String> {
    run(
        "security",
        &[
            "add-generic-password",
            "-s",
            service(provider),
            "-a",
            "momr",
            "-w",
            key,
            "-U",
        ],
    )
    .map(|_| ())
    .map_err(|e| format!("Could not save the API key: {e}"))
}

pub fn has_api_key(provider: Provider) -> bool {
    api_key(provider).is_ok()
}

fn run_security(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!("exit {}", output.status.code().unwrap_or(-1)));
    }
    String::from_utf8(output.stdout).map_err(|e| e.to_string())
}

/// One recognised word with absolute times, the common shape of both
/// providers' answers.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker: Option<String>,
}

/// Our language codes to ElevenLabs' `language_code` (ISO-639-1); "auto" is
/// omitted so the service detects it.
pub fn eleven_language(code: &str) -> Option<&str> {
    if code == "auto" { None } else { Some(code) }
}

/// Our language codes to Google's BCP-47 `languageCode`; "auto" falls back to
/// US English, which Google requires to be explicit.
pub fn google_language(code: &str) -> &str {
    match code {
        "en" | "auto" => "en-US",
        "id" => "id-ID",
        "nl" => "nl-NL",
        "de" => "de-DE",
        "fr" => "fr-FR",
        "es" => "es-ES",
        "it" => "it-IT",
        "pt" => "pt-PT",
        _ => "en-US",
    }
}

fn agent() -> ureq::Agent {
    // Read error bodies ourselves, so a rejected key or quota comes back as
    // the provider's message rather than a bare status code.
    ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .http_status_as_error(false)
            .build(),
    )
}

fn check_status(provider: &str, status: u16, body: &str) -> Result<serde_json::Value, String> {
    // The provider's own message when it gives one, else the status.
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v["error"]["message"]
                .as_str()
                .or_else(|| v["detail"]["message"].as_str())
                .or_else(|| v["detail"].as_str())
                .or_else(|| v["message"].as_str())
                .map(str::to_owned)
        })
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| format!("HTTP {status}"));
    match status {
        200..=299 => serde_json::from_str(body)
            .map_err(|e| format!("{provider} returned text that is not JSON: {e}")),
        401 | 403 => Err(format!("{provider} rejected the API key: {detail}")),
        402 => Err(format!("{provider} is out of credits: {detail}")),
        429 => Err(format!("{provider} is out of quota: {detail}")),
        _ => Err(format!("{provider} failed: {detail}")),
    }
}

/// One ElevenLabs chunk: mono WAV in, words and the detected language code out.
pub fn transcribe_eleven(
    key: &str,
    wav: &Path,
    language: &str,
) -> Result<(Vec<Word>, Option<String>), String> {
    let audio = std::fs::read(wav).map_err(|e| format!("{}: {e}", wav.display()))?;
    let mut form = ureq::unversioned::multipart::Form::new()
        .text("model_id", "scribe_v1")
        .text("diarize", "true");
    if let Some(code) = eleven_language(language) {
        form = form.text("language_code", code);
    }
    let part = ureq::unversioned::multipart::Part::bytes(&audio)
        .mime_str("audio/wav")
        .map_err(|e| format!("multipart: {e}"))?
        .file_name("chunk.wav");
    form = form.part("file", part);
    let mut response = agent()
        .post(ELEVEN_URL)
        .header("xi-api-key", key)
        .send(form)
        .map_err(|e| format!("ElevenLabs request failed: {e}"))?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("ElevenLabs reply unreadable: {e}"))?;
    let value = check_status("ElevenLabs", status, &body)?;
    let language = value["language_code"].as_str().map(str::to_owned);
    Ok((parse_eleven_words(&value)?, language))
}

/// `words[]` (`text`, `start`/`end` seconds, `speaker_id` when diarized) to
/// `Word`s. Silence transcribes to no words, which is a fine empty result.
pub fn parse_eleven_words(value: &serde_json::Value) -> Result<Vec<Word>, String> {
    let words = value["words"].as_array().cloned().unwrap_or_default();
    let mut out = Vec::with_capacity(words.len());
    for w in &words {
        let (Some(text), Some(start), Some(end)) =
            (w["text"].as_str(), w["start"].as_f64(), w["end"].as_f64())
        else {
            continue;
        };
        out.push(Word {
            text: text.to_owned(),
            start_ms: (start.max(0.0) * 1000.0).round() as u64,
            end_ms: (end.max(0.0) * 1000.0).round() as u64,
            speaker: w["speaker_id"].as_str().map(str::to_owned),
        });
    }
    Ok(out)
}

/// The request body for one Google chunk: 16 kHz mono LINEAR16, base64.
/// Pure, so tests cover it without the network.
pub fn google_body(audio: &[u8], language: &str) -> serde_json::Value {
    serde_json::json!({
        "config": {
            "encoding": "LINEAR16",
            "sampleRateHertz": 16000,
            "languageCode": google_language(language),
            "enableWordTimeOffsets": true,
            "diarizationConfig": {"enableSpeakerDiarization": true, "minSpeakerCount": 1, "maxSpeakerCount": 6},
        },
        "audio": {"content": base64_encode(audio)},
    })
}

/// A private scratch directory for one provider run.
pub fn workdir() -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!(
        "momr-provider-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir(&dir)?;
    Ok(dir)
}

/// Writes f32 samples as mono WAV at `rate`. The providers take plain WAV,
/// so no ffmpeg is needed on this step.
pub fn write_wav_mono(path: &Path, samples: &[f32], rate: u32) -> std::io::Result<()> {
    let data: Vec<u8> = samples
        .iter()
        .flat_map(|s| {
            let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
            v.to_le_bytes()
        })
        .collect();
    let mut wav = vec![0u8; 44];
    wav[0..4].copy_from_slice(b"RIFF");
    wav[4..8].copy_from_slice(&(36 + data.len() as u32).to_le_bytes());
    wav[8..12].copy_from_slice(b"WAVE");
    wav[12..16].copy_from_slice(b"fmt ");
    wav[16..20].copy_from_slice(&16u32.to_le_bytes());
    wav[20..22].copy_from_slice(&1u16.to_le_bytes());
    wav[22..24].copy_from_slice(&1u16.to_le_bytes());
    wav[24..28].copy_from_slice(&rate.to_le_bytes());
    wav[28..32].copy_from_slice(&(rate * 2).to_le_bytes());
    wav[32..34].copy_from_slice(&2u16.to_le_bytes());
    wav[34..36].copy_from_slice(&16u16.to_le_bytes());
    wav[36..40].copy_from_slice(b"data");
    wav[40..44].copy_from_slice(&(data.len() as u32).to_le_bytes());
    wav.extend_from_slice(&data);
    std::fs::write(path, wav)
}

/// One Google chunk: 16 kHz mono WAV in, words out. Google needs an explicit
pub fn transcribe_google(
    key: &str,
    wav_16k: &Path,
    language: &str,
) -> Result<(Vec<Word>, Option<String>), String> {
    let raw = wav_to_mono16k_raw(wav_16k)?;
    let mut response = agent()
        .post(format!("{GOOGLE_URL}?key={key}"))
        .send_json(google_body(&raw, language))
        .map_err(|e| format!("Google request failed: {e}"))?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Google reply unreadable: {e}"))?;
    Ok((
        parse_google_words(&check_status("Google", status, &body)?)?,
        None,
    ))
}
/// The transcription model on OpenRouter, `openrouter_model` in config.toml.
/// Whisper-1 is the cheap default; any transcription-capable slug works
/// (whisper-large-v3, gpt-4o-transcribe, chirp-3, …).
pub fn openrouter_model() -> String {
    crate::models::config_value("openrouter_model")
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| "openai/whisper-1".to_owned())
}

/// The request body for one OpenRouter chunk: base64 WAV plus word
/// timestamps. Pure, so tests cover it without the network.
pub fn openrouter_body(audio: &[u8], language: &str, model: &str) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": model,
        "input_audio": {"data": base64_encode(audio), "format": "wav"},
        "response_format": "verbose_json",
        "timestamp_granularities": ["word"],
    });
    if language != "auto" {
        body["language"] = language.into();
    }
    body
}

/// One OpenRouter chunk: WAV in, words out. No diarization: every word comes
/// back without a speaker, so multi-voice audio lands on one speaker label.
pub fn transcribe_openrouter(
    key: &str,
    wav: &Path,
    language: &str,
) -> Result<(Vec<Word>, Option<String>), String> {
    let audio = std::fs::read(wav).map_err(|e| format!("{}: {e}", wav.display()))?;
    let mut response = agent()
        .post(OPENROUTER_URL)
        .header("Authorization", format!("Bearer {key}"))
        .send_json(openrouter_body(&audio, language, &openrouter_model()))
        .map_err(|e| format!("OpenRouter request failed: {e}"))?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("OpenRouter reply unreadable: {e}"))?;
    let value = check_status("OpenRouter", status, &body)?;
    let detected = value["language"].as_str().map(str::to_owned);
    Ok((parse_openrouter_words(&value)?, detected))
}

/// verbose_json `segments[].words[]` (`word`, `start`/`end` seconds) to
/// `Word`s; falls back to whole segments, then to plain `text` with no times.
pub fn parse_openrouter_words(value: &serde_json::Value) -> Result<Vec<Word>, String> {
    let mut out = Vec::new();
    for segment in value["segments"].as_array().cloned().unwrap_or_default() {
        let seg_start = segment["start"].as_f64().unwrap_or(0.0).max(0.0);
        let seg_end = segment["end"].as_f64().unwrap_or(seg_start).max(seg_start);
        let words = segment["words"].as_array().cloned().unwrap_or_default();
        if words.is_empty() {
            if let Some(text) = segment["text"].as_str().filter(|t| !t.trim().is_empty()) {
                out.push(Word {
                    text: text.trim().to_owned(),
                    start_ms: (seg_start * 1000.0).round() as u64,
                    end_ms: (seg_end * 1000.0).round() as u64,
                    speaker: None,
                });
            }
            continue;
        }
        for w in &words {
            let (Some(text), Some(start), Some(end)) =
                (w["word"].as_str(), w["start"].as_f64(), w["end"].as_f64())
            else {
                continue;
            };
            out.push(Word {
                text: text.to_owned(),
                start_ms: (start.max(0.0) * 1000.0).round() as u64,
                end_ms: (end.max(0.0) * 1000.0).round() as u64,
                speaker: None,
            });
        }
    }
    if out.is_empty()
        && let Some(text) = value["text"].as_str().filter(|t| !t.trim().is_empty())
    {
        out.push(Word {
            text: text.trim().to_owned(),
            start_ms: 0,
            end_ms: 0,
            speaker: None,
        });
    }
    Ok(out)
}
fn wav_to_mono16k_raw(wav: &Path) -> Result<Vec<u8>, String> {
    let out = wav.with_extension("g16.s16");
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-nostdin", "-i"])
        .arg(wav)
        .args(["-f", "s16le", "-ar", "16000", "-ac", "1"])
        .arg(&out)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("ffmpeg not found: {e}"))?;
    if !status.success() {
        return Err("ffmpeg could not prepare the audio for Google".into());
    }
    let bytes = std::fs::read(&out).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&out);
    Ok(bytes)
}

/// `results[].alternatives[0].words[]` (`word`, `startTime`/`endTime` like
/// `"1.200s"`, `speakerTag`) to `Word`s.
pub fn parse_google_words(value: &serde_json::Value) -> Result<Vec<Word>, String> {
    let mut out = Vec::new();
    for result in value["results"].as_array().cloned().unwrap_or_default() {
        let Some(alternative) = result["alternatives"].as_array().and_then(|a| a.first()) else {
            continue;
        };
        for w in alternative["words"].as_array().cloned().unwrap_or_default() {
            let (Some(text), Some(start), Some(end)) = (
                w["word"].as_str(),
                w["startTime"].as_str().and_then(parse_google_time),
                w["endTime"].as_str().and_then(parse_google_time),
            ) else {
                continue;
            };
            out.push(Word {
                text: text.to_owned(),
                start_ms: start,
                end_ms: end,
                speaker: w["speakerTag"].as_u64().map(|tag| format!("speaker_{tag}")),
            });
        }
    }
    Ok(out)
}

/// `"1.200s"` to milliseconds.
fn parse_google_time(text: &str) -> Option<u64> {
    let secs: f64 = text.strip_suffix('s')?.parse().ok()?;
    if secs < 0.0 {
        return None;
    }
    Some((secs * 1000.0).round() as u64)
}

/// Standard base64, no new crate for one function.
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut n: u32 = 0;
        for (i, &b) in chunk.iter().enumerate() {
            n |= (b as u32) << (16 - 8 * i);
        }
        let pad = 3 - chunk.len();
        for i in 0..4 - pad {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
        for _ in 0..pad {
            out.push('=');
        }
    }
    out
}

/// Split points for chunked uploads: `(start_ms, end_ms)` covering the whole
/// duration. Pure, so the offsets are tested without ffmpeg.
pub fn chunk_ranges(total_ms: u64, chunk_ms: u64) -> Vec<(u64, u64)> {
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < total_ms {
        let end = (start + chunk_ms).min(total_ms);
        ranges.push((start, end));
        start = end;
    }
    ranges
}

/// Cuts mono WAV chunks from `src` into `dir`, one file per range, at `rate`.
/// Returns `(chunk file, absolute offset ms)`.
pub fn write_wav_chunks(
    src: &Path,
    ranges: &[(u64, u64)],
    dir: &Path,
    rate: u32,
) -> Result<Vec<(PathBuf, u64)>, String> {
    let mut chunks = Vec::with_capacity(ranges.len());
    for (i, (start, end)) in ranges.iter().enumerate() {
        let out = dir.join(format!("chunk-{i:03}.wav"));
        let status = Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-nostdin"])
            .arg("-ss")
            .arg(format!("{:.3}", *start as f64 / 1000.0))
            .arg("-t")
            .arg(format!("{:.3}", (*end - *start) as f64 / 1000.0))
            .arg("-i")
            .arg(src)
            .arg("-ar")
            .arg(rate.to_string())
            .arg("-ac")
            .arg("1")
            .arg(&out)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("ffmpeg not found: {e}"))?;
        if !status.success() {
            return Err("ffmpeg could not cut the audio for upload".into());
        }
        chunks.push((out, *start));
    }
    Ok(chunks)
}

/// Groups words into segments: same speaker, split on gaps past `max_gap_ms`
/// or 60 characters, so a cloud reply reads like transcript lines.
pub fn group_words(words: &[Word], max_gap_ms: u64) -> Vec<(String, u64, u64, Option<String>)> {
    let mut groups: Vec<(String, u64, u64, Option<String>)> = Vec::new();
    for word in words {
        let separate = match groups.last() {
            None => true,
            Some((text, _, end, speaker)) => {
                speaker != &word.speaker
                    || word.start_ms.saturating_sub(*end) > max_gap_ms
                    || text.len() > 60
            }
        };
        if separate {
            groups.push((
                word.text.clone(),
                word.start_ms,
                word.end_ms,
                word.speaker.clone(),
            ));
        } else if let Some((text, _, end, _)) = groups.last_mut() {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&word.text);
            *end = word.end_ms;
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn google_time_parses() {
        assert_eq!(parse_google_time("1.200s"), Some(1200));
        assert_eq!(parse_google_time("0s"), Some(0));
        assert_eq!(parse_google_time("90.0005s"), Some(90001));
        assert_eq!(parse_google_time("nope"), None);
        assert_eq!(parse_google_time("-1s"), None);
    }

    #[test]
    fn eleven_parses_words_and_speakers() {
        let value = serde_json::json!({
            "text": "hi there",
            "words": [
                {"text": "hi", "start": 0.1, "end": 0.3, "speaker_id": "speaker_0"},
                {"text": "there", "start": 0.4, "end": 0.7, "speaker_id": "speaker_1"},
            ],
        });
        let words = parse_eleven_words(&value).unwrap();
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].start_ms, 100);
        assert_eq!(words[1].speaker.as_deref(), Some("speaker_1"));
        // Silence is an empty word list, not an error.
        assert_eq!(
            parse_eleven_words(&serde_json::json!({"text": ""})).unwrap(),
            vec![]
        );
    }

    #[test]
    fn google_parses_words_and_tags() {
        let value = serde_json::json!({
            "results": [{"alternatives": [{"transcript": "halo dunia",
                "words": [
                    {"word": "halo", "startTime": "0.100s", "endTime": "0.300s", "speakerTag": 1},
                    {"word": "dunia", "startTime": "0.400s", "endTime": "0.800s", "speakerTag": 2},
                ]}]}],
        });
        let words = parse_google_words(&value).unwrap();
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].speaker.as_deref(), Some("speaker_1"));
        assert_eq!(words[1].end_ms, 800);
    }

    #[test]
    fn status_errors_name_the_cause() {
        assert!(
            check_status("ElevenLabs", 401, r#"{"detail":{"message":"x"}}"#)
                .unwrap_err()
                .contains("rejected the API key")
        );
        assert!(
            check_status("Google", 429, "{}")
                .unwrap_err()
                .contains("quota")
        );
        assert!(check_status("Google", 200, r#"{"results":[]}"#).is_ok());
        assert!(
            check_status("Google", 500, "boom")
                .unwrap_err()
                .contains("HTTP 500")
        );
        assert!(
            check_status(
                "OpenRouter",
                402,
                r#"{"error":{"message":"insufficient credits"}}"#
            )
            .unwrap_err()
            .contains("out of credits")
        );
    }

    #[test]
    fn chunks_cover_the_duration() {
        assert_eq!(chunk_ranges(0, 1000), vec![]);
        assert_eq!(chunk_ranges(500, 1000), vec![(0, 500)]);
        assert_eq!(
            chunk_ranges(2500, 1000),
            vec![(0, 1000), (1000, 2000), (2000, 2500)]
        );
    }

    #[test]
    fn words_group_by_speaker_and_gap() {
        let words = vec![
            Word {
                text: "a".into(),
                start_ms: 0,
                end_ms: 100,
                speaker: Some("s0".into()),
            },
            Word {
                text: "b".into(),
                start_ms: 150,
                end_ms: 250,
                speaker: Some("s0".into()),
            },
            Word {
                text: "c".into(),
                start_ms: 5000,
                end_ms: 5100,
                speaker: Some("s0".into()),
            },
            Word {
                text: "d".into(),
                start_ms: 5150,
                end_ms: 5250,
                speaker: Some("s1".into()),
            },
        ];
        let groups = group_words(&words, 700);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].0, "a b");
        assert_eq!(groups[2].3.as_deref(), Some("s1"));
    }

    #[test]
    fn keychain_wiring_passes_service_and_key() {
        let saved = std::cell::RefCell::new(Vec::new());
        let run = |program: &str, args: &[&str]| {
            saved
                .borrow_mut()
                .push((program.to_owned(), args.join(" ")));
            Ok("key-123\n".to_owned())
        };
        assert_eq!(
            api_key_with(Provider::ElevenLabs, run).as_deref(),
            Ok("key-123")
        );
        let calls = saved.borrow();
        assert!(calls[0].1.contains("momr-elevenlabs"));
        let failing = |_: &str, _: &[&str]| Err::<String, String>("nope".into());
        assert!(api_key_with(Provider::Google, failing).is_err());
    }

    #[test]
    fn wav_header_round_trips() {
        let dir = std::env::temp_dir();
        let path = dir.join("momr-wav-test.wav");
        write_wav_mono(&path, &[0.0, 0.5, -0.5, 1.0, -1.0], 16000).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[24..28], 16000u32.to_le_bytes());
        assert_eq!(bytes.len(), 44 + 5 * 2);
        let samples: Vec<i16> = bytes[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes(*c))
            .collect();
        assert_eq!(samples, vec![0, 16384, -16384, 32767, -32767]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn openrouter_body_carries_audio_and_model() {
        let body = openrouter_body(&[1, 2, 3], "en", "openai/whisper-1");
        assert_eq!(body["model"], "openai/whisper-1");
        assert_eq!(body["input_audio"]["format"], "wav");
        assert_eq!(body["language"], "en");
        assert!(!body["input_audio"]["data"].as_str().unwrap().is_empty());
        let auto = openrouter_body(&[1], "auto", "openai/whisper-1");
        assert!(auto.get("language").is_none());
    }

    #[test]
    fn openrouter_parses_words_then_segments_then_text() {
        let value = serde_json::json!({
            "text": "halo dunia",
            "language": "id",
            "segments": [
                {"text": "halo", "start": 0.1, "end": 0.3,
                 "words": [{"word": "halo", "start": 0.1, "end": 0.3}]},
                {"text": "dunia", "start": 0.4, "end": 0.8, "words": []},
            ],
        });
        let words = parse_openrouter_words(&value).unwrap();
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].text, "halo");
        assert_eq!(words[0].start_ms, 100);
        assert_eq!(words[1].text, "dunia");
        assert_eq!(words[1].end_ms, 800);
        assert!(words.iter().all(|w| w.speaker.is_none()));
        let bare = parse_openrouter_words(&serde_json::json!({"text": "hi"})).unwrap();
        assert_eq!(bare.len(), 1);
        assert!(
            parse_openrouter_words(&serde_json::json!({}))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn ids_round_trip() {
        assert_eq!(from_id("local"), Some(Provider::Local));
        assert_eq!(from_id(" elevenlabs "), Some(Provider::ElevenLabs));
        assert_eq!(from_id("google"), Some(Provider::Google));
        assert_eq!(from_id("openrouter"), Some(Provider::OpenRouter));
        assert_eq!(from_id("whisper"), None);
        assert_eq!(from_id(""), None);
        // The flag wins for one run, without touching config.
        set_override(Provider::OpenRouter);
        assert_eq!(selected(), Provider::OpenRouter);
        *OVERRIDE.lock().unwrap() = None;
    }
}
