# 12 Native shell option

## Goal

Know when the GTK app is not good enough and what to build if so: a SwiftUI front end on the same Rust core. This plan is a reference until the criteria below say otherwise; nothing here is scheduled.

## When to enter this plan

After plan 07 is done and the smoke checklist has been walked by at least two people on real meetings of their own (never committed), enter this plan if **two or more** of these hold:

1. VoiceOver cannot read the transcript or operate the recording controls (plan 07 step 13 found GTK's macOS accessibility insufficient).
2. Scrolling, trackpad gestures or text rendering are visibly not native to users who did not know the app is GTK (they mention it unprompted).
3. Bundling GTK (plan 08) keeps breaking on macOS updates and the maintainers spend more time on the bundle than on the app.
4. A feature Mac users ask for needs AppKit and cannot be reached from GTK: Shortcuts actions, Share sheet, Handoff, a Dock progress bar, Stage Manager behaviour, live activities in the menu bar beyond plan 09.

If fewer than two hold, the answer is to keep improving plan 07.

## Architecture

```
momr-core (Rust crate, no GTK)           momr (GTK app)
├── audio staging and levels                └── depends on momr-core
├── export (ffmpeg)
├── transcribe (whisper-rs), diarize, nemotron (ort)
├── meeting, chapters, agent, models, paths
├── ipc (socket + NDJSON)
└── C ABI (or UniFFI) for Swift            MOM Recorder.app (SwiftUI)
                                            ├── RecorderView: meters, name, format, language, Start
                                            ├── TranscriptView: rows, inline edit, speaker swap, delete
                                            ├── WaveformPlayer: Canvas waveform, AVAudioEngine playback
                                            ├── TranscribingView: progress (SwiftUI animation)
                                            ├── Preferences, About, menu bar item, document type
                                            └── calls momr-core through the bridge
```

The split is already latent in the code: `ui.rs`, `animation.rs` and `player.rs`'s drawing are the only modules that import GTK for UI; `settings.rs`, `theme.rs`, `models.rs`, `transcribe.rs` import `glib` only for paths and timing, which `paths.rs` (plan 06) already isolates.

## Steps, when entered

### 1. Extract `momr-core` (pure refactor, the GTK app keeps working)

Workspace with two crates. Move every module that does not draw into `momr-core`; replace its `glib` uses with std or a tiny `paths` trait implemented by the front end. Progress events (`transcribe::Events`) become a channel of plain enums. The GTK app depends on the crate and behaves identically; CI proves it.

### 2. Bridge

Choose one:

- **UniFFI**: generates Swift bindings from Rust with async support; heavier build, cleaner Swift.
- **C ABI + a hand-written Swift wrapper**: `#[no_mangle] extern "C"` functions with opaque handles and JSON for structured data; fewer tools, more boilerplate.

Expose: start/pause/stop recording into a staging dir; level stream; export to a meeting folder; transcribe (two tracks or one file) with progress; diarize; load and save a manifest; chapters through the agent; model management. Playback moves to Swift (`AVAudioEngine`), which removes the ffmpeg-to-audiotoolbox pipeline. Capture can move in-process too, retiring `momr-audio`.

### 3. SwiftUI app, in the order the GTK app grew

1. Ready page with live meters (levels from the core over the bridge), Start.
2. Recording and paused with the compact strip as a small utility window.
3. Transcribing progress; done page with transcript rows and the waveform player.
4. Inline editing, speaker rename, undo; Copy transcript.
5. Import; chapters; recovery on launch.
6. Preferences, About, document type, menu bar item.

Parity is the smoke checklist in plan 10; the folder format and manifest stay identical so meetings move between the two front ends and from upstream.

### 4. Ship both for a while

The DMG carries the SwiftUI app; the Homebrew formula keeps the GTK binary as `momr-gtk` for a release or two. Retire the GTK front end when the checklist is fully green on the Swift one.

## Costs

- A second UI to keep in sync with core changes.
- Apple Developer account for distribution (already needed for plan 08).
- Xcode as a build dependency.

## Status

Reference only. Revisit after plan 07's smoke checklist.
