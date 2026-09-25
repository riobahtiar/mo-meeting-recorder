//! Remembered preferences: the audio format, the transcription language and
//! the name you go by in transcripts.

use std::path::PathBuf;

use crate::export::Format;
use crate::transcribe::LANGUAGE_CODES;

fn path() -> PathBuf {
    crate::paths::settings_file()
}

fn load() -> serde_json::Value {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .filter(|value| value.is_object())
        .unwrap_or_else(|| serde_json::json!({}))
}

/// Updates one key and keeps the others.
fn save(key: &str, value: &str) {
    let mut settings = load();
    settings[key] = serde_json::Value::String(value.to_owned());
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, settings.to_string());
}

pub fn load_format() -> Format {
    load()["format"]
        .as_str()
        .map(Format::from_key)
        .unwrap_or(Format::Mono)
}

pub fn save_format(format: Format) {
    save("format", format.key());
}

/// A whisper language code from `LANGUAGE_CODES`, "auto" when unset or unknown.
pub fn load_language() -> &'static str {
    let settings = load();
    let saved = settings["language"].as_str().unwrap_or("auto");
    LANGUAGE_CODES
        .iter()
        .find(|code| **code == saved)
        .copied()
        .unwrap_or("auto")
}

pub fn save_language(code: &str) {
    save("language", code);
}

/// What the mic side is called in new transcripts, "You" until you change it.
pub fn load_your_name() -> String {
    load()["your_name"]
        .as_str()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(crate::meeting::default_you())
        .to_owned()
}

pub fn save_your_name(name: &str) {
    save("your_name", name);
}

/// An overridden meetings folder, or None for the Documents default.
pub fn load_meetings_dir() -> Option<std::path::PathBuf> {
    load()["meetings_dir"]
        .as_str()
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
}

pub fn save_meetings_dir(dir: &std::path::Path) {
    save("meetings_dir", &dir.display().to_string());
}

/// The interface language: "id" for Indonesian, anything else English.
/// Defaults from $LANG so an Indonesian system starts in Indonesian.
pub fn load_ui_language() -> &'static str {
    let settings = load();
    let saved = settings["ui_language"].as_str().unwrap_or("");
    if saved == "id" || saved == "en" {
        return if saved == "id" { "id" } else { "en" };
    }
    if std::env::var("LANG").is_ok_and(|lang| lang.starts_with("id")) {
        "id"
    } else {
        "en"
    }
}

pub fn save_ui_language(code: &str) {
    save("ui_language", code);
}
