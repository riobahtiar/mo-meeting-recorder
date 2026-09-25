//! Remembered preferences in settings.json: the audio format, the
//! transcription language, the name you go by in transcripts, a relocated
//! meetings folder, the interface language, the appearance, the window size,
//! the timer's default length and the chosen audio sources. (Which engines
//! run lives in config.toml, see `models::config_value`, because users edit
//! that by hand.)

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

/// Updates one key and keeps the others. A settings file that cannot be read
/// is left alone rather than replaced by one holding only this key; one that
/// is not JSON (hand-edited, say) is replaced, since nothing in it can be used.
fn save(key: &str, value: &str) -> std::io::Result<()> {
    save_value(key, serde_json::Value::String(value.to_owned()))
}

/// `save` for any JSON value; `Null` removes the key, so an unset choice
/// reads as absent rather than as a literal null.
fn save_value(key: &str, value: serde_json::Value) -> std::io::Result<()> {
    let path = path();
    let mut settings = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .filter(|value| value.is_object())
            .unwrap_or_else(|| serde_json::json!({})),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(e) => return Err(e),
    };
    if value.is_null() {
        if let Some(object) = settings.as_object_mut() {
            object.remove(key);
        }
    } else {
        settings[key] = value;
    }
    write_atomic(&path, settings.to_string().as_bytes())
}

/// Writes through a temporary file and a rename, so a reader never sees a
/// half-written file and a failed write leaves the old one whole.
pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

pub fn load_format() -> Format {
    load()["format"]
        .as_str()
        .map(Format::from_key)
        .unwrap_or(Format::Mono)
}

pub fn save_format(format: Format) -> std::io::Result<()> {
    save("format", format.key())
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

pub fn save_language(code: &str) -> std::io::Result<()> {
    save("language", code)
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

pub fn save_your_name(name: &str) -> std::io::Result<()> {
    save("your_name", name)
}

/// An overridden meetings folder, or None for the Documents default.
pub fn load_meetings_dir() -> Option<std::path::PathBuf> {
    load()["meetings_dir"]
        .as_str()
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
}

pub fn save_meetings_dir(dir: &std::path::Path) -> std::io::Result<()> {
    save("meetings_dir", &dir.display().to_string())
}

/// The meetings folder: the one chosen in Settings, else `~/Documents/Meetings`.
/// The single place that resolves it, so the library and Settings agree.
pub fn meetings_dir() -> std::path::PathBuf {
    load_meetings_dir().unwrap_or_else(crate::paths::meetings)
}

/// The interface language saved in Settings, None when never chosen (then
/// `locales::current` follows the system).
pub fn load_ui_language() -> Option<crate::locales::Lang> {
    load()["ui_language"]
        .as_str()
        .and_then(crate::locales::Lang::from_code)
}

pub fn save_ui_language(lang: crate::locales::Lang) -> std::io::Result<()> {
    save("ui_language", lang.code())
}

pub use crate::theme::Appearance;

pub fn load_appearance() -> Appearance {
    load()["appearance"]
        .as_str()
        .map(Appearance::from_key)
        .unwrap_or(Appearance::System)
}

pub fn save_appearance(appearance: Appearance) -> std::io::Result<()> {
    save("appearance", appearance.key())
}

/// The window size it was last closed with, when one was saved and it is
/// something a window can be.
pub fn load_window_size() -> Option<(i32, i32)> {
    let settings = load();
    let width = settings["window_width"].as_i64()?;
    let height = settings["window_height"].as_i64()?;
    ((200..=20_000).contains(&width) && (100..=20_000).contains(&height))
        .then_some((width as i32, height as i32))
}

pub fn save_window_size(width: i32, height: i32) -> std::io::Result<()> {
    save_value("window_width", serde_json::json!(width))?;
    save_value("window_height", serde_json::json!(height))
}

/// The "stop after" length the timer dialog opens with, in minutes.
pub fn load_timer_minutes() -> u32 {
    load()["timer_minutes"]
        .as_u64()
        .filter(|m| (1..=24 * 60).contains(m))
        .map_or(60, |m| m as u32)
}

pub fn save_timer_minutes(minutes: u32) -> std::io::Result<()> {
    save_value("timer_minutes", serde_json::json!(minutes))
}

/// The microphone to record, by Core Audio device UID; None follows the
/// system default.
pub fn load_mic_device() -> Option<String> {
    load()["mic_device"]
        .as_str()
        .map(str::trim)
        .filter(|uid| !uid.is_empty())
        .map(str::to_owned)
}

pub fn save_mic_device(uid: Option<&str>) -> std::io::Result<()> {
    save_value(
        "mic_device",
        uid.map_or(serde_json::Value::Null, |uid| {
            serde_json::Value::String(uid.to_owned())
        }),
    )
}

/// The apps whose audio is recorded, by bundle identifier; empty means
/// every app.
pub fn load_computer_sources() -> Vec<String> {
    load()["computer_sources"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|v| v.as_str())
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

pub fn save_computer_sources(bundles: &[String]) -> std::io::Result<()> {
    save_value(
        "computer_sources",
        if bundles.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::json!(bundles)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_keys_round_trip() {
        for appearance in Appearance::ALL {
            assert_eq!(Appearance::from_key(appearance.key()), appearance);
        }
        assert_eq!(Appearance::from_key("purple"), Appearance::System);
    }
}
