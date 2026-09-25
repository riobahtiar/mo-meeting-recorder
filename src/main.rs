//! MOM Recorder: records a meeting in two tracks (mic and computer
//! audio), transcribes it after the call with whisper.cpp on this Mac (or,
//! on explicit opt-in, a cloud provider), and streams live levels to a menu
//! bar item or any other client.
//!
//! The socket commands (`start`, `stop`, `pause`, `compact`, `watch`) only
//! talk to a running app, so they skip the environment set-up: SwiftBar and
//! keybindings run them without `TERM`, where the login-shell probe would
//! cost every click up to three seconds.

mod animation;
mod player;
mod theme;
mod ui;

use gtk::glib;

pub const APP_ID: &str = "io.github.riobahtiar.MOMRecorder";
pub use momr_platform::APP_NAME;
fn main() -> glib::ExitCode {
    momr_core::locales::init_lang(momr_core::settings::current_locale());

    let command = std::env::args().nth(1);
    if !matches!(
        command.as_deref(),
        Some("start" | "stop" | "compact" | "pause" | "watch")
    ) {
        extend_path(login_shell_path);
        bundle_environment();
    }
    match command.as_deref() {
        None => ui::run(None),
        Some("watch") => {
            momr_core::ipc::watch();
            glib::ExitCode::SUCCESS
        }
        Some(command @ ("start" | "stop" | "compact" | "pause")) => {
            if momr_core::ipc::send(command) {
                glib::ExitCode::SUCCESS
            } else {
                eprintln!("{APP_NAME}: {}", momr_core::locales::t("cli.not_running"));
                glib::ExitCode::FAILURE
            }
        }
        Some("transcribe-file") => exit_code(momr_core::transcribe::cli_file(
            &std::env::args().skip(2).collect::<Vec<_>>(),
        )),
        Some("diarize") => exit_code(momr_core::diarize::cli(
            &std::env::args().skip(2).collect::<Vec<_>>(),
        )),
        Some("transcribe") => exit_code(momr_core::transcribe::cli(
            &std::env::args().skip(2).collect::<Vec<_>>(),
        )),
        Some("finish") => exit_code(momr_core::finish::cli(
            &std::env::args().skip(2).collect::<Vec<_>>(),
        )),
        Some("ask") => exit_code(momr_core::agent::cli(
            &std::env::args().skip(2).collect::<Vec<_>>(),
        )),
        Some("-h" | "--help") => {
            println!(
                "{}",
                momr_core::locales::t("cli.usage").replace("{}", APP_NAME)
            );
            println!();
            println!("  {}", momr_core::locales::t("cli.no_command"));
            println!("  {}", momr_core::locales::t("cli.meeting"));
            println!("  {}", momr_core::locales::t("cli.start"));
            println!("  {}", momr_core::locales::t("cli.stop"));
            println!("  {}", momr_core::locales::t("cli.compact"));
            println!("  {}", momr_core::locales::t("cli.pause"));
            println!("  {}", momr_core::locales::t("cli.watch"));
            println!("  {}", momr_core::locales::t("cli.transcribe"));
            println!("  {}", momr_core::locales::t("cli.transcribe_file"));
            println!("  {}", momr_core::locales::t("cli.finish"));
            println!("  {}", momr_core::locales::t("cli.diarize_help"));
            println!("  {}", momr_core::locales::t("cli.ask"));
            glib::ExitCode::SUCCESS
        }
        Some(path)
            if path.ends_with(&format!(".{}", momr_core::meeting::EXTENSION))
                || std::path::Path::new(path).is_dir() =>
        {
            ui::run(Some(path))
        }
        Some(other) => {
            eprintln!(
                "{APP_NAME}: {}",
                momr_core::locales::t("cli.unknown").replace("{}", other)
            );
            glib::ExitCode::from(2)
        }
    }
}

/// The core reports command-line exits as plain integers; GTK wants its own
/// code type. Only 0, 1 and 2 ever cross here.
fn exit_code(code: i32) -> glib::ExitCode {
    glib::ExitCode::from(code as u8)
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
    // SAFETY: first thing in main after extend_path, which leaves no thread
    // behind; GTK has not started any yet.
    unsafe {
        std::env::set_var("XDG_DATA_DIRS", &share);
        std::env::set_var("GSETTINGS_SCHEMA_DIR", &schemas);
    }
}

/// A Finder or Spotlight launch brings only `/usr/bin:/bin:/usr/sbin:/sbin`,
/// without Homebrew, the helper's neighbours or the user's tool bins. Prepend
/// them before GTK starts any thread, so `ffmpeg`, `momr-audio` and the agents
/// resolve the same way they do from a terminal. `shell_path` asks the login
/// shell for its PATH; tests pass their own.
fn extend_path(shell_path: impl FnOnce() -> Option<String>) {
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
    if let Some(shell_path) = shell_path() {
        extra.extend(std::env::split_paths(&shell_path));
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
        // SAFETY: first thing in main; `login_shell_path` has reaped its
        // child and started no thread, so nothing reads the environment
        // concurrently. (set_var is unsafe in edition 2024.)
        unsafe { std::env::set_var("PATH", path) };
    }
}

/// The login shell's PATH, for a GUI launch that has no terminal (no `TERM`)
/// and so none of the user's rc files. Polled on this thread with a short
/// timeout, then killed, so a slow rc file cannot hang the launch and no
/// thread is left running when `extend_path` changes the environment.
fn login_shell_path() -> Option<String> {
    use std::io::Read;
    if std::env::var_os("TERM").is_some() {
        return None;
    }
    let shell = std::env::var("SHELL").ok()?;
    let mut child = std::process::Command::new(&shell)
        .args(["-lc", "printf %s \"$PATH\""])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            _ => break None,
        }
    };
    if !status.is_some_and(|s| s.success()) {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    let mut path = String::new();
    child.stdout.take()?.read_to_string(&mut path).ok()?;
    Some(path)
}

/// One lock for every test that changes the process environment: `set_var`
/// racing a `getenv` elsewhere (GLib's included) is undefined behaviour, so
/// such tests must not run alongside each other.
#[cfg(test)]
pub fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn extend_path_puts_the_exe_dir_first_and_keeps_the_original() {
        let _guard = env_lock();
        let saved = std::env::var_os("PATH");
        unsafe { std::env::set_var("PATH", "/usr/bin:/bin:/usr/bin") };
        // No login shell in tests: its answer is passed in.
        extend_path(|| Some("/usr/sbin:/bin".into()));
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
        assert!(rest.contains(&PathBuf::from("/usr/sbin")));
        assert_eq!(rest.iter().filter(|p| p.as_os_str() == "/bin").count(), 1);
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
