# MOM Recorder

**MOM Recorder** (short: **MOMR**) records your meetings on a Mac: your microphone and the computer audio as two tracks, and when you stop you get a transcript with speakers, chapters and a player. You can also drop in a recording you already have. Everything is transcribed on your own machine with [whisper.cpp](https://github.com/ggml-org/whisper.cpp). No bot joins your call, and no audio leaves your computer. It works with any meeting app, because it simply listens to what your Mac plays and what you say.

> **Status: in development.** MOM Recorder grew out of [Meeting Recorder](https://github.com/jankeesvw/omarchy-meeting-recorder) by Jankees van Woezik, a Linux app. The Rust core is kept; the Linux integrations are being replaced by macOS ones. It does not build on macOS yet. The plan, the porting map and the decisions live in [`plans/`](plans/README.md).

## Where it stands

| Milestone | Plan | Status |
|---|---|---|
| Compiles on macOS; command-line transcription works | [02](plans/02-compile-on-macos.md) | done — `cargo build --release`, `cargo test` (24 passed), clippy and fmt clean; `transcribe-file` works on CPU and with `--features metal`; window opens, meters flat (no capture until plan 03) |
| Records the microphone and the computer audio | [03](plans/03-audio-capture.md) | not started |
| Plays back; compact strip; ⌘ shortcuts | [04](plans/04-playback-window-shortcuts.md) | not started |
| Chapters through an agent set in `config.toml` | [05](plans/05-agent-and-config.md) | not started |
| Files under `~/Library`; works when launched from Finder | [06](plans/06-paths-and-environment.md) | not started |
| Native menu bar, window chrome, system font, Apple colours, Preferences, About | [07](plans/07-macos-look-and-feel.md) | not started |
| Homebrew formula; signed `MOM Recorder.app` in a DMG that opens `.meeting-recorder` files | [08](plans/08-app-bundle-and-distribution.md) | not started |
| Live recording status in the menu bar | [09](plans/09-menu-bar-item.md) | not started |

Scope is macOS 14 or newer on Apple silicon and Intel. iPhone and iPad are out: the app is GTK 4 and libadwaita. Screenshots come with the macOS look in plan 07.

## What it does

- **Records both sides of the call** as two separate tracks, with live meters before you start so you can see both arrive. Pause freezes both.
- **Stays out of the way.** The window shrinks to a strip with only the clock and the two waves.
- **Transcribes on your own machine** when you stop, with an animation that shows the lines as they are recognised.
- **Tells the speakers apart.** Your side and the other side come from the two tracks; several people on the other side are told apart by voice.
- **Imports any recording** you drop on the window, and separates up to eight voices with NVIDIA's [Nemotron 3 Diarization](https://huggingface.co/nvidia/Nemotron-3-Diarization), run locally.
- **Gives you a transcript you can listen to**, edit in place, and copy. Click any line to play from there.
- **Chapters by your coding agent**, when one is set up. The agent runs with every tool switched off and can only answer with text.
- **Keeps your recording safe.** An unfinished recording is offered back on the next start.

## Build

```bash
xcode-select --install
brew install gtk4 libadwaita adwaita-icon-theme cmake pkgconf ffmpeg
cargo build --release
```

Until [plan 02](plans/02-compile-on-macos.md) lands, the build stops in `src/player.rs` on a Linux-only call. The first transcription downloads the whisper model (about 1.6 GB, once); telling voices apart in an imported file downloads the speaker model (about 120 MB) on first use.

Every meeting is a plain folder in `~/Documents/Meetings`: the audio, `transcript.md`, a small `.meeting-recorder` manifest and a hidden `.tracks/` with both sides. The layout is the same as upstream's, so a meeting recorded with the Linux app opens here. Models, settings and the cache go under `~/Library/Application Support/momr` and `~/Library/Caches/momr` once [plan 06](plans/06-paths-and-environment.md) lands.

## Command line

| Command | What it does |
|---|---|
| `momr` | Open the recorder, ready to record |
| `momr <folder or .meeting-recorder file>` | Open a saved meeting |
| `momr start` / `pause` / `stop` / `compact` | Control the running app |
| `momr watch` | Stream the recorder state as NDJSON |
| `momr transcribe <mic> <computer> [--language xx] [--model name]` | Transcribe two tracks to Markdown |
| `momr transcribe-file <audio> [--speakers N] [--language xx] [--model name]` | Transcribe one file, telling voices apart |
| `momr ask "<prompt>" < text` | Run a prompt through the agent, without tools |

The `transcribe` commands need no window or audio device, so they are the first thing to try on a fresh build.

## Privacy

The audio, the transcript and everything else stay on your Mac. The only thing that leaves it is the transcript text for the chapters, and only when you have set up an agent: it goes to that agent's service, the one you already chose and pay for.

## Contributing

Read [AGENTS.md](AGENTS.md), then the plan you are working on in [`plans/`](plans/README.md).

## Credits and license

MOM Recorder is based on [Meeting Recorder](https://github.com/jankeesvw/omarchy-meeting-recorder) by Jankees van Woezik. Transcription is [whisper.cpp](https://github.com/ggml-org/whisper.cpp) through [whisper-rs](https://github.com/tazz4843/whisper-rs). Speaker separation is Nemotron 3 Diarization in the [ONNX community](https://huggingface.co/onnx-community/Nemotron-3-Diarization-ONNX) export, under the [OpenMDW license](https://huggingface.co/nvidia/Nemotron-3-Diarization). MIT, as upstream.
