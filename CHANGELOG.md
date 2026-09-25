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

### Retired
- The wildcard focus ring in `macos.css` that outlined every container (plan 14).
- Per-page window resizing (480×700 ready, 1100×760 done) (plan 14).
- The strip's custom drag handle and its "Drag to move" hint (plan 14).

## 1.1.1 and earlier

The macOS port from the Linux original: plans 02 to 13 in `plans/`. See the plan board for what each delivered.
