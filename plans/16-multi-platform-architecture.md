# 16 Multi-platform architecture

## Goal

Know how to ship MOM Recorder on Windows, and later Linux, without a second codebase and without slowing the macOS app down: which parts are already portable, which seams each platform needs, and which user-interface layer to build on. This plan is a reference, like plan 12; it is entered when the Windows version is scheduled.

## Where the code stands

Every module that does not draw is already platform-neutral in spirit: `transcribe.rs`, `diarize.rs`, `nemotron.rs`, `export.rs`, `meeting.rs`, `chapters.rs`, `agent.rs`, `models.rs`, `provider.rs`, `ipc.rs`, `settings.rs`, `cleanup.rs`, `timer.rs`. They touch the platform through a few seams, and each seam is a child process or a path (D03), which is what makes a port a matter of swapping the leaves. The GTK-specific modules are `ui.rs`, `animation.rs` and `player.rs`'s drawing; `theme.rs` supplies colours; `paths.rs` and `helper.rs` know where things are on a Mac.

## Seams per platform

| Seam | macOS (today) | Windows | Linux |
|---|---|---|---|
| Microphone | `momr-audio mic` (AVAudioEngine) | WASAPI capture (`wasapi` crate, or `cpal`) in a `momr-audio.exe` | PipeWire/PulseAudio (`parec`, as upstream) |
| Computer audio | Core Audio process tap | WASAPI loopback; per-app through `ActivateAudioInterfaceAsync` with `AUDIOCLIENT_ACTIVATION_PARAMS` (process loopback, Windows 10 20H2+), no virtual device needed | PipeWire monitor source (`@DEFAULT_MONITOR@`) |
| Playback | `ffmpeg -f audiotoolbox` | `ffmpeg -f wasapi`? No: ffmpeg has no WASAPI output; use `ffmpeg -f s16le -` piped into a small WASAPI player, or play in-process | `ffmpeg` into `pacat`, as upstream |
| Die with parent | `momr-audio run` (kqueue) | Job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` | `prctl(PR_SET_PDEATHSIG)`, as upstream |
| Keep awake | `caffeinate -i -w` | `SetThreadExecutionState(ES_CONTINUOUS)` | `systemd-inhibit` or the portal |
| Secrets | Keychain through `security` | Credential Manager (DPAPI) through the `windows` crate or `keyring` | Secret Service through `secret-tool` |
| Paths | `~/Library/…` | `%APPDATA%\momr`, `%LOCALAPPDATA%\momr\cache`, `Documents\Meetings` | XDG, as upstream |
| Live state | Unix socket, NDJSON | Unix socket works on Windows 10 1803+ (`AF_UNIX`), else a named pipe with the same NDJSON | Unix socket |
| Menu bar item | `momr-menubar` (Swift) | Tray icon in the shell | `momr watch` plugin for the user's bar |
| Whisper GPU | Metal | Vulkan or CUDA features of whisper-rs | Vulkan |
| Speaker model runtime | ONNX Runtime (CPU/CoreML) | ONNX Runtime with DirectML | ONNX Runtime |
| Open documents | `Info.plist` UTI, `GApplication::open` | File association in the installer, argv | freedesktop MIME |
| Install | DMG, Homebrew | MSIX or an Inno Setup installer, winget | Flatpak, distro packages |

The on-disk formats do not change: the meeting folder, `transcript.md`, the manifest and the NDJSON lines are the contract between platforms, so a meeting recorded on Windows opens on a Mac.

## The core crate first

Whatever the shell, the first step is the one plan 12 already describes: a Cargo workspace with `momr-core` (everything that does not draw) and a thin platform-facing API:

```
momr/                     workspace
├── crates/momr-core      audio staging and levels, export, transcribe, diarize, meeting, chapters,
│                         agent, models, provider, settings, cleanup, timer, ipc protocol
├── crates/momr-platform  the seam trait: capture command, playback, keep-awake, secrets, paths,
│   ├── macos.rs          die-with-parent; one file per platform behind cfg(target_os)
│   ├── windows.rs
│   └── linux.rs
├── apps/momr-gtk         the GTK app as it is today, depending on the two crates
└── apps/momr-<shell>     the next shell
```

The seam trait is small because every seam is already a `Command` or a path. `cfg(target_os)` is allowed in `momr-platform` and nowhere else; D02's rule ("no gating") keeps applying to the core and the shells.

## Which shell

The macOS shell stays GTK until plan 12's criteria say otherwise. For Windows the question is what to build the second shell in; the candidates, judged on the four things that matter here (native feel, speed to build, one codebase for the front end, and how the maintainer works):

| Option | Native feel | Build cost | Front-end reuse | Fit |
|---|---|---|---|---|
| GTK 4 + libadwaita on Windows | Low: GNOME look, no dark title bar, DLL bundle | Lowest: the app already exists | Complete | Fine for a first Windows build to test the seams; not what Windows users expect |
| Tauri 2 (Rust core, web front end in the system WebView) | Medium: native menus, tray, window chrome, notifications through plugins; the content area is web | Low: Vue or Nuxt front end, Tailwind; `tauri-plugin-*` for tray, dialogs, autostart | Complete across macOS, Windows, Linux; mobile possible | High: matches a Vue/Tailwind/TypeScript workflow; WebView2 ships with Windows 10/11, WKWebView on macOS |
| Slint | Medium-high: renders its own widgets with platform-style themes (Fluent, Cupertino, Material), accessible | Medium: a new declarative language, Rust callbacks | Complete | Good if the front end must stay in Rust; commercial licence not needed for open source |
| Iced / egui | Low-medium: draws everything itself, accessibility incomplete | Medium | Complete | Better for tools than for a Mac-feeling app |
| Dioxus | Medium: WebView-based on desktop, React-style Rust | Medium | Complete | Reasonable, less mature than Tauri |
| Native per platform (SwiftUI, WinUI 3) | Highest | Highest: three front ends | None | Only if native chrome is the product |

**Recommendation.** Extract `momr-core` and `momr-platform` now (they cost nothing on macOS and make the GTK app easier to test). Build the Windows shell in Tauri 2 with a Vue front end and Tailwind, sharing the core over Tauri commands and events (levels as an event stream, the same NDJSON shapes `ipc.rs` already serialises). Keep the transcribing scene as a `<canvas>` port of `animation.rs`. Ship the GTK app on macOS until the Tauri shell reaches parity on the plan 10 smoke checklist there too; then decide per platform which shell ships, judged on the plan 12 criteria (accessibility, native feel, bundle upkeep).

**Rejected.** Rewriting the core in TypeScript to run in the WebView: whisper.cpp, ONNX Runtime and the audio helpers are the app, and they are Rust and Swift for a reason. A universal Electron app: heavier than Tauri for the same front end, with no gain here.

## Steps, when entered

1. Workspace split (pure refactor; the GTK app behaves identically, CI proves it).
2. `momr-platform` trait with the macOS implementation moved in from `audio.rs`, `player.rs`, `paths.rs`, `helper.rs`, `provider.rs` (Keychain).
3. Windows implementations, one seam at a time, each with its `cargo test` behind a closure the way plan 10 describes; `momr-audio.exe` in Rust (`wasapi` crate), since there is no Swift on Windows.
4. Tauri shell: ready page with live meters, then record and strip, then done page and player, then Settings; each step judged against the smoke checklist.
5. Installer and signing; file association; a Windows CI runner.

## Sources

- Tauri 2: https://v2.tauri.app — plugins for tray, dialog, notification, autostart, global shortcut.
- Slint: https://slint.dev — Rust and declarative UI, platform themes.
- Iced: https://iced.rs; egui: https://github.com/emilk/egui; Dioxus: https://dioxuslabs.com.
- WASAPI process loopback: https://learn.microsoft.com/en-us/samples/microsoft/windows-classic-samples/applicationloopbackaudio-sample/ and `ActivateAudioInterfaceAsync` (`VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK`); Rust: https://docs.rs/wasapi (`AudioClient::new_application_loopback_client`).
- whisper-rs features: `metal`, `vulkan`, `cuda`; ort execution providers: CoreML, DirectML, CUDA.

## Status

Entered 2026-09-25 as the multi-target blueprint (D25): macOS first, core
compiling for Windows 11+ from slice 1. Next: the `momr-platform` seam
trait, then Windows implementations seam by seam when scheduled.
