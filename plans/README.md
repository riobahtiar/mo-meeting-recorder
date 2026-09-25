# Plans for MOM Recorder on macOS

This folder is the working plan for turning the app into **MOM Recorder** (short: **MOMR**, binary `momr`), a macOS app. Each numbered file is one milestone with its own goal, ordered steps, verification and done criteria. `00-overview.md` holds the goal, the target architecture and the porting map; `01-decisions.md` holds every decision already taken, so nobody relitigates them by accident.

## How to work a plan

1. Pick the lowest-numbered plan whose **Prerequisites** are all done (see the board below).
2. Read its **Background** before touching code: it names the files and functions and says why they are the way they are.
3. Work the **Steps** in order. Each step ends in something you can check; check it before moving on.
4. Run the plan's **Verify** section in full when the steps are done, then tick the **Done when** list.
5. Update the board here, the status table in the root `README.md`, the plan's own status checklist and `CHANGELOG.md` (one line per feature added or retired), in the same commit as the code. A plan that is fully done moves to `archives/` (see its README).
6. A decision the plan leaves **Open** is settled by adding an entry to `01-decisions.md`, then continuing.

Checkbox states: `[ ]` not started, `[~]` in progress, `[x]` done.

## Board

| Plan | Goal | Prerequisites | Phase | Status |
|---|---|---|---|---|
| [00 Overview](00-overview.md) | Goal, scope, architecture, porting map, risks | | | reference |
| [01 Decisions](01-decisions.md) | Decision log | | | reference |
| [02 Compile on macOS](archives/02-compile-on-macos.md) | `cargo build` and `cargo test` pass; Omarchy leftovers that only a compiler can check are gone | | 1 Works | archived |
| [03 Audio capture](03-audio-capture.md) | Both meters move; recording writes both tracks | 02 | 1 Works | `[~]` |
| [04 Playback, window, shortcuts](04-playback-window-shortcuts.md) | Play and seek; children die with the app; compact strip; ⌘ shortcuts | 02 | 1 Works | `[~]` |
| [05 Agent and config](05-agent-and-config.md) | Chapters through an agent named in config, same no-tools boundary | 02 | 1 Works | `[~]` |
| [06 Paths and environment](archives/06-paths-and-environment.md) | Files under `~/Library`; Finder-launched app finds its tools | 02 | 1 Works | archived |
| [07 macOS look and feel](07-macos-look-and-feel.md) | Native menu bar, window chrome, typography, controls, colours, Preferences, About | 03, 04, 06 | 2 Feels native | `[~]` |
| [08 App bundle and distribution](08-app-bundle-and-distribution.md) | Homebrew formula; signed `MOM Recorder.app` in a DMG that opens `.meeting-recorder` files | 03, 04, 05, 06 | 3 Ships | `[~]` |
| [09 Menu bar item](09-menu-bar-item.md) | Live recording status in the menu bar | 03, 08 | 2 Feels native | `[~]` |
| [10 Testing and CI](10-testing-and-ci.md) | Tests for the new seams; CI on macOS (workflows disabled for now) | 02 | all | `[~]` |
| [11 Identity](11-identity.md) | Confirm the bundle id, UTI and folder names; last brand sweep | 08 | 3 Ships | `[~]` |
| [12 Native shell option](12-native-shell-option.md) | AppKit shell on the Rust core; GTK retires at parity | 07 | 4 Native | `[~]` entered 2026-09-25 (D24) |
| [13 Transcription providers](13-transcription-providers.md) | ElevenLabs, Google and OpenRouter transcription, English + Indonesian UI | 05, 07 | 5 Providers | `[~]` |
| [14 UI polish](14-ui-polish.md) | Focus rings, one window size, appearance switch, rows that fit, animation, Settings pages and button, compact strip | 07 | 2 Feels native | `[~]` |
| [15 Reset, timer, sources](15-reset-timer-sources.md) | Storage cleanup and reset; timed recordings; microphone and per-app computer audio | 14 | 6 Features | `[~]` |
| [16 Multi-platform architecture](16-multi-platform-architecture.md) | Core crate, platform seams and the shell to build Windows and Linux versions on | 12 | 4 Native | `[~]` entered 2026-09-25 as blueprint (D25) |
| [17 Sources and voice enhancement](17-sources-and-voice-enhancement.md) | Record the mic, the computer or both; a voice enhancement switch, transcripts from the original audio (D26) | 12, 15 | 6 Features | `[~]` |

Phases: **1 Works** is a usable app started from a terminal. **2 Feels native** is what a Mac user expects from the chrome. **3 Ships** is something a person can download and double-click. **4 Native** is the AppKit shell on the Rust core (plan 12, entered by D24) and the groundwork for other platforms (plan 16). **5 Providers** and **6 Features** are what people asked for once it recorded.

A plan whose every box is ticked moves to [`archives/`](archives/README.md), whose index says what each one delivered. What each release added or retired is in the root [`CHANGELOG.md`](../CHANGELOG.md).

## Conventions in these files

- Paths are relative to the repository root. Functions are written `file.rs` `name()`.
- Code blocks marked *sketch* show the shape of a change, not a drop-in patch; adapt names and error handling to the surrounding code.
- "Verify:" lines inside a step are facts taken from documentation or source reading that have not been exercised on this machine yet. Exercise them first; if one is wrong, fix the plan in the same commit.
- Shortcuts are written with macOS symbols: ⌘ Command, ⇧ Shift, ⌥ Option, ⌃ Control.
- Names: **MOM Recorder** in anything a user reads, **MOMR** where space is short (menu bar item, log prefixes), `momr` for the binary, the helper prefix (`momr-audio`, `momr-menubar`), folders and the socket.
