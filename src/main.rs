//! MOM Recorder: records a meeting in two tracks (mic and computer
//! audio), transcribes it with whisper.cpp after the call, and streams live
//! levels to a menu bar item or any other client.

mod agent;
mod animation;
mod audio;
mod chapters;
mod diarize;
mod export;
mod ipc;
mod meeting;
mod models;
mod nemotron;
mod player;
mod settings;
mod theme;
mod transcribe;
mod ui;

use gtk::glib;

pub const APP_ID: &str = "io.github.riobahtiar.MOMRecorder";
pub const APP_NAME: &str = "momr";

fn main() -> glib::ExitCode {
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
