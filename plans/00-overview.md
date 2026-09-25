# 00 Overview

## Goal

MOM Recorder is a macOS app that records the microphone and the computer audio as two tracks, transcribes locally with whisper.cpp, tells speakers apart, plays back, and opens `.meeting-recorder` files from Finder. It looks and behaves the way Mac apps do. It grew out of [jankeesvw/omarchy-meeting-recorder](https://github.com/jankeesvw/omarchy-meeting-recorder), a Linux app for the Omarchy distribution; the Rust core is kept, everything Omarchy is removed, and macOS takes the place of Linux at every seam.

## Scope

- **macOS 14 (Sonoma) and newer**, Apple silicon and Intel. The Core Audio process tap API for capturing computer audio without a virtual device needs 14.2.
- **macOS only.** Linux code paths are removed as their macOS replacement lands; `cfg(target_os)` exists only in `crates/momr-platform` (D25), for targets that are scheduled. Upstream is the origin, not a merge partner: a fix from upstream is cherry-picked when it still applies to the shared core (transcription, diarization, export, transcript editing).
- **Not iOS or iPadOS.** GTK 4 and libadwaita have no port there.
- **Not a rewrite.** The Rust code stays; platform work happens at the seams listed below.
- **Meetings stay compatible with upstream.** The folder layout, the `.meeting-recorder` manifest and `transcript.md` are unchanged, so a meeting recorded with the Linux app opens here.

## Done means

1. `cargo build --release` passes on macOS with no warnings; `cargo test` is green.
2. On the ready page both meters move: the microphone one when you speak, the computer one when the Mac plays sound.
3. A recording produces the upstream folder layout (`audio.ogg` or `mic.ogg` plus `computer.ogg`, `transcript.md`, `<name>.meeting-recorder`, `.tracks/`), and a folder recorded with upstream on Linux opens on the done page.
4. Playback and seeking work; the compact strip toggles; an imported mp3 with two voices gets two speakers.
5. The app has a native menu bar with standard menus, window buttons on the left, the system font, follows light, dark and accent, opens native file panels, and has About and Preferences in the app menu.
6. `MOM Recorder.app` in a DMG installs without Homebrew, passes Gatekeeper, asks for the microphone with the app's own text, and opens `.meeting-recorder` files on double-click.
7. No Omarchy name, path, command or integration remains in the repository (`grep -ri omarchy` finds only this sentence's history and the upstream credit).

## Architecture today

```
main.rs ── argv ──► ui.rs (window, pages, recording state machine)
                       │
   ┌───────────┬───────┼──────────┬───────────┬──────────────┐
audio.rs    player.rs  export.rs  transcribe.rs  agent.rs    ipc.rs
parec ──►   ffmpeg│pacat  ffmpeg   ffmpeg decode  omarchy-     unix socket
raw s16le   playback   opus       whisper-rs     default-agent  NDJSON
                                  diarize.rs     setsid/timeout  watch
                                  nemotron.rs (ort)
theme.rs (Omarchy colors.toml)   settings.rs / models.rs (XDG paths)
bar_widget.rs (Omarchy bar)      meeting.rs / chapters.rs (formats)
```

Every operating-system dependency is a child process or a path. That is what makes the port tractable: the contracts between `ui.rs` and the leaves are byte streams and files.

## Architecture on macOS

The Rust code is a workspace (plan 12, D25): `crates/momr-core` is the engine without a window, `crates/momr-platform` the OS seams, `src/` the GTK shell. `apps/momr-appkit` is the native shell that is to replace GTK (D24); it runs the `momr` binary for the engine.

```
MOM Recorder.app
├── Contents/MacOS/momr                       the Rust binary, GTK 4 + libadwaita
├── Contents/MacOS/momr-audio                 Swift helper: mic and process-tap capture, raw s16le on stdout,
│                                             plus `run`, a die-with-parent wrapper for ffmpeg
├── Contents/MacOS/momr-menubar               Swift status item reading `momr watch` (plan 09)
├── Contents/MacOS/ffmpeg, ffprobe            bundled
├── Contents/Frameworks/*.dylib               GTK, libadwaita, GLib, Pango, Cairo, …
├── Contents/Resources/                       icons, GLib schemas, app icon, macos.css
└── Contents/Info.plist                       document type, usage descriptions
```

## Porting map

One row per seam. "Remove" means the Linux code is deleted when the macOS replacement lands. The plan column says where the work is specified.

| Seam | Where | Today (Linux) | macOS | Plan |
|---|---|---|---|---|
| Microphone capture | `audio.rs` `capture()` | `parec -d @DEFAULT_SOURCE@`, raw s16le 48 kHz stereo on stdout | `ffmpeg -f avfoundation -i ":default"` first; then `momr-audio mic`, which follows default-device changes. Remove `parec`. | 03 |
| Computer audio capture | `audio.rs` `capture()` | `parec -d @DEFAULT_MONITOR@` | `momr-audio system`: Core Audio process tap on all processes, resampled to 48 kHz; BlackHole loopback device as fallback | 03 |
| Children die with the app | `crates/momr-core/src/playback.rs` `guarded()` | `prctl(PR_SET_PDEATHSIG)` | `momr-audio run -- <cmd>`: execs the command and kills it when the parent exits (kqueue on the parent pid). Remove `prctl`. | 02, 04 |
| Playback | `crates/momr-core/src/playback.rs` `Playback::start()` | `ffmpeg` piped into `pacat` | `ffmpeg … -f audiotoolbox -`, one process. Remove `pacat`. | 04 |
| Compact strip | `ui.rs` `set_compact()`, `hyprctl_*()` | `hyprctl dispatch` resize and move | `gtk::Window::set_default_size`. Remove `hyprctl_*`. | 04 |
| Keyboard | `ui.rs` `set_accels_for_action` | `<Control>m/w/q` | `<Primary>`; compact on ⇧⌘M because ⌘M minimizes | 04, 07 |
| Chapters agent | `agent.rs` `status()` | `omarchy-default-agent` prints the id | `agent = "…"` in `config.toml`. Remove the Omarchy lookup and its message. | 05 |
| Agent process group and timeout | `agent.rs` `run()` | `setsid sh -c 'ulimit -f … && exec timeout …'` | `momr_platform::process::spawn_detached` (its own session), `ulimit -f` through `sh`, in-process timeout; `gtimeout` used when present | 05 |
| Bounded reads | `agent.rs` `read_bounded()` | Linux literals for `O_NOFOLLOW`, `O_NONBLOCK` | `momr_platform::fs::open_no_follow` with the macOS values | 02 |
| Directories | `settings.rs`, `models.rs`, `transcribe.rs` | Hand-rolled `XDG_*` with `~/.local/…` fallbacks | `crates/momr-platform/src/paths.rs` builds `~/Library/Application Support/momr` and `~/Library/Caches/momr` from the home directory (Homebrew's GLib has no Cocoa support, D22), an absolute `XDG_*` still winning | 06 |
| PATH for GUI launches | `main.rs` | inherited from the session | Finder launches with `/usr/bin:/bin:…`; prepend Homebrew and user bin dirs before GTK starts | 06 |
| Meetings folder | `meeting.rs` `folder_for()` | `~/Documents/Meetings` | Same, `home_dir()/Documents/Meetings` (`paths::meetings`) unless Settings moved it | 06 |
| Live state socket | `ipc.rs` | Unix socket in `$XDG_RUNTIME_DIR` | Same protocol through `momr_platform::sock`; `~/Library/Caches/momr/momr.sock` (under an absolute `$XDG_CACHE_HOME` when set), falling back to `$TMPDIR/momr.sock` past the 104-byte `sun_path` limit | 06 |
| Theme | `theme.rs` | Reads Omarchy's `colors.toml`, followed live | libadwaita follows system appearance and accent; speaker and wave colours from Apple's system palette. Remove the `colors.toml` reader. | 07 |
| Menu bar, chrome, controls | `ui.rs` | libadwaita defaults | Native `GMenuModel` menubar, window buttons left, `macos.css`, About and Preferences dialogs | 07 |
| File panels | `ui.rs` `gtk::FileDialog` | GTK dialog | Native `NSOpenPanel` through GTK's quartz file chooser; nothing to change, verify filters | 07 |
| Bar widget | `bar_widget.rs`, `ui.rs` `offer_bar_widget()`, `settings.rs` `bar_widget_offered` | Offers to link an Omarchy bar plugin | Remove. A menu bar item reads `watch` instead. | 02, 09 |
| Whisper GPU | `Cargo.toml` features | `vulkan` | `metal` feature. Remove `vulkan`. | 02 |
| ONNX Runtime | `nemotron.rs` | `ort` with `download-binaries` | Same; prebuilt Apple binaries download at build time | 02 |
| Opening `.meeting-recorder` files | `main.rs` argv | freedesktop MIME type (files already removed) | `Info.plist` document type and UTI; `GApplication` `open` signal, because Finder does not pass argv | 08 |
| Install | (pacman packaging removed) | | Homebrew formula, then signed `.app` in a DMG | 08 |
| Identity | `main.rs` `APP_ID`, `momr-platform` `APP_NAME`; `Cargo.toml` | done: `momr`, `io.github.riobahtiar.MOMRecorder` | Confirm the bundle id before the first DMG; sweep the remaining comments | 11 |

## Phases

1. **Works** (plans 02 to 06; 02 and 06 are in `archives/`): a developer builds it and records a meeting from a terminal.
2. **Feels native** (07, 09): a Mac user does not notice it is GTK from the chrome, menus, shortcuts, fonts or colours.
3. **Ships** (08, 10, 11): DMG and Homebrew, CI, identity confirmed.
4. **Native** (12, 16): entered 2026-09-25 (D24) — an AppKit shell on the Rust core, GTK retiring at parity; a core crate and a cross-platform shell when Windows or Linux is scheduled.
5. **Providers** (13) and **Features** (14, 15): cloud transcription, the Indonesian interface, and what the first display session asked for.

## Risks

| Risk | Effect | Mitigation |
|---|---|---|
| Process tap API differences across 14.2 to 27 | System capture fails on some versions | Test on the oldest supported macOS in a VM; BlackHole fallback; helper reports a clear exit code the app turns into a banner |
| TCC permission prompts land on the wrong process | Mic silently empty when launched from a terminal | Document the responsible-process rule; bundle early (plan 08) so the app owns its permissions |
| GTK's macOS renderer glitches | Blank or flickering window | `GSK_RENDERER=cairo` as a documented fallback; pin GTK version in the formula |
| libadwaita CSS drift | `macos.css` breaks on a libadwaita update | Keep the layer small, scoped to a `.macos` class, covered by a screenshot check |
| Bundling GTK dylibs and resources | App works with Homebrew, fails on a clean Mac | Test every DMG on a clean user account or VM without Homebrew |
| Universal binaries | Homebrew dylibs are per architecture | Ship two DMGs (arm64, x86_64) |
| Upstream fixes | Diverging code makes cherry-picks harder over time | Keep the shared core modules (`transcribe`, `diarize`, `nemotron`, `export`, `meeting`, `chapters`) close to upstream; put macOS work in the seams |
