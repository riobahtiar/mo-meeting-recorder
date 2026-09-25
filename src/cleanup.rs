//! Clearing what the app accumulates, for Settings › Storage: the cache
//! (staging folders of recordings, import scratch, the socket), the downloaded
//! speech models, and the two settings files. Meetings are the user's
//! documents and the Keychain keys are the user's secrets; nothing here
//! touches either, and the callers say so in their confirmations.
//!
//! Every function takes its paths, so tests run on a temporary directory and
//! the app passes the real ones from `paths.rs`. A staging folder with audio
//! in it is an unfinished recording the next launch would offer to save;
//! clearing counts those so the confirmation can name them, and the caller
//! passes the folder of the recording in progress and the socket in `keep`.

use std::path::{Path, PathBuf};

/// What `clear_cache` removed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Cleared {
    pub bytes: u64,
    /// Staging folders with audio in them that are gone now.
    pub unfinished_recordings: usize,
}

/// Bytes under `path`: the file's size, or the sum of a folder's contents.
/// Unreadable entries count as zero rather than failing the whole size.
pub fn size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_dir() {
        std::fs::read_dir(path)
            .map(|entries| entries.flatten().map(|e| size(&e.path())).sum())
            .unwrap_or(0)
    } else {
        meta.len()
    }
}

/// `bytes` for a subtitle: "1.2 GB", "340 MB", "12 KB", "0 B".
pub fn human(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Whether a cache entry is a staging folder holding recorded audio, the
/// same test `ui.rs` uses to offer a recovery.
fn is_unfinished_recording(dir: &Path) -> bool {
    dir.is_dir()
        && ["mic.raw", "system.raw"]
            .iter()
            .any(|name| std::fs::metadata(dir.join(name)).is_ok_and(|m| m.len() > 0))
}

/// Removes every entry of `cache` except the paths in `keep`. A missing
/// cache is already clear. The first error stops the sweep, with whatever
/// was removed before it counted.
pub fn clear_cache(cache: &Path, keep: &[PathBuf]) -> std::io::Result<Cleared> {
    let mut cleared = Cleared::default();
    let entries = match std::fs::read_dir(cache) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(cleared),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let path = entry?.path();
        if keep.contains(&path) {
            continue;
        }
        let bytes = size(&path);
        let unfinished = is_unfinished_recording(&path);
        if path.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
        cleared.bytes += bytes;
        if unfinished {
            cleared.unfinished_recordings += 1;
        }
    }
    Ok(cleared)
}

/// Removes the downloaded speech models (`ggml-*.bin`) and returns the bytes
/// freed. Other files in the folder, such as a model the user placed there
/// under another name, stay: the app only deletes what it downloaded.
pub fn delete_models(models: &Path) -> std::io::Result<u64> {
    let mut freed = 0;
    let entries = match std::fs::read_dir(models) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_file() && name.starts_with("ggml-") && name.ends_with(".bin") {
            freed += size(&path);
            std::fs::remove_file(&path)?;
        }
    }
    Ok(freed)
}

/// Bytes the downloaded models take.
pub fn models_size(models: &Path) -> u64 {
    std::fs::read_dir(models)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    p.is_file() && name.starts_with("ggml-") && name.ends_with(".bin")
                })
                .map(|p| size(&p))
                .sum()
        })
        .unwrap_or(0)
}

/// Removes the settings files, so every setting reads as its default on the
/// next access. A file that is already gone is fine.
pub fn reset_settings(files: &[PathBuf]) -> std::io::Result<()> {
    for file in files {
        match std::fs::remove_file(file) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("momr-cleanup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sizes_add_up_and_read_well() {
        let dir = scratch("size");
        std::fs::write(dir.join("a"), [0u8; 1500]).unwrap();
        std::fs::create_dir(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/b"), [0u8; 500]).unwrap();
        assert_eq!(size(&dir), 2000);
        assert_eq!(size(&dir.join("missing")), 0);
        assert_eq!(human(0), "0 B");
        assert_eq!(human(999), "999 B");
        assert_eq!(human(2000), "2.0 KB");
        assert_eq!(human(340_000_000), "340 MB");
        assert_eq!(human(1_600_000_000), "1.6 GB");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_clear_keeps_what_it_is_told_and_counts_recordings() {
        let cache = scratch("cache");
        // An unfinished recording, an empty staging folder, an import
        // scratch folder, the socket, and the live recording.
        std::fs::create_dir(cache.join("1700000000")).unwrap();
        std::fs::write(cache.join("1700000000/mic.raw"), [1u8; 4000]).unwrap();
        std::fs::create_dir(cache.join("1700000001")).unwrap();
        std::fs::create_dir(cache.join("import-5")).unwrap();
        std::fs::write(cache.join("import-5/mic.raw"), [0u8; 10]).unwrap();
        std::fs::write(cache.join("momr.sock"), []).unwrap();
        std::fs::create_dir(cache.join("1700000009")).unwrap();
        std::fs::write(cache.join("1700000009/mic.raw"), [1u8; 100]).unwrap();
        let keep = [cache.join("momr.sock"), cache.join("1700000009")];
        let cleared = clear_cache(&cache, &keep).unwrap();
        assert_eq!(cleared.bytes, 4010);
        // The import scratch has audio too; it is a folder ffmpeg was
        // writing, not a meeting, but it counts the same way (it would be
        // offered as a recovery too).
        assert_eq!(cleared.unfinished_recordings, 2);
        assert!(cache.join("momr.sock").exists());
        assert!(cache.join("1700000009/mic.raw").exists());
        assert!(!cache.join("1700000000").exists());
        assert!(!cache.join("1700000001").exists());
        // A cache that is not there is already clear.
        assert_eq!(
            clear_cache(&cache.join("nope"), &[]).unwrap(),
            Cleared::default()
        );
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    fn models_only_lose_what_was_downloaded() {
        let models = scratch("models");
        std::fs::write(models.join("ggml-tiny.bin"), [0u8; 300]).unwrap();
        std::fs::write(models.join("ggml-base.en.bin"), [0u8; 200]).unwrap();
        std::fs::write(models.join("mine.bin"), [0u8; 100]).unwrap();
        assert_eq!(models_size(&models), 500);
        assert_eq!(delete_models(&models).unwrap(), 500);
        assert!(models.join("mine.bin").exists());
        assert!(!models.join("ggml-tiny.bin").exists());
        assert_eq!(delete_models(&models.join("nope")).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&models);
    }

    #[test]
    fn settings_reset_tolerates_missing_files() {
        let dir = scratch("settings");
        let settings = dir.join("settings.json");
        let config = dir.join("config.toml");
        std::fs::write(&settings, "{}").unwrap();
        reset_settings(&[settings.clone(), config.clone()]).unwrap();
        assert!(!settings.exists());
        reset_settings(&[settings, config]).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
