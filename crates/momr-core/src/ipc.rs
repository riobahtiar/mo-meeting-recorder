//! Live state for a menu bar item or any other client.
//!
//! The app listens on a Unix socket, `~/Library/Caches/momr/momr.sock` (or
//! `$TMPDIR/momr.sock` when that path is too long for a socket), and writes one JSON
//! line per tick to every connected client: 20 times a second while recording,
//! once a second otherwise. `momr watch` connects to it and
//! copies those lines to stdout, printing `{"state":"off"}` while the app is not
//! running, so a menu bar item only has to read NDJSON from a process.
//!
//! A line looks like:
//! {"state":"recording","elapsed":754,"title":"Weekly","mic":0.62,"computer":0.31,"progress":0.0}
//! with `mic` and `computer` as meter levels from 0 to 1, and `progress` the
//! transcription progress from 0 to 1 while the state is "transcribing".

use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::audio::to_meter;
use momr_platform::APP_NAME;
use momr_platform::sock::{self, Stream};
const MAX_LINE: usize = 4096;

#[derive(Clone, Default)]
pub struct Status {
    /// idle, recording, paused, stopping, transcribing or done
    pub state: &'static str,
    pub started_at: i64,
    /// Seconds spent paused so far, and when the current pause began (0: not paused).
    pub paused_secs: i64,
    pub pause_began: i64,
    pub title: String,
    pub progress: f64,
}

pub type SharedStatus = Arc<Mutex<Status>>;

/// Where the app listens. `momr-menubar` gets it as `MOMR_SOCKET`, so the
/// two can never disagree about the rule below.
pub fn socket_path() -> PathBuf {
    socket_path_in(&momr_platform::paths::cache())
}

/// `momr.sock` under `base`, or directly under the temp dir when that would
/// overflow macOS's 104-byte `sun_path` limit (long user names). `$TMPDIR` is
/// a private per-user directory, so the fallback keeps its privacy.
fn socket_path_in(base: &Path) -> PathBuf {
    let path = base.join(format!("{APP_NAME}.sock"));
    if path.as_os_str().len() > 100 {
        std::env::temp_dir().join(format!("{APP_NAME}.sock"))
    } else {
        path
    }
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Commands a client may send, one per line.
pub const COMMANDS: [&str; 4] = ["start", "stop", "compact", "pause"];

/// Starts the socket server at `path`. Called once, from the primary
/// instance. Clients get the state lines; a line a client writes that names
/// one of `COMMANDS` is passed on to `commands`. `peaks` gives the latest
/// (mic, computer) peaks. An error means no menu bar item or `momr stop`
/// can reach this app, which the caller tells the user.
pub fn serve(
    path: impl AsRef<Path>,
    status: SharedStatus,
    peaks: impl Fn() -> (f32, f32) + Send + 'static,
    commands: async_channel::Sender<&'static str>,
) -> Result<(), String> {
    let path = path.as_ref();
    if let Some(dir) = path.parent() {
        // First run: ~/Library/Caches/momr does not exist yet.
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    // A socket file left behind by a crash refuses new binds; nobody answers on it.
    if sock::connect(path).is_err() {
        let _ = std::fs::remove_file(path);
    }
    let listener = sock::bind(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let clients: Arc<Mutex<Vec<Stream>>> = Arc::default();

    let accepted = clients.clone();
    thread::spawn(move || {
        for stream in std::iter::from_fn(|| listener.accept().ok()) {
            // Best effort: macOS refuses SO_SNDTIMEO once the peer has
            // already gone, which fire-and-forget clients (`momr start`)
            // always have. Dropping the connection then would lose the
            // command, so a client without a timeout still gets its state
            // lines, and one that stops reading is dropped on the next
            // failed write instead.
            let _ = stream.set_write_timeout(Some(Duration::from_millis(20)));
            if let Ok(reader) = stream.try_clone() {
                let commands = commands.clone();
                thread::spawn(move || read_commands(reader, &commands));
            }
            accepted.lock().unwrap().push(stream);
        }
    });

    thread::spawn(move || {
        loop {
            let snapshot = status.lock().unwrap().clone();
            let recording = snapshot.state == "recording";
            let taking = recording || snapshot.state == "paused";
            let until = if snapshot.pause_began > 0 {
                snapshot.pause_began
            } else {
                now()
            };
            let busy = recording || snapshot.state == "transcribing";
            let (mic, computer) = peaks();
            let line = serde_json::json!({
                "state": snapshot.state,
                "elapsed": if taking { (until - snapshot.started_at - snapshot.paused_secs).max(0) } else { 0 },
                "title": snapshot.title,
                "mic": round(to_meter(mic)),
                "computer": round(to_meter(computer)),
                "progress": round(snapshot.progress),
            })
            .to_string()
                + "\n";
            // A client that cannot keep up is dropped rather than waited for.
            clients
                .lock()
                .unwrap()
                .retain_mut(|client| match client.write_all(line.as_bytes()) {
                    Ok(()) => true,
                    Err(e) => e.kind() == ErrorKind::Interrupted,
                });
            thread::sleep(Duration::from_millis(if recording {
                50
            } else if busy {
                250
            } else {
                1000
            }));
        }
    });
    Ok(())
}

fn read_commands(stream: Stream, commands: &async_channel::Sender<&'static str>) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.by_ref().take(MAX_LINE as u64).read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {
                if let Some(command) = COMMANDS.iter().find(|c| **c == line.trim()) {
                    let _ = commands.send_blocking(command);
                }
            }
        }
    }
}

/// `momr stop`: ask the running app to stop recording.
pub fn send(command: &str) -> bool {
    match sock::connect(socket_path()) {
        Ok(mut stream) => stream.write_all(format!("{command}\n").as_bytes()).is_ok(),
        Err(_) => false,
    }
}

fn round(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// `momr watch`: relay the app's state lines to stdout.
pub fn watch() {
    let mut stdout = std::io::stdout();
    loop {
        if let Ok(stream) = sock::connect(socket_path()) {
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.by_ref().take(MAX_LINE as u64).read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) if !line.ends_with('\n') => break, // over-long line: not ours
                    Ok(_) => {
                        if stdout
                            .write_all(line.as_bytes())
                            .and_then(|_| stdout.flush())
                            .is_err()
                        {
                            return; // the client went away
                        }
                    }
                }
            }
        }
        if writeln!(stdout, r#"{{"state":"off"}}"#)
            .and_then(|_| stdout.flush())
            .is_err()
        {
            return;
        }
        thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_base_keeps_the_socket_next_to_the_cache() {
        let base = Path::new("/Users/someone/Library/Caches/momr");
        assert_eq!(socket_path_in(base), base.join(format!("{APP_NAME}.sock")));
    }

    /// `momr start` connects, writes one line and hangs up at once; the
    /// command must still arrive, and the state lines must flow to a reader.
    /// Unix-only: elsewhere the socket constructors report unsupported.
    #[cfg(unix)]
    #[test]
    fn fire_and_forget_commands_arrive() {
        let dir = std::env::temp_dir().join(format!("momr-ipc-{}", std::process::id()));
        let path = dir.join("t.sock");
        let (tx, rx) = async_channel::unbounded();
        let status: SharedStatus = Arc::new(Mutex::new(Status {
            state: "idle",
            ..Default::default()
        }));
        serve(&path, status, || (0.5, 0.0), tx).expect("listens");
        let mut client = sock::connect(&path).expect("connects");
        client.write_all(b"stop\nnot-a-command\n").unwrap();
        drop(client);
        let command = rx.recv_blocking().expect("the command reaches the app");
        assert_eq!(command, "stop");
        let reader = sock::connect(&path).unwrap();
        let mut line = String::new();
        BufReader::new(reader).read_line(&mut line).unwrap();
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["state"], "idle");
        assert!(value["mic"].as_f64().unwrap() > 0.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn long_base_falls_back_to_the_temp_dir() {
        let base = Path::new("/Users/").join("a".repeat(200));
        let path = socket_path_in(&base);
        assert!(path.as_os_str().len() <= 100, "{}", path.display());
        assert_eq!(
            path.file_name().unwrap(),
            format!("{APP_NAME}.sock").as_str()
        );
    }
}
