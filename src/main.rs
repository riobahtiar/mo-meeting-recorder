//! MOM Recorder: records a meeting in two tracks (mic and computer
//! audio), transcribes it with whisper.cpp after the call, and streams live
//! levels to a menu bar item or any other client.

mod agent;
mod animation;
mod audio;
mod chapters;
mod diarize;
mod export;
mod helper;
mod ipc;
mod meeting;
mod models;
mod nemotron;
mod paths;
mod player;
mod settings;
mod theme;
mod transcribe;
mod ui;

use gtk::glib;

pub const APP_ID: &str = "io.github.riobahtiar.MOMRecorder";
pub const APP_NAME: &str = "momr";
fn main() -> glib::ExitCode {
    extend_path();
    bundle_environment();
    match std::env::args().nth(1).as_deref() {
        None => ui::run(None),
        Some("watch") => {
            ipc::watch();
            glib::ExitCode::SUCCESS
        }
        Some(command @ ("start" | "stop" | "compact" | "pause")) => {
            if ipc::send(command) {
                glib::ExitCode::SUCCESS
            } else {
                eprintln!("{APP_NAME}: the recorder is not running");
                glib::ExitCode::FAILURE
            }
        }
        Some("transcribe-file") => {
            transcribe::cli_file(&std::env::args().skip(2).collect::<Vec<_>>())
        }
        Some("diarize") => diarize::cli(&std::env::args().skip(2).collect::<Vec<_>>()),
        Some("transcribe") => transcribe::cli(&std::env::args().skip(2).collect::<Vec<_>>()),
        Some("ask") => agent::cli(&std::env::args().skip(2).collect::<Vec<_>>()),
        Some("-h" | "--help") => {
            println!(
                "Usage: {APP_NAME} [start | stop | pause | compact | watch | transcribe <mic> <computer> [--language xx]]"
            );
            println!();
            println!("  (no command)  open the recorder, ready to record");
            println!("  <meeting>     open a .meeting-recorder file or a meeting folder");
            println!("  start         start recording in the open window (for a keybinding)");
            println!("  stop          stop the running recording (for a keybinding)");
            println!(
                "  watch         stream the recorder state as NDJSON, for a menu bar item or any other client"
            );
            println!("  transcribe    transcribe two tracks and print the transcript as Markdown");
            println!(
                "  ask           run a prompt over stdin through the default agent, without tools"
            );
            glib::ExitCode::SUCCESS
        }
        Some(path)
            if path.ends_with(&format!(".{}", meeting::EXTENSION))
                || std::path::Path::new(path).is_dir() =>
        {
            ui::run(Some(path))
        }
        Some(other) => {
            eprintln!("{APP_NAME}: unknown command '{other}', see --help");
            glib::ExitCode::from(2)
        }
    }
}

/// Inside `MOM Recorder.app`, point GTK at the bundled resources instead of
/// Homebrew's prefix: schemas, icons and data dirs. No launcher script, so
/// signing and the TCC identity stay simple. A no-op outside the bundle.
fn bundle_environment() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(contents) = exe.parent().and_then(|p| p.parent()) else {
        return;
    };
    if contents.file_name().is_none_or(|name| name != "Contents") {
        return;
    }
    let share = contents.join("Resources/share");
    let schemas = share.join("glib-2.0/schemas");
    // SAFETY: first thing in main, single-threaded (see extend_path).
    unsafe {
        std::env::set_var("XDG_DATA_DIRS", &share);
        std::env::set_var("GSETTINGS_SCHEMA_DIR", &schemas);
    }
}

/// A Finder or Spotlight launch brings only `/usr/bin:/bin:/usr/sbin:/sbin`,
/// without Homebrew, the helper's neighbours or the user's tool bins. Prepend
/// them before GTK starts any thread, so `ffmpeg`, `momr-audio` and the agents
/// resolve the same way they do from a terminal.
fn extend_path() {
    use std::path::PathBuf;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut extra: Vec<PathBuf> = vec!["/opt/homebrew/bin".into(), "/usr/local/bin".into()];
    if let Some(home) = home {
        for dir in [
            ".local/bin",
            ".cargo/bin",
            ".npm-global/bin",
            ".bun/bin",
            ".volta/bin",
        ] {
            extra.push(home.join(dir));
        }
    }
    // The helper next to this executable first.
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        extra.insert(0, dir.to_path_buf());
    }
    // A GUI launch has no login shell; ask it once for its PATH, best effort
    // with a short timeout so a slow rc file cannot hang the launch.
    if std::env::var_os("TERM").is_none()
        && let Ok(shell) = std::env::var("SHELL")
    {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let output = std::process::Command::new(&shell)
                .args(["-lc", "printf %s \"$PATH\""])
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output();
            let _ = tx.send(output);
        });
        if let Ok(Ok(output)) = rx.recv_timeout(std::time::Duration::from_secs(3))
            && output.status.success()
        {
            let shell_path = String::from_utf8_lossy(&output.stdout);
            extra.extend(std::env::split_paths(&shell_path.into_owned()));
        }
    }
    let current = std::env::var_os("PATH").unwrap_or_default();
    let keep: Vec<PathBuf> = std::env::split_paths(&current).collect();
    let mut seen = std::collections::HashSet::new();
    let joined: Vec<PathBuf> = extra
        .into_iter()
        .chain(keep)
        .filter(|p| p.is_dir() && seen.insert(p.clone()))
        .collect();
    if let Ok(path) = std::env::join_paths(joined) {
        // SAFETY: first thing in main, before any other thread exists; the
        // environment is not read concurrently. (set_var is unsafe in edition
        // 2024.)
        unsafe { std::env::set_var("PATH", path) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::{LazyLock, Mutex, MutexGuard};

    /// `PATH` is process-global, so serialise with the other env-touching tests.
    static LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[test]
    fn extend_path_puts_the_exe_dir_first_and_keeps_the_original() {
        let _guard: MutexGuard<'static, ()> = LOCK.lock().unwrap();
        let saved = std::env::var_os("PATH");
        unsafe { std::env::set_var("PATH", "/usr/bin:/bin:/usr/bin") };
        extend_path();
        let path = std::env::var_os("PATH").unwrap();
        let mut entries = std::env::split_paths(&path);
        let exe_dir = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        assert_eq!(entries.next().unwrap(), exe_dir);
        let rest: Vec<PathBuf> = entries.collect();
        assert!(rest.contains(&PathBuf::from("/usr/bin")));
        assert!(rest.contains(&PathBuf::from("/bin")));
        assert_eq!(
            rest.iter().filter(|p| p.as_os_str() == "/usr/bin").count(),
            1
        );
        match saved {
            Some(path) => unsafe { std::env::set_var("PATH", path) },
            None => unsafe { std::env::remove_var("PATH") },
        }
    }
}
