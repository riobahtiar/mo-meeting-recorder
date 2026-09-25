# 17 Recording sources and voice enhancement

## Goal

Two choices on the ready page that people asked for: which side of the call is recorded (the microphone, the computer audio, or both as today), and a voice enhancement switch that takes out wind, traffic, keyboard and room noise and leaves voices clear and full, the way a decent studio microphone would. The transcript is never made from the enhanced audio (D26).

## Done when

- [ ] The ready page has a Record row with "Microphone and computer audio", "Microphone only" and "Computer audio only"; the choice is kept across launches, is fixed while a recording runs, and the meter of a side that is not kept is dimmed but still moves. The AppKit shell offers the same choice and reads the saved one.
- [ ] A recording with one side produces the usual meeting folder: the side not kept is silence in the audio files and in `.tracks/`, and the transcript has only the kept side's speakers.
- [ ] The ready page has a Voice enhancement switch; with it on, the meeting's audio files are enhanced, the manifest says so, and the transcript is the same as with it off (D26).
- [ ] `cargo test` covers the source keys and rule and the enhancement's command shape; the AppKit tests cover the source keys.

## Prerequisites

Plan 12 (the core both shells share) and plan 15 (the Audio settings the sources sit beside).

## Background

- **Sources.** Both captures keep running whatever the choice (`audio.rs`), so the meters work before a call and switching back needs no restart. A side that is not kept gets an empty raw file at Start: `export_audio` pads the shorter track with silence (`pad_to_same_length`), `speech_gain_db` leaves silence alone, and the transcriber skips a silent track (`transcribe.rs` `is_silent`), so the meeting folder keeps its two-track shape and upstream readers see nothing new.
- **Enhancement.** Options and their evidence are in [`research/voice-enhancement.md`](research/voice-enhancement.md). Whatever is chosen runs on the audio a person listens to only; the raw staging tracks and `.tracks/` stay as captured (D26).

## Steps

### 1. Recording sources

`audio::Sources` (`Both`, `MicOnly`, `ComputerOnly`) with a settings key (`sources`, unknown reads as both) and `records(Device)`. The GTK ready page gets a `ComboRow` between the title and the audio file rows; `start` opens the kept sides' raw files through `Source::start_recording` and creates the others empty; the silent-computer hint only fires when the computer side was kept. The AppKit shell gets a popup with the same keys, read from `settings.json` without writing it.

### 2. Voice enhancement

To be specified from the research: the pipeline, where it runs (capture, export, or both), its dependency cost, and the manifest key.

## Verify

1. Choose each Record option, record ten seconds while speaking and playing a video: the folder has both files, the side not kept is silent, the transcript names only the kept side.
2. Relaunch: the choice is kept. Start a recording: the row is disabled until the meeting is saved.
3. Voice enhancement: to be written with step 2.

## Status

- [x] Step 1 recording sources (coded 2026-09-26; `cargo test`, clippy and the AppKit tests pass; the on-screen check is Verify 1–2)
- [ ] Step 2 voice enhancement
