//! Which whisper model transcribes, where it is on disk, and fetching it.
//!
//! The model is picked with `--model` on the command line, or `model = "…"`
//! in `~/.config/momr/config.toml`, and is
//! `large-v3-turbo` otherwise. A name from `MODELS` is looked for in the app's
//! own model folder and in voxtype's (same files, no need to have them twice),
//! and downloaded when it is in neither. A path to a `.bin` file is used as is.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use gtk::glib;
use whisper_rs::DtwModelPreset;

use crate::transcribe::{Abort, Events, download, models_dir};

pub const DEFAULT: &str = "large-v3-turbo";

pub struct Model {
    pub name: &'static str,
    /// Download size in MB, for the text in the app.
    pub size_mb: u32,
    preset: DtwModelPreset,
}

pub const MODELS: [Model; 10] = [
    Model {
        name: "tiny",
        size_mb: 75,
        preset: DtwModelPreset::Tiny,
    },
    Model {
        name: "tiny.en",
        size_mb: 75,
        preset: DtwModelPreset::TinyEn,
    },
    Model {
        name: "base",
        size_mb: 142,
        preset: DtwModelPreset::Base,
    },
    Model {
        name: "base.en",
        size_mb: 142,
        preset: DtwModelPreset::BaseEn,
    },
    Model {
        name: "small",
        size_mb: 466,
        preset: DtwModelPreset::Small,
    },
    Model {
        name: "small.en",
        size_mb: 466,
        preset: DtwModelPreset::SmallEn,
    },
    Model {
        name: "medium",
        size_mb: 1500,
        preset: DtwModelPreset::Medium,
    },
    Model {
        name: "medium.en",
        size_mb: 1500,
        preset: DtwModelPreset::MediumEn,
    },
    Model {
        name: "large-v3",
        size_mb: 3100,
        preset: DtwModelPreset::LargeV3,
    },
    Model {
        name: "large-v3-turbo",
        size_mb: 1600,
        preset: DtwModelPreset::LargeV3Turbo,
    },
];

/// Set by `--model`; wins over the config file.
static OVERRIDE: Mutex<Option<String>> = Mutex::new(None);
/// Held while a model downloads, so a second caller waits instead of
/// fetching the same file again.
static DOWNLOADING: Mutex<()> = Mutex::new(());

pub fn set_override(name: &str) {
    *OVERRIDE.lock().unwrap() = Some(name.trim().to_owned());
}

pub fn config_file() -> PathBuf {
    crate::paths::config_file()
}

/// The value of `key = "…"` in config.toml, comments stripped; None when the
/// key is absent or empty.
pub fn config_value(key: &str) -> Option<String> {
    std::fs::read_to_string(config_file())
        .ok()
        .and_then(|text| parse_config_value(&text, key))
}

/// The value of `key = "…"` in one config file's text, so tests can cover the
/// shape without touching the real file.
fn parse_config_value(text: &str, key: &str) -> Option<String> {
    text.lines()
        .find_map(|line| {
            let (found, value) = line.split_once('=')?;
            (found.trim() == key).then(|| {
                let value = value.split('#').next().unwrap_or("");
                value.trim().trim_matches('"').to_owned()
            })
        })
        .filter(|value| !value.is_empty())
}

/// The configured model: a name from `MODELS` or a path to a model file.
pub fn configured() -> String {
    if let Some(name) = OVERRIDE.lock().unwrap().clone() {
        return name;
    }
    config_value("model").unwrap_or_else(|| DEFAULT.to_owned())
}

/// Rewrites `key = "value"` in config.toml, appending it when absent and
pub fn save_config_value(key: &str, value: &str) {
    let path = config_file();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let text = rewrite_config_line(&text, key, value);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, text);
}

/// `text` with `key = "value"` replaced or appended.
fn rewrite_config_line(text: &str, key: &str, value: &str) -> String {
    let line = format!("{key} = \"{value}\"");
    let mut replaced = false;
    let mut out: Vec<String> = text
        .lines()
        .map(|existing| {
            if !replaced
                && existing
                    .split_once('=')
                    .is_some_and(|(k, _)| k.trim() == key)
            {
                replaced = true;
                line.clone()
            } else {
                existing.to_owned()
            }
        })
        .collect();
    if !replaced {
        out.push(line);
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

fn known(name: &str) -> Option<&'static Model> {
    let name = name.strip_prefix("ggml-").unwrap_or(name);
    let name = name.strip_suffix(".bin").unwrap_or(name);
    MODELS.iter().find(|m| m.name == name)
}

fn file_name(model: &Model) -> String {
    format!("ggml-{}.bin", model.name)
}

/// voxtype's models, wherever voxtype keeps them: same files, no need to have
/// them twice.
fn voxtype_models() -> PathBuf {
    glib::user_data_dir().join("voxtype/models")
}

/// A complete model file: at least most of its expected size.
fn usable(path: &Path, model: Option<&Model>) -> bool {
    let min = model.map_or(10_000_000, |m| u64::from(m.size_mb) * 800_000);
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() >= min)
}

/// The configured model's file, when it is on disk.
pub fn find() -> Option<PathBuf> {
    let name = configured();
    match known(&name) {
        Some(model) => [models_dir(), voxtype_models()]
            .into_iter()
            .map(|dir| dir.join(file_name(model)))
            .find(|path| usable(path, Some(model))),
        None => {
            let path = PathBuf::from(&name);
            usable(&path, None).then_some(path)
        }
    }
}

/// For the app: the model's name and its size when it still has to be downloaded.
pub fn missing() -> Option<(String, u32)> {
    if find().is_some() {
        return None;
    }
    let name = configured();
    known(&name).map(|m| (m.name.to_owned(), m.size_mb))
}

/// The model file, downloaded first when needed. Blocking.
pub fn ensure(events: &Events, abort: &Abort) -> Result<PathBuf, String> {
    let _one_at_a_time = DOWNLOADING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(path) = find() {
        return Ok(path);
    }
    let name = configured();
    let Some(model) = known(&name) else {
        let names: Vec<&str> = MODELS.iter().map(|m| m.name).collect();
        return Err(format!(
            "unknown model \"{name}\": use one of {} or a path to a model file",
            names.join(", ")
        ));
    };
    let target = models_dir().join(file_name(model));
    let url = format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
        file_name(model)
    );
    download(
        &url,
        &target,
        "Downloading model",
        u64::from(model.size_mb) * 800_000,
        events,
        abort,
    )?;
    Ok(target)
}

/// The attention-head preset for word times; None for a model file of unknown kind.
pub fn dtw_preset() -> Option<DtwModelPreset> {
    known(&configured()).map(|m| m.preset.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_file_names_both_resolve() {
        assert_eq!(known("large-v3").map(|m| m.name), Some("large-v3"));
        assert_eq!(known("ggml-small.en.bin").map(|m| m.name), Some("small.en"));
        assert!(known("gpt-5").is_none());
    }

    #[test]
    fn config_values_read_by_key() {
        let text = "model = \"tiny\" # for tests\nagent = \"claude\"\n";
        assert_eq!(parse_config_value(text, "model").as_deref(), Some("tiny"));
        assert_eq!(parse_config_value(text, "agent").as_deref(), Some("claude"));
        assert_eq!(parse_config_value(text, "missing"), None);
        assert_eq!(parse_config_value("agent = \"\"\n", "agent"), None);
    }

    #[test]
    fn config_lines_rewrite_and_append() {
        let text = "# keep me\nmodel = \"tiny\"\n";
        let rewritten = rewrite_config_line(text, "model", "small");
        assert!(rewritten.contains("model = \"small\""));
        assert!(rewritten.contains("# keep me"));
        assert!(!rewritten.contains("\"tiny\""));
        let appended = rewrite_config_line(text, "agent", "pi");
        assert!(appended.contains("model = \"tiny\""));
        assert!(appended.contains("agent = \"pi\""));
    }
}
