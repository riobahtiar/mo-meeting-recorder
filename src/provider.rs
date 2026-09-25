//! Optional cloud transcription providers: ElevenLabs speech-to-text, Google
//! Cloud Speech-to-Text, and any speech-to-text model on OpenRouter. Local
//! whisper stays the default; a provider only runs on explicit opt-in in
//! Settings (or `--provider` on the command line), and only the audio leaves
//! the Mac (never the whole meeting folder). API keys live in the macOS
//! Keychain, never in config files; the config only names the provider. Keys
//! reach `security` on stdin, never in its arguments, where any local process
//! could read them from the process list. The OpenRouter model is
//! `openrouter_model` in config.toml, Whisper-1 unless set to another
//! transcription-capable slug.
//!
//! Every provider takes audio files and returns words with times, so they
//! slot into `transcribe.rs` where whisper regions become segments: chunked
//! audio in (ffmpeg, offsets kept), word lists out, mapped to absolute times.
//! Speaker ids only mean something within one request, so "speaker 2" of one
//! chunk need not be the same voice in the next.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::locales::{t, tf};

pub const ELEVEN_URL: &str = "https://api.elevenlabs.io/v1/speech-to-text";
pub const GOOGLE_URL: &str = "https://speech.googleapis.com/v1/speech:recognize";
pub const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/audio/transcriptions";
/// ElevenLabs takes long files; ten minutes keeps one request small.
pub const ELEVEN_CHUNK_MS: u64 = 10 * 60 * 1000;
/// Google's synchronous `recognize` caps audio at 60 seconds.
pub const GOOGLE_CHUNK_MS: u64 = 55 * 1000;
/// Short chunks keep one OpenRouter request well inside the request limits of
/// the models it routes to, whichever the user picks.
pub const OPENROUTER_CHUNK_MS: u64 = 55 * 1000;
/// How often one chunk is tried before the run fails: a dropped connection or
/// a 5xx mid-meeting should not throw away the chunks already paid for.
const ATTEMPTS: u32 = 3;
/// Security's exit status for "the item could not be found".
const ERR_SEC_ITEM_NOT_FOUND: i32 = 44;

/// Which engine transcribes: whisper on this Mac, or a cloud service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Local,
    Cloud(Cloud),
}

/// The cloud services. Only these have keys, chunk sizes and requests, so the
/// functions that need one take a `Cloud`, and local whisper cannot reach them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cloud {
    ElevenLabs,
    Google,
    OpenRouter,
}

impl Provider {
    /// Every provider in Settings order.
    pub const ALL: [Provider; 4] = [
        Provider::Local,
        Provider::Cloud(Cloud::ElevenLabs),
        Provider::Cloud(Cloud::Google),
        Provider::Cloud(Cloud::OpenRouter),
    ];

    /// The id in config.toml, on the command line and in the manifest.
    pub fn id(self) -> &'static str {
        match self {
            Provider::Local => "local",
            Provider::Cloud(Cloud::ElevenLabs) => "elevenlabs",
            Provider::Cloud(Cloud::Google) => "google",
            Provider::Cloud(Cloud::OpenRouter) => "openrouter",
        }
    }

    pub fn from_id(id: &str) -> Option<Provider> {
        let id = id.trim();
        Provider::ALL.into_iter().find(|p| p.id() == id)
    }

    /// The name in Settings, in the interface language.
    pub fn label(self) -> &'static str {
        match self {
            Provider::Local => t("provider.name_local"),
            Provider::Cloud(Cloud::ElevenLabs) => t("provider.name_eleven"),
            Provider::Cloud(Cloud::Google) => t("provider.name_google"),
            Provider::Cloud(Cloud::OpenRouter) => t("provider.name_openrouter"),
        }
    }
}

impl Cloud {
    /// The brand name in messages.
    pub fn name(self) -> &'static str {
        match self {
            Cloud::ElevenLabs => "ElevenLabs",
            Cloud::Google => "Google",
            Cloud::OpenRouter => "OpenRouter",
        }
    }

    pub fn chunk_ms(self) -> u64 {
        match self {
            Cloud::ElevenLabs => ELEVEN_CHUNK_MS,
            Cloud::Google => GOOGLE_CHUNK_MS,
            Cloud::OpenRouter => OPENROUTER_CHUNK_MS,
        }
    }

    fn service(self) -> &'static str {
        match self {
            Cloud::ElevenLabs => "momr-elevenlabs",
            Cloud::Google => "momr-google",
            Cloud::OpenRouter => "momr-openrouter",
        }
    }
}

/// The provider named in config.toml, local whisper when none is. An id that
/// names no provider is an error, not a quiet fall back: the user believes
/// audio goes one way while it goes the other.
pub fn configured() -> Result<Provider, String> {
    match crate::models::config_value("provider") {
        None => Ok(Provider::Local),
        Some(id) => Provider::from_id(&id).ok_or_else(|| tf("provider.unknown_id", &[&id])),
    }
}

pub fn save_configured(provider: Provider) -> std::io::Result<()> {
    crate::models::save_config_value("provider", provider.id())
}

/// Why a key cannot be used. Missing is the case the user fixes in Settings;
/// anything else (a locked keychain, a refused access prompt) is the
/// Keychain's own message, since re-entering the key would not help.
#[derive(Debug, PartialEq)]
pub enum KeyError {
    Missing(Cloud),
    Keychain(Cloud, String),
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            KeyError::Missing(cloud) => f.write_str(&tf("provider.key_missing", &[cloud.name()])),
            KeyError::Keychain(cloud, detail) => {
                f.write_str(&tf("provider.key_unreadable", &[cloud.name(), detail]))
            }
        }
    }
}

/// A failed `security` run: its exit status and what it said on stderr.
#[derive(Debug)]
pub struct SecurityFailure {
    code: Option<i32>,
    stderr: String,
}

impl SecurityFailure {
    fn detail(&self) -> String {
        let line = self.stderr.lines().rev().find(|l| !l.trim().is_empty());
        match (line, self.code) {
            (Some(line), _) => line.trim().to_owned(),
            (None, Some(code)) => format!("exit {code}"),
            (None, None) => "killed".to_owned(),
        }
    }
}

/// The API key from the Keychain.
pub fn api_key(cloud: Cloud) -> Result<String, KeyError> {
    api_key_with(cloud, run_security)
}

fn api_key_with(
    cloud: Cloud,
    run: impl Fn(&[&str], Option<&str>) -> Result<String, SecurityFailure>,
) -> Result<String, KeyError> {
    let key = run(
        &["find-generic-password", "-s", cloud.service(), "-w"],
        None,
    )
    .map_err(|e| key_error(cloud, e))?;
    let key = key.trim().to_owned();
    if key.is_empty() {
        return Err(KeyError::Missing(cloud));
    }
    Ok(key)
}

/// Whether a key is saved, without reading the secret itself.
pub fn key_status(cloud: Cloud) -> Result<(), KeyError> {
    key_status_with(cloud, run_security)
}

fn key_status_with(
    cloud: Cloud,
    run: impl Fn(&[&str], Option<&str>) -> Result<String, SecurityFailure>,
) -> Result<(), KeyError> {
    run(&["find-generic-password", "-s", cloud.service()], None)
        .map(|_| ())
        .map_err(|e| key_error(cloud, e))
}

fn key_error(cloud: Cloud, failure: SecurityFailure) -> KeyError {
    if failure.code == Some(ERR_SEC_ITEM_NOT_FOUND) {
        KeyError::Missing(cloud)
    } else {
        KeyError::Keychain(cloud, failure.detail())
    }
}

/// Stores the API key in the Keychain, replacing one that exists. The key
/// goes to `security -i` as a quoted command on stdin; an empty key is
/// refused, since it would read back as "no key saved" anyway.
pub fn save_api_key(cloud: Cloud, key: &str) -> Result<(), String> {
    save_api_key_with(cloud, key, run_security)
}

fn save_api_key_with(
    cloud: Cloud,
    key: &str,
    run: impl Fn(&[&str], Option<&str>) -> Result<String, SecurityFailure>,
) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() || key.contains(['\n', '\r']) {
        return Err(t("provider.key_empty").to_owned());
    }
    let quoted = key.replace('\\', "\\\\").replace('"', "\\\"");
    let command = format!(
        "add-generic-password -U -s {} -a momr -w \"{quoted}\"\n",
        cloud.service()
    );
    run(&["-i"], Some(&command))
        .map(|_| ())
        .map_err(|e| tf("provider.key_save_failed", &[&e.detail()]))
}

/// Runs `/usr/bin/security` by its full path: `extend_path` puts user-writable
/// folders ahead of `/usr/bin`, and this program handles the keys.
fn run_security(args: &[&str], stdin: Option<&str>) -> Result<String, SecurityFailure> {
    use std::io::Write;
    let failure = |stderr: String| SecurityFailure { code: None, stderr };
    let mut child = Command::new("/usr/bin/security")
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| failure(e.to_string()))?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(text.as_bytes())
            .map_err(|e| failure(e.to_string()))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| failure(e.to_string()))?;
    if !output.status.success() {
        return Err(SecurityFailure {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    String::from_utf8(output.stdout).map_err(|e| failure(e.to_string()))
}

/// One recognised word, the common shape of every provider's answer. Times
/// are within the chunk as parsed and absolute once `transcribe.rs` adds the
/// chunk's offset.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    /// The provider's voice id, meaningful within one request only.
    pub speaker: Option<String>,
}

impl Word {
    /// A word with its end never before its start, whatever the reply said.
    fn new(text: &str, start_ms: u64, end_ms: u64, speaker: Option<String>) -> Word {
        Word {
            text: text.to_owned(),
            start_ms,
            end_ms: end_ms.max(start_ms),
            speaker,
        }
    }
}

/// What one chunk came back as.
#[derive(Debug, PartialEq)]
pub struct Chunk {
    pub words: Vec<Word>,
    /// The language the provider detected, when it says.
    pub language: Option<String>,
    /// False when the model gave text without times (some OpenRouter models):
    /// the caller then spreads it over the chunk.
    pub timed: bool,
}

/// A failed chunk, and whether trying it again could help.
#[derive(Debug)]
pub struct ChunkError {
    pub message: String,
    pub transient: bool,
}

impl ChunkError {
    fn permanent(message: String) -> ChunkError {
        ChunkError {
            message,
            transient: false,
        }
    }
}

/// Our language codes to ElevenLabs' `language_code` (ISO-639-1); "auto" is
/// omitted so the service detects it.
pub fn eleven_language(code: &str) -> Option<&str> {
    if code == "auto" { None } else { Some(code) }
}

/// Our language codes to Google's BCP-47 `languageCode`. None for "auto" and
/// codes Google is not given here: its v1 API has no detection, and guessing
/// US English would hand back confident nonsense for another language.
pub fn google_language(code: &str) -> Option<&'static str> {
    match code {
        "en" => Some("en-US"),
        "id" => Some("id-ID"),
        "nl" => Some("nl-NL"),
        "de" => Some("de-DE"),
        "fr" => Some("fr-FR"),
        "es" => Some("es-ES"),
        "it" => Some("it-IT"),
        "pt" => Some("pt-PT"),
        _ => None,
    }
}

/// Refuses a run that cannot work before any audio is uploaded.
pub fn check_language(cloud: Cloud, language: &str) -> Result<(), String> {
    if cloud == Cloud::Google && google_language(language).is_none() {
        return Err(t("provider.google_needs_language").to_owned());
    }
    Ok(())
}

/// An HTTP agent whose calls end: a stalled upload would otherwise hold the
/// Transcribing screen forever, since Cancel is only checked between chunks.
/// The overall limit grows with the chunk, which is uploaded and then
/// transcribed in one call.
fn agent(cloud: Cloud) -> ureq::Agent {
    let chunk = Duration::from_millis(cloud.chunk_ms());
    ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            // Read error bodies ourselves, so a rejected key or quota comes
            // back as the provider's message rather than a bare status code.
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_global(Some(Duration::from_secs(120) + chunk))
            .build(),
    )
}

/// The provider's own message when it gives one, else the start of the body.
fn error_detail(status: u16, body: &str) -> String {
    let parsed = serde_json::from_str::<serde_json::Value>(body).ok();
    let structured = parsed.as_ref().and_then(|v| {
        v["error"]["message"]
            .as_str()
            .or_else(|| v["error"].as_str())
            .or_else(|| v["detail"]["message"].as_str())
            .or_else(|| v["detail"][0]["msg"].as_str())
            .or_else(|| v["detail"].as_str())
            .or_else(|| v["message"].as_str())
            .filter(|m| !m.is_empty())
            .map(str::to_owned)
    });
    structured.unwrap_or_else(|| {
        let snippet: String = body.trim().chars().take(200).collect();
        if snippet.is_empty() {
            format!("HTTP {status}")
        } else {
            format!("HTTP {status}: {snippet}")
        }
    })
}

fn check_status(cloud: Cloud, status: u16, body: &str) -> Result<serde_json::Value, ChunkError> {
    let name = cloud.name();
    let detail = || error_detail(status, body);
    match status {
        200..=299 => {
            let value: serde_json::Value = serde_json::from_str(body).map_err(|e| {
                ChunkError::permanent(tf("provider.err_not_json", &[name, &e.to_string()]))
            })?;
            // OpenRouter can answer 200 with an error object from the model.
            if value.get("error").is_some_and(|e| !e.is_null()) {
                return Err(ChunkError {
                    message: tf("provider.err_failed", &[name, &detail()]),
                    transient: true,
                });
            }
            Ok(value)
        }
        401 | 403 => Err(ChunkError::permanent(tf(
            "provider.err_rejected",
            &[name, &detail()],
        ))),
        // Google answers a bad key with 400 and this reason.
        400 if body.contains("API_KEY_INVALID") || body.contains("API key not valid") => Err(
            ChunkError::permanent(tf("provider.err_rejected", &[name, &detail()])),
        ),
        402 => Err(ChunkError::permanent(tf(
            "provider.err_credits",
            &[name, &detail()],
        ))),
        429 => Err(ChunkError {
            message: tf("provider.err_quota", &[name, &detail()]),
            transient: true,
        }),
        _ => Err(ChunkError {
            message: tf("provider.err_failed", &[name, &detail()]),
            transient: status >= 500,
        }),
    }
}

/// Sends one request and reads its reply. Transport failures (timeouts,
/// dropped connections) are worth another try; a reply is judged by status.
fn send(
    cloud: Cloud,
    request: impl FnOnce(&ureq::Agent) -> Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<serde_json::Value, ChunkError> {
    let name = cloud.name();
    let mut response = request(&agent(cloud)).map_err(|e| ChunkError {
        message: tf("provider.err_request", &[name, &e.to_string()]),
        transient: true,
    })?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| ChunkError {
            message: tf("provider.err_request", &[name, &e.to_string()]),
            transient: true,
        })?;
    check_status(cloud, status, &body)
}

/// One chunk through `cloud`, tried up to `ATTEMPTS` times while the failure
/// is transient, waiting a little longer each time. `aborted` is checked
/// before each retry, so Cancel does not wait out the back-off.
pub fn transcribe_chunk(
    cloud: Cloud,
    key: &str,
    wav: &Path,
    language: &str,
    aborted: &dyn Fn() -> bool,
) -> Result<Chunk, String> {
    let mut attempt = 1;
    loop {
        let result = match cloud {
            Cloud::ElevenLabs => transcribe_eleven(key, wav, language),
            Cloud::Google => transcribe_google(key, wav, language),
            Cloud::OpenRouter => transcribe_openrouter(key, wav, language),
        };
        match result {
            Ok(chunk) => return Ok(chunk),
            Err(e) if e.transient && attempt < ATTEMPTS && !aborted() => {
                eprintln!(
                    "{}: {} (attempt {attempt} of {ATTEMPTS}), retrying",
                    crate::APP_NAME,
                    e.message
                );
                std::thread::sleep(Duration::from_secs(2u64.pow(attempt)));
                attempt += 1;
            }
            Err(e) => return Err(e.message),
        }
    }
}

fn read_audio(wav: &Path) -> Result<Vec<u8>, ChunkError> {
    std::fs::read(wav).map_err(|e| ChunkError::permanent(format!("{}: {e}", wav.display())))
}

/// One ElevenLabs chunk: mono WAV in, words and the detected language code out.
fn transcribe_eleven(key: &str, wav: &Path, language: &str) -> Result<Chunk, ChunkError> {
    let audio = read_audio(wav)?;
    let value = send(Cloud::ElevenLabs, |agent| {
        let mut form = ureq::unversioned::multipart::Form::new()
            .text("model_id", "scribe_v1")
            .text("diarize", "true");
        if let Some(code) = eleven_language(language) {
            form = form.text("language_code", code);
        }
        let part = ureq::unversioned::multipart::Part::bytes(&audio)
            .mime_str("audio/wav")?
            .file_name("chunk.wav");
        agent
            .post(ELEVEN_URL)
            .header("xi-api-key", key)
            .send(form.part("file", part))
    })?;
    Ok(Chunk {
        words: parse_eleven_words(&value).map_err(ChunkError::permanent)?,
        language: value["language_code"].as_str().map(str::to_owned),
        timed: true,
    })
}

/// `words[]` (`text`, `start`/`end` seconds, `speaker_id` when diarized) to
/// `Word`s. The list also carries `spacing` entries (the blanks between
/// words, which the grouping puts back) and `audio_event` entries such as
/// "(laughter)", which are left out like whisper's noise markers. Silence is
/// an empty list; a reply with no list at all is an error, not silence.
pub fn parse_eleven_words(value: &serde_json::Value) -> Result<Vec<Word>, String> {
    let Some(words) = value["words"].as_array() else {
        return Err(tf("provider.err_shape", &["ElevenLabs", "words"]));
    };
    let mut out = Vec::with_capacity(words.len());
    let mut untimed = 0;
    for w in words {
        if matches!(w["type"].as_str(), Some("spacing" | "audio_event")) {
            continue;
        }
        let (Some(text), Some(start), Some(end)) =
            (w["text"].as_str(), w["start"].as_f64(), w["end"].as_f64())
        else {
            untimed += 1;
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        out.push(Word::new(
            text.trim(),
            seconds_to_ms(start),
            seconds_to_ms(end),
            w["speaker_id"].as_str().map(str::to_owned),
        ));
    }
    if untimed > out.len() {
        return Err(tf("provider.err_shape", &["ElevenLabs", "start/end"]));
    }
    Ok(out)
}

fn seconds_to_ms(secs: f64) -> u64 {
    (secs.max(0.0) * 1000.0).round() as u64
}

/// The request body for one Google chunk: 16 kHz mono LINEAR16, base64.
/// Pure, so tests cover it without the network. The language must already
/// have passed `check_language`.
pub fn google_body(audio: &[u8], language: &str) -> serde_json::Value {
    serde_json::json!({
        "config": {
            "encoding": "LINEAR16",
            "sampleRateHertz": 16000,
            "languageCode": google_language(language).unwrap_or("en-US"),
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

/// One Google chunk: 16 kHz mono WAV in, words out. Google takes raw
/// LINEAR16 without the WAV header, so the chunk is converted once more
/// through ffmpeg. The key goes in the `x-goog-api-key` header rather than
/// the URL, which ends up in error messages.
fn transcribe_google(key: &str, wav_16k: &Path, language: &str) -> Result<Chunk, ChunkError> {
    let raw = wav_to_mono16k_raw(wav_16k).map_err(ChunkError::permanent)?;
    let body = google_body(&raw, language);
    let value = send(Cloud::Google, |agent| {
        agent
            .post(GOOGLE_URL)
            .header("x-goog-api-key", key)
            .send_json(&body)
    })?;
    Ok(Chunk {
        words: parse_google_words(&value),
        language: None,
        timed: true,
    })
}

/// The transcription model on OpenRouter, `openrouter_model` in config.toml,
/// Whisper-1 unless set; any transcription-capable slug works
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
fn transcribe_openrouter(key: &str, wav: &Path, language: &str) -> Result<Chunk, ChunkError> {
    let audio = read_audio(wav)?;
    let body = openrouter_body(&audio, language, &openrouter_model());
    let value = send(Cloud::OpenRouter, |agent| {
        agent
            .post(OPENROUTER_URL)
            .header("Authorization", format!("Bearer {key}"))
            .send_json(&body)
    })?;
    let (words, timed) = parse_openrouter_words(&value);
    Ok(Chunk {
        words,
        language: value["language"].as_str().map(str::to_owned),
        timed,
    })
}

/// verbose_json `segments[].words[]` (`word`, `start`/`end` seconds) to
/// `Word`s; falls back to whole segments, then to plain `text`, which has no
/// times (the `bool` is false then, and the caller spreads it over the chunk).
pub fn parse_openrouter_words(value: &serde_json::Value) -> (Vec<Word>, bool) {
    let mut out = Vec::new();
    for segment in value["segments"].as_array().into_iter().flatten() {
        let seg_start = segment["start"].as_f64().unwrap_or(0.0).max(0.0);
        let seg_end = segment["end"].as_f64().unwrap_or(seg_start).max(seg_start);
        let words = segment["words"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if words.is_empty() {
            if let Some(text) = segment["text"].as_str().filter(|t| !t.trim().is_empty()) {
                out.push(Word::new(
                    text.trim(),
                    seconds_to_ms(seg_start),
                    seconds_to_ms(seg_end),
                    None,
                ));
            }
            continue;
        }
        for w in words {
            let (Some(text), Some(start), Some(end)) =
                (w["word"].as_str(), w["start"].as_f64(), w["end"].as_f64())
            else {
                continue;
            };
            out.push(Word::new(
                text.trim(),
                seconds_to_ms(start),
                seconds_to_ms(end),
                None,
            ));
        }
    }
    if out.is_empty()
        && let Some(text) = value["text"].as_str().filter(|t| !t.trim().is_empty())
    {
        return (vec![Word::new(text.trim(), 0, 0, None)], false);
    }
    (out, true)
}

/// Runs ffmpeg and keeps the last line it printed, so a failure says why.
fn run_ffmpeg(args: &[&std::ffi::OsStr], what: &str) -> Result<(), String> {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-nostdin"])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| tf("provider.ffmpeg_missing", &[&e.to_string()]))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let reason = stderr
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");
    Err(format!("{what}: {}", reason.trim()))
}

fn wav_to_mono16k_raw(wav: &Path) -> Result<Vec<u8>, String> {
    let out = wav.with_extension("g16.s16");
    run_ffmpeg(
        &[
            "-i".as_ref(),
            wav.as_os_str(),
            "-f".as_ref(),
            "s16le".as_ref(),
            "-ar".as_ref(),
            "16000".as_ref(),
            "-ac".as_ref(),
            "1".as_ref(),
            out.as_os_str(),
        ],
        t("provider.ffmpeg_google"),
    )?;
    let bytes = std::fs::read(&out).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&out);
    Ok(bytes)
}

/// `results[].alternatives[0].words[]` (`word`, `startTime`/`endTime` like
/// `"1.200s"`, `speakerTag`) to `Word`s. With diarization on, Google repeats
/// every word of the request in the last result, this time with speaker
/// tags, so then only that result is read; reading them all would give each
/// word twice.
pub fn parse_google_words(value: &serde_json::Value) -> Vec<Word> {
    let results: Vec<&serde_json::Value> =
        value["results"].as_array().into_iter().flatten().collect();
    let words_of = |result: &serde_json::Value| -> Vec<serde_json::Value> {
        result["alternatives"][0]["words"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    };
    let tagged = |result: &&serde_json::Value| {
        words_of(result)
            .iter()
            .any(|w| w["speakerTag"].as_u64().is_some_and(|tag| tag > 0))
    };
    let chosen: Vec<&serde_json::Value> = match results.last() {
        Some(last) if tagged(last) => vec![*last],
        _ => results,
    };
    let mut out = Vec::new();
    for result in chosen {
        for w in words_of(result) {
            let (Some(text), Some(start), Some(end)) = (
                w["word"].as_str(),
                w["startTime"].as_str().and_then(parse_google_time),
                w["endTime"].as_str().and_then(parse_google_time),
            ) else {
                continue;
            };
            out.push(Word::new(
                text,
                start,
                end,
                w["speakerTag"]
                    .as_u64()
                    .filter(|tag| *tag > 0)
                    .map(|tag| format!("speaker_{tag}")),
            ));
        }
    }
    out
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

/// Cuts one mono WAV chunk, `range` in ms, from `src` into `out` at `rate`.
pub fn write_wav_chunk(src: &Path, range: (u64, u64), out: &Path, rate: u32) -> Result<(), String> {
    let (start, end) = range;
    let seconds = |ms: u64| format!("{:.3}", ms as f64 / 1000.0);
    let (start_s, length_s, rate_s) = (seconds(start), seconds(end - start), rate.to_string());
    run_ffmpeg(
        &[
            "-ss".as_ref(),
            start_s.as_ref(),
            "-t".as_ref(),
            length_s.as_ref(),
            "-i".as_ref(),
            src.as_os_str(),
            "-ar".as_ref(),
            rate_s.as_ref(),
            "-ac".as_ref(),
            "1".as_ref(),
            out.as_os_str(),
        ],
        t("provider.ffmpeg_cut"),
    )
}

/// The longest a grouped line grows before a new one starts, in characters.
const LINE_CHARS: usize = 60;

/// Groups words into lines: same speaker, split on gaps past `max_gap_ms` or
/// once the next word would take the line past `LINE_CHARS`, so a cloud reply
/// reads like transcript lines. Each line is a `Word` spanning its words.
pub fn group_words(words: &[Word], max_gap_ms: u64) -> Vec<Word> {
    let mut lines: Vec<Word> = Vec::new();
    for word in words {
        match lines.last_mut() {
            Some(line)
                if line.speaker == word.speaker
                    && word.start_ms.saturating_sub(line.end_ms) <= max_gap_ms
                    && line.text.chars().count() + 1 + word.text.chars().count() <= LINE_CHARS =>
            {
                line.text.push(' ');
                line.text.push_str(&word.text);
                line.end_ms = line.end_ms.max(word.end_ms);
            }
            _ => lines.push(word.clone()),
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, start_ms: u64, end_ms: u64, speaker: Option<&str>) -> Word {
        Word::new(text, start_ms, end_ms, speaker.map(str::to_owned))
    }

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
                {"text": "hi", "start": 0.1, "end": 0.3, "type": "word", "speaker_id": "speaker_0"},
                {"text": " ", "start": 0.3, "end": 0.4, "type": "spacing", "speaker_id": "speaker_0"},
                {"text": "(laughter)", "start": 0.3, "end": 0.4, "type": "audio_event"},
                {"text": "there", "start": 0.4, "end": 0.7, "type": "word", "speaker_id": "speaker_1"},
            ],
        });
        let words = parse_eleven_words(&value).unwrap();
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].start_ms, 100);
        assert_eq!(words[1].speaker.as_deref(), Some("speaker_1"));
        // Silence is an empty word list, not an error.
        assert_eq!(
            parse_eleven_words(&serde_json::json!({"text": "", "words": []})).unwrap(),
            vec![]
        );
        // A reply without the list is a changed API, not silence.
        assert!(parse_eleven_words(&serde_json::json!({"text": "hi"})).is_err());
    }

    /// Spacing entries used to become words of their own, and the grouping
    /// joined them with more blanks: "hi   there".
    #[test]
    fn eleven_spacing_does_not_double_blanks() {
        let value = serde_json::json!({"words": [
            {"text": "hi", "start": 0.0, "end": 0.2, "type": "word"},
            {"text": " ", "start": 0.2, "end": 0.3, "type": "spacing"},
            {"text": "there", "start": 0.3, "end": 0.5, "type": "word"},
        ]});
        let lines = group_words(&parse_eleven_words(&value).unwrap(), 700);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "hi there");
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
        let words = parse_google_words(&value);
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].speaker.as_deref(), Some("speaker_1"));
        assert_eq!(words[1].end_ms, 800);
    }

    /// With diarization Google sends the words once per result without tags,
    /// then all of them again, tagged, in the last result.
    #[test]
    fn google_diarized_words_come_once() {
        let untagged = |w: &str, s: &str, e: &str| serde_json::json!({"word": w, "startTime": s, "endTime": e});
        let tagged = |w: &str, s: &str, e: &str, tag: u64| serde_json::json!({"word": w, "startTime": s, "endTime": e, "speakerTag": tag});
        let value = serde_json::json!({"results": [
            {"alternatives": [{"words": [untagged("halo", "0s", "0.5s")]}]},
            {"alternatives": [{"words": [untagged("dunia", "1s", "1.5s")]}]},
            {"alternatives": [{"words": [
                tagged("halo", "0s", "0.5s", 1),
                tagged("dunia", "1s", "1.5s", 2),
            ]}]},
        ]});
        let words = parse_google_words(&value);
        let texts: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, ["halo", "dunia"]);
        assert!(words.iter().all(|w| w.speaker.is_some()));
        // Without diarization every result counts.
        let plain = serde_json::json!({"results": [
            {"alternatives": [{"words": [untagged("a", "0s", "1s")]}]},
            {"alternatives": [{"words": [untagged("b", "1s", "2s")]}]},
        ]});
        assert_eq!(parse_google_words(&plain).len(), 2);
        // Google answers silence with an empty object.
        assert!(parse_google_words(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn google_languages_are_explicit() {
        assert_eq!(google_language("id"), Some("id-ID"));
        assert_eq!(google_language("en"), Some("en-US"));
        assert_eq!(google_language("auto"), None);
        assert!(check_language(Cloud::Google, "auto").is_err());
        assert!(check_language(Cloud::Google, "id").is_ok());
        assert!(check_language(Cloud::ElevenLabs, "auto").is_ok());
        // Every language offered in the app has its own Google locale.
        for code in crate::transcribe::LANGUAGE_CODES {
            if code != "auto" {
                assert!(google_language(code).is_some(), "{code}");
            }
        }
        let body = google_body(&[0, 0], "id");
        assert_eq!(body["config"]["languageCode"], "id-ID");
        assert_eq!(body["config"]["sampleRateHertz"], 16000);
        assert_eq!(body["audio"]["content"], "AAA=");
    }

    #[test]
    fn status_errors_name_the_cause() {
        let err = |cloud, status, body| check_status(cloud, status, body).unwrap_err();
        assert!(
            err(Cloud::ElevenLabs, 401, r#"{"detail":{"message":"x"}}"#)
                .message
                .contains("rejected the API key")
        );
        assert!(
            err(Cloud::Google, 403, "{}")
                .message
                .contains("rejected the API key")
        );
        let bad_google_key = r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT"}}"#;
        assert!(
            err(Cloud::Google, 400, bad_google_key)
                .message
                .contains("rejected the API key")
        );
        let quota = err(Cloud::Google, 429, "{}");
        assert!(quota.message.contains("quota") && quota.transient);
        assert!(check_status(Cloud::Google, 200, r#"{"results":[]}"#).is_ok());
        let boom = err(Cloud::Google, 500, "boom");
        assert!(boom.message.contains("HTTP 500: boom") && boom.transient);
        assert!(!err(Cloud::Google, 400, "{}").transient);
        assert!(
            err(
                Cloud::OpenRouter,
                402,
                r#"{"error":{"message":"insufficient credits"}}"#
            )
            .message
            .contains("out of credits")
        );
        // ElevenLabs validation errors and plain-string details.
        assert!(
            err(
                Cloud::ElevenLabs,
                422,
                r#"{"detail":[{"msg":"file too large"}]}"#
            )
            .message
            .contains("file too large")
        );
        assert!(
            err(Cloud::ElevenLabs, 400, r#"{"detail":"bad"}"#)
                .message
                .contains("bad")
        );
        // An empty message falls back to the body.
        assert!(
            err(Cloud::OpenRouter, 400, r#"{"error":{"message":""}}"#)
                .message
                .contains("HTTP 400")
        );
        // A 200 that is not JSON, or carries an error object, is no transcript.
        assert!(
            err(Cloud::OpenRouter, 200, "<html>")
                .message
                .contains("not JSON")
        );
        assert!(
            err(
                Cloud::OpenRouter,
                200,
                r#"{"error":{"message":"model busy"}}"#
            )
            .message
            .contains("model busy")
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
    fn words_group_by_speaker_gap_and_length() {
        let words = vec![
            word("a", 0, 100, Some("s0")),
            word("b", 150, 250, Some("s0")),
            word("c", 5000, 5100, Some("s0")),
            word("d", 5150, 5250, Some("s1")),
        ];
        let lines = group_words(&words, 700);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].text, "a b");
        assert_eq!((lines[0].start_ms, lines[0].end_ms), (0, 250));
        assert_eq!(lines[2].speaker.as_deref(), Some("s1"));
        // A line never grows past LINE_CHARS.
        let long: Vec<Word> = (0..40)
            .map(|i| word("word", i * 100, i * 100 + 50, None))
            .collect();
        assert!(
            group_words(&long, 700)
                .iter()
                .all(|l| l.text.chars().count() <= LINE_CHARS)
        );
    }

    #[test]
    fn words_never_end_before_they_start() {
        assert_eq!(Word::new("x", 500, 100, None).end_ms, 500);
    }

    fn security_fake<'a>(
        calls: &'a std::cell::RefCell<Vec<(String, Option<String>)>>,
        answer: Result<&'static str, (Option<i32>, &'static str)>,
    ) -> impl Fn(&[&str], Option<&str>) -> Result<String, SecurityFailure> + 'a {
        move |args, stdin| {
            calls
                .borrow_mut()
                .push((args.join(" "), stdin.map(str::to_owned)));
            answer
                .map(str::to_owned)
                .map_err(|(code, stderr)| SecurityFailure {
                    code,
                    stderr: stderr.to_owned(),
                })
        }
    }

    #[test]
    fn keychain_reads_tell_missing_from_broken() {
        let calls = std::cell::RefCell::new(Vec::new());
        assert_eq!(
            api_key_with(Cloud::ElevenLabs, security_fake(&calls, Ok("key-123\n"))).as_deref(),
            Ok("key-123")
        );
        assert!(calls.borrow()[0].0.contains("momr-elevenlabs"));
        assert_eq!(
            api_key_with(
                Cloud::Google,
                security_fake(&calls, Err((Some(44), "not found")))
            ),
            Err(KeyError::Missing(Cloud::Google))
        );
        assert_eq!(
            api_key_with(
                Cloud::Google,
                security_fake(&calls, Err((Some(51), "User interaction is not allowed.")))
            ),
            Err(KeyError::Keychain(
                Cloud::Google,
                "User interaction is not allowed.".into()
            ))
        );
        assert_eq!(
            api_key_with(Cloud::OpenRouter, security_fake(&calls, Ok("\n"))),
            Err(KeyError::Missing(Cloud::OpenRouter))
        );
        // The existence check never asks for the secret.
        calls.borrow_mut().clear();
        assert!(key_status_with(Cloud::Google, security_fake(&calls, Ok("attrs"))).is_ok());
        assert!(!calls.borrow()[0].0.contains("-w"));
    }

    #[test]
    fn keys_are_saved_through_stdin() {
        let calls = std::cell::RefCell::new(Vec::new());
        save_api_key_with(
            Cloud::OpenRouter,
            "  sk-\"a\\b  ",
            security_fake(&calls, Ok("")),
        )
        .unwrap();
        let (args, stdin) = calls.borrow()[0].clone();
        assert_eq!(args, "-i");
        assert!(!args.contains("sk-"));
        assert_eq!(
            stdin.as_deref(),
            Some("add-generic-password -U -s momr-openrouter -a momr -w \"sk-\\\"a\\\\b\"\n")
        );
        // Empty keys and keys with line breaks never reach the Keychain.
        calls.borrow_mut().clear();
        assert!(save_api_key_with(Cloud::Google, "   ", security_fake(&calls, Ok(""))).is_err());
        assert!(save_api_key_with(Cloud::Google, "a\nb", security_fake(&calls, Ok(""))).is_err());
        assert!(calls.borrow().is_empty());
        // A failed save carries security's reason.
        let err = save_api_key_with(
            Cloud::Google,
            "k",
            security_fake(&calls, Err((Some(2), "add-generic-password: returned 2"))),
        )
        .unwrap_err();
        assert!(err.contains("returned 2"));
    }

    #[test]
    fn wav_header_round_trips() {
        let dir = workdir().unwrap();
        let path = dir.join("test.wav");
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
        let _ = std::fs::remove_dir_all(&dir);
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
                 "words": [{"word": " halo", "start": 0.1, "end": 0.3}]},
                {"text": "dunia", "start": 0.4, "end": 0.8, "words": []},
            ],
        });
        let (words, timed) = parse_openrouter_words(&value);
        assert!(timed);
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].text, "halo");
        assert_eq!(words[0].start_ms, 100);
        assert_eq!(words[1].text, "dunia");
        assert_eq!(words[1].end_ms, 800);
        assert!(words.iter().all(|w| w.speaker.is_none()));
        let (bare, timed) = parse_openrouter_words(&serde_json::json!({"text": "hi"}));
        assert_eq!(bare.len(), 1);
        assert!(!timed);
        assert!(parse_openrouter_words(&serde_json::json!({})).0.is_empty());
    }

    #[test]
    fn ids_round_trip() {
        for provider in Provider::ALL {
            assert_eq!(Provider::from_id(provider.id()), Some(provider));
        }
        assert_eq!(
            Provider::from_id(" elevenlabs "),
            Some(Provider::Cloud(Cloud::ElevenLabs))
        );
        assert_eq!(Provider::from_id("whisper"), None);
        assert_eq!(Provider::from_id(""), None);
    }
}
