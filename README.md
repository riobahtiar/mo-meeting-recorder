# MOM Recorder

**MOM Recorder** (short: **MOMR**) records your meetings on a Mac: your microphone and the computer audio as two tracks, and when you stop you get a transcript with speakers, chapters and a player. You can also drop in a recording you already have. By default everything is transcribed on your own machine with [whisper.cpp](https://github.com/ggml-org/whisper.cpp), so no audio leaves your computer; the optional cloud providers you can switch on are described under [Privacy](#privacy). No bot joins your call. It works with any meeting app, because it simply listens to what your Mac plays and what you say.

> **Status: in development.** MOM Recorder grew out of [Meeting Recorder](https://github.com/jankeesvw/omarchy-meeting-recorder) by Jankees van Woezik, a Linux app. The Rust core is kept; the Linux integrations are being replaced by macOS ones. It builds and records on macOS from a terminal checkout; the double-clickable app and the on-screen polish are still landing. The plan, the porting map and the decisions live in [`plans/`](plans/README.md).

## Where it stands

| Milestone | Plan | Status |
|---|---|---|
| Compiles on macOS; command-line transcription works | [02](plans/archives/02-compile-on-macos.md) | done — `cargo build --release`, `cargo test`, clippy and fmt clean; `transcribe-file` works on CPU and with `--features metal`; window opens |
| Records the microphone and the computer audio | [03](plans/03-audio-capture.md) | in progress — helper captures both (tap plays back at 0.77 peak), staging tracks equal length, transcribe gives You and Remote; drawn meters and device switching need a display session |
| Plays back; compact strip; ⌘ shortcuts | [04](plans/04-playback-window-shortcuts.md) | in progress — audiotoolbox playback, `run` watchdog (kill -9 safe), caffeinate held/reaped, ⌘ accelerators; in-app seek and strip need a display session |
| Chapters through an agent set in `config.toml` | [05](plans/05-agent-and-config.md) | in progress — `agent = "…"` selects, `ask` runs live (pi), crush refused, hung group killed in test; long-meeting chapters need a display session |
| Files under `~/Library`; works when launched from Finder | [06](plans/archives/06-paths-and-environment.md) | done — `~/Library` homes (GLib has no Cocoa support here, see D22), socket/staging/watch verified, `open` launch spawns helper (TCC-flat meters until the plan 08 bundle) |
| Native menu bar, window chrome, system font, Apple colours, Preferences, About | [07](plans/07-macos-look-and-feel.md) | in progress — actions+menu+About+Preferences coded, Apple palette with tests, app icon in the bundle; on-screen checks need a display session |
| Homebrew formula; signed `MOM Recorder.app` in a DMG that opens `.meeting-recorder` files | [08](plans/08-app-bundle-and-distribution.md) | in progress — bundle assembles and launches, formula + workflow staged; signing, DMG and install docs wait for cert and tag |
| Live recording status in the menu bar | [09](plans/09-menu-bar-item.md) | in progress — SwiftBar script renders, native item builds with tested protocol, app spawns/kills it; on-screen check needs a display session |
| Tests and CI on macOS | [10](plans/10-testing-and-ci.md) | in progress — unit tests, the end-to-end `say` fixture, the meeting-folder fixture and the Swift tests are in place; CI workflows are written but disabled for now (commit ae48747); the smoke checklist needs a display session |
| Identity: bundle id, UTI, folder names, brand sweep | [11](plans/11-identity.md) | in progress — `io.github.riobahtiar.MOMRecorder`, its UTI and the `momr` folders confirmed (D13), comment sweep and demo kit done; macOS screenshots and the release version bump remain |
| Native AppKit shell on the Rust core; GTK retires at parity | [12](plans/12-native-shell-option.md) | in progress — workspace split done; AppKit shell records and saves through `momr finish`; meters on screen and a live record need a display session |
| Cloud transcription and Indonesian UI | [13](plans/13-transcription-providers.md) | in progress — ElevenLabs, Google and OpenRouter routed with Keychain keys, EN/ID locales; live cloud runs need user keys, UI walk needs a display session |
| UI polish after the first display session | [14](plans/14-ui-polish.md) | in progress — focus rings, remembered window size, appearance switch, Settings pages and gear button, compact strip and animation fit coded; the on-screen check is next |
| Storage reset, recording timer, audio sources | [15](plans/15-reset-timer-sources.md) | in progress — cleanup and timer logic tested, Settings › Storage, Timer dialog, microphone and per-app pickers coded; helper builds with tested `--device`/`--bundle` flags, on-screen source switching needs a display session |
| Windows and Linux versions | [16](plans/16-multi-platform-architecture.md) | blueprint — core compiles for Windows 11+ from slice 1, `momr-platform` seam next |

Scope is macOS 14 or newer on Apple silicon and Intel. iPhone and iPad are out: the app is GTK 4 and libadwaita. Screenshots come with the macOS look in plan 07.

## What it does

- **Records both sides of the call** as two separate tracks, with live meters before you start so you can see both arrive. Pause freezes both.
- **Stays out of the way.** The window shrinks to a strip with only the clock and the two waves.
- **Transcribes on your own machine** by default when you stop (cloud providers are opt-in, see [Privacy](#privacy)), with an animation that shows the lines as they are recognised.
- **Tells the speakers apart.** Your side and the other side come from the two tracks; several people on the other side are told apart by voice.
- **Imports any recording** you drop on the window, and separates up to eight voices with NVIDIA's [Nemotron 3 Diarization](https://huggingface.co/nvidia/Nemotron-3-Diarization), run locally.
- **Gives you a transcript you can listen to**, edit in place, and copy. Click any line to play from there.
- **Chapters by your coding agent**, when one is set up. The agent runs with every tool switched off and can only answer with text.
- **Keeps your recording safe.** An unfinished recording is offered back on the next start.
- **Records on a timer.** Stop after a set length, or start and stop at clock times (⌘T).
- **Records what you choose.** Pick the microphone, and record every app or only the ones you name (Settings › Audio).
- **Cleans up after itself.** Settings › Storage shows what the app keeps and clears it; meetings are never touched.

## Build

```bash
xcode-select --install
brew install gtk4 libadwaita adwaita-icon-theme cmake pkgconf ffmpeg
sh scripts/build-macos.sh   # Rust binary + momr-audio/momr-menubar helpers
```

The first transcription downloads the whisper model (about 1.6 GB, once); telling voices apart in an imported file downloads the speaker model (about 120 MB) on first use.

Every meeting is a plain folder in `~/Documents/Meetings`: the audio, `transcript.md`, a small `.meeting-recorder` manifest and a hidden `.tracks/` with both sides. The layout is the same as upstream's, so a meeting recorded with the Linux app opens here, and one recorded here opens there. The manifest differs in two small ways. New meetings write `"app": "momr"` where upstream wrote `"omarchy-meeting-recorder"`; no reader checks that field, so both open in both apps. A new optional `"provider"` field records which transcription engine made the transcript (`local`, `elevenlabs`, `google` or `openrouter`), and older readers ignore it. The speaker labels in `transcript.md` ("You", "Remote", "Remote N", "Speaker N") and its language line stay in English whatever the interface language, because scripts read them. Models, settings and `config.toml` live under `~/Library/Application Support/momr`; staging, the cache and the live-state socket under `~/Library/Caches/momr`.

## Command line

| Command | What it does |
|---|---|
| `momr` | Open the recorder, ready to record |
| `momr <folder or .meeting-recorder file>` | Open a saved meeting |
| `momr start` / `pause` / `stop` / `compact` | Control the running app |
| `momr watch` | Stream the recorder state as NDJSON |
| `momr transcribe <mic> <computer> [--language xx] [--model name] [--provider id]` | Transcribe two tracks to Markdown |
| `momr transcribe-file <audio> [--speakers N] [--language xx] [--model name] [--provider local\|elevenlabs\|google\|openrouter]` | Transcribe one file, telling voices apart |
| `momr finish <staging folder> [--title T]` | Save a stopped recording from its staging folder into a meeting folder (audio, tracks, manifest, transcript); prints the folder |
| `momr ask "<prompt>" < text` | Run a prompt through the agent, without tools |

The `transcribe` commands need no window or audio device, so they are the first thing to try on a fresh build. Both take short forms too (`-l`, `-m`, `-p`, and `-s` for `transcribe-file`). `--provider` (or `-p`) picks the transcription engine for that run only, overriding the `provider` key in `config.toml` without changing it; a cloud provider still needs its API key saved in Settings.

## Privacy

With the default local transcription, the audio, the transcript and
everything else stay on your Mac. Two things can leave it, each only when
you set it up:

- Chapters: the transcript text goes to the agent's service, the one you
  already chose and pay for.
- Cloud transcription (Settings › Transcription › ElevenLabs, Google or OpenRouter):
  the meeting audio goes to that provider for transcription. API keys stay
  in your macOS Keychain, under the services `momr-elevenlabs`, `momr-google`
  and `momr-openrouter`, never in a config file. ElevenLabs, Google and
  OpenRouter keep and process uploads under their own terms — use the local
  default for anything sensitive. The OpenRouter model is `openrouter_model`
  in config.toml (`openai/whisper-1` unless set).

A cloud run behaves a little differently from the local one:

- Google Cloud needs an explicit transcription language. Auto-detect is
  refused for Google, because its v1 API has no language detection.
- The audio is uploaded in chunks (about 10 minutes for ElevenLabs, 55
  seconds for Google and OpenRouter), and the provider numbers speakers
  afresh in each chunk. "Remote 2" in one chunk may be a different voice in
  the next, so check the names on long meetings. OpenRouter returns no
  speaker tags at all.

## Contributing

Read [AGENTS.md](AGENTS.md), then the plan you are working on in [`plans/`](plans/README.md).

## Credits and license

MOM Recorder is based on [Meeting Recorder](https://github.com/jankeesvw/omarchy-meeting-recorder) by Jankees van Woezik. Transcription is [whisper.cpp](https://github.com/ggml-org/whisper.cpp) through [whisper-rs](https://github.com/tazz4843/whisper-rs). Speaker separation is Nemotron 3 Diarization in the [ONNX community](https://huggingface.co/onnx-community/Nemotron-3-Diarization-ONNX) export, under the [OpenMDW license](https://huggingface.co/nvidia/Nemotron-3-Diarization). MIT, as upstream.
