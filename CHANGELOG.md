# Changelog

One line per feature added or retired, newest first. Details live in the plan the line names; the pull request has the full story.

## Unreleased
### Added
- Settings › General › Appearance: System, Light or Dark, applied at once (plan 14).
- Gear button in the header bar that opens Settings; ⌘, unchanged (plan 14).
- Settings split into General, Transcription, Recording, Audio and Storage pages (plan 14).
- The window remembers its size between launches; the done page folds its sidebar in a narrow window (plan 14).
- Settings › Storage: cache, speech models and settings with sizes, clear buttons and a full reset; meetings and Keychain keys are never touched (plan 15).
- Recording timer: stop after a length, or start and stop at clock times; ⌘T; countdown in the status line (plan 15).
- Settings › Audio: choose the microphone and record all apps or only chosen ones (plan 15; `momr-audio mic --device`, `system --bundle`).
- Plans folder: `plans/archives/` with an index for finished plans; plan 16 on Windows and Linux versions.

### Changed
- Compact strip reworked: the header bar stays (clock as the title, Pause, Stop, Expand), one two-lane wave under it (plan 14).
- Transcribing scene: the title is measured and fitted to the window, in Menlo (plan 14).
- API key rows are expanders with the status and the where-to-get-it hint in full (plan 14).
- Rust workspace: `momr-core` holds cleanup, export, helper and locales with no UI or platform code; the GTK app behaves identically, and the core checks on Windows MSVC (plans 12, 16).
- `momr-platform` seam crate with de-glibbed `paths` (`$HOME`/`%USERPROFILE%`, absolute `XDG_*`, `~/Documents`); both new crates check on Windows MSVC, lib and tests (plans 12, 16).
- Recording timer and filename sanitizer in `momr-core` (chrono clock, DST gaps yield None); shell behavior unchanged (plan 12).
- Transcribe chain in `momr-core` (transcribe, models, meeting, provider, nemotron, diarize, settings, appearance enum); CLI exits convert at the shell boundary, behavior unchanged (plan 12).
- Platform `process`, `fs` and `sock` seams; audio, IPC, agent and chapters in the core; capture restart signals the single pid, not the group (plan 12).
- Playback mechanics in `momr-core` with the output sink as a parameter; widget and colors stay in the shell (plan 12).
- Native AppKit shell started: ready window with live meters from `momr-audio`, app menu and About (plan 12).
- AppKit recording: staging in the shared layout, pause with clock, stop exports the meeting folder and transcribes (plan 12).
### Retired
- The wildcard focus ring in `macos.css` that outlined every container (plan 14).
- Per-page window resizing (480×700 ready, 1100×760 done) (plan 14).
- The strip's custom drag handle and its "Drag to move" hint (plan 14).

### Fixed
- AppKit shell launches: the delegate is set by hand, since `@main` needs a nib to create it (plan 12).
- AppKit pause no longer erases the recording so far: resume appends to the raw tracks (plan 12).
- AppKit stop no longer hangs on transcripts over the pipe buffer (about 30 minutes of meeting) (plan 12).
- AppKit capture: a helper that exits is reported with its reason and restarted, no longer spinning a core; Start is refused when nothing captures (plan 12).
- The socket keeps accepting after a failed accept, so `momr stop`, `watch` and the menu bar item stay connected (plan 12).
- Plain `cargo test` and `cargo clippy` cover the whole workspace again (`default-members`) (plan 12).
- Settings dialog: wider (820), the stock × in the header hidden (traffic lights already close it), tighter page-switcher padding, so Indonesian tab titles fit (plan 14 follow-up).
- Swift helper builds on the macOS 27 SDK: the per-app tap passes process object ids straight to `stereoMixdownOfProcesses` (plan 15).
- Clippy clean again: the Audio settings process list sorts by key (plan 15).

## 1.1.1 and earlier

The macOS port from the Linux original: plans 02 to 13 in `plans/`. See the plan board for what each delivered.
