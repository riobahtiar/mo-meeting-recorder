# AGENTS.md

**MOM Recorder** (short **MOMR**, binary `momr`) is a macOS app in Rust with GTK 4 and libadwaita: two-track meeting recording, local transcription with whisper.cpp, speaker separation, playback. It is based on [Meeting Recorder](https://github.com/jankeesvw/omarchy-meeting-recorder) by Jankees van Woezik, a Linux app; the job of this repository is to **replace every Linux integration with its macOS counterpart and make the app feel like a Mac app**. macOS is the only target.

## Where things are

- **The plan is `plans/`.** Start at [`plans/README.md`](plans/README.md): it says how to work a plan and which one is next. [`plans/00-overview.md`](plans/00-overview.md) has the goal, the target architecture, the porting map (every Linux seam, its file and function, and its macOS replacement) and what "done" means. [`plans/01-decisions.md`](plans/01-decisions.md) has every decision already taken; implement those as written and add an entry before departing from one. Finished plans live in [`plans/archives/`](plans/archives/README.md), whose index says what each delivered; [`plans/16-multi-platform-architecture.md`](plans/16-multi-platform-architecture.md) is the reference for a Windows or Linux version.
- **Before editing a file the porting map names, read the plan its row points to.** When you finish a step, tick it in the plan, update the board in `plans/README.md`, the status table in `README.md` and `CHANGELOG.md` (one short line per feature added or retired), in the same commit as the code. A plan with every box ticked moves to `plans/archives/`.
- Every `src/*.rs` opens with a module doc that says what the module does and why it is built that way. Read it before editing; it is the source of truth for the module. `src/ui.rs` is the window and state machine; every other module is a leaf it calls, and platform work lands in the leaves.
- **Fixtures, screenshots and clips use invented meetings only**, never a real one. `demo/script.txt` and `demo/import-script.txt` are the invented meetings; `say -o x.aiff "text"` voices new ones.
- Upstream's README and demo kit, for how the original behaved: `git show 1a352b3:README.md`.

## Rules

- **Linux code goes when its macOS replacement lands.** No `cfg(target_os)` gating, no dead Linux paths. The plan for each seam says what replaces what. Until a seam is replaced, leave its Linux code and comments as they are, so the code and its docs keep agreeing.
- **On-disk and wire formats are interfaces.** The meeting folder layout, the `.meeting-recorder` JSON, `transcript.md`, `.tracks/`, the NDJSON `watch` lines and the socket commands are shared with upstream and read by users' scripts. Change one only with a reader for the old shape and a README note.
- **The agent boundary is "no tools", enforced by the agent's own switch.** A new agent in `agent.rs` gets a single documented switch that turns off every tool, or it is refused, as the module doc lays out. Extend the tests there with every agent you add.
- **Dependencies stay few.** `ureq`, `std::process::Command`, hand-parsed `serde_json`, no async runtime. Add a crate only when std or GLib cannot do the job, and say why in `Cargo.toml` the way the GPU feature is annotated. Apple APIs go in the Swift helper (`helpers/momr-audio`), not in Rust bindings.
- **Comments say why, in full sentences,** in the voice of the module docs. Tests live in `#[cfg(test)] mod tests` in the module; code that runs external commands takes them as closures so tests can fake them.
- **Names.** "MOM Recorder" in anything a user reads; "MOMR" where space is short; `momr` for the binary, helpers (`momr-audio`, `momr-menubar`), folders and the socket. The app id is `io.github.riobahtiar.MOMRecorder` until plan 11 confirms it.
- **Report what you verified.** `cargo test` covers the logic; the audio seams are only proven by running the app and watching the meters. Say which of these you did.

## Build and check

```bash
xcode-select --install
brew install gtk4 libadwaita adwaita-icon-theme cmake pkgconf ffmpeg
cargo build --release
cargo test
cargo fmt --check && cargo clippy --all-targets -- -D warnings
cargo run --release --example transcribe_animation     # the animation alone, no audio needed
```

- CMake and the Xcode tools compile whisper.cpp; `ort` downloads ONNX Runtime during the build, so the first build needs the network.
- `transcribe-file` needs no window or device: `target/release/momr transcribe-file some.mp3 --model tiny` is the first thing to try on a fresh build.
- A binary started from a terminal uses the terminal's microphone permission; a flat mic meter means System Settings › Privacy & Security › Microphone. A process tap asks under System Audio Recording.
- Blank or flickering window: try `GSK_RENDERER=cairo`. Missing symbolic icons: `adwaita-icon-theme` is not installed.
