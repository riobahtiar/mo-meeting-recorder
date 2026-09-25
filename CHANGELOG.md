# Changelog

One line per feature added or retired, newest first. Details live in the plan the line names; the pull request has the full story.

## Unreleased

### Added
- Voice enhancement switch on the ready page: AUSoundIsolation (`momr-audio enhance`) plus an ffmpeg voice chain on the saved audio; `.tracks/` and the transcript stay original (plan 17, D26).
- Manifest: optional `"enhanced"` field; absent reads as false (plan 17).
- Ready page: Record the microphone and computer audio, the microphone only or the computer audio only; the side not kept is silence, so the folder shape is unchanged (plan 17).
- `momr finish <staging>`: saves a stopped recording (audio, tracks, manifest, transcript) with the saved settings; the AppKit shell's Stop runs it (plan 12).
- Settings › General › Appearance: System, Light or Dark, applied at once (plan 14).
- Gear button in the header bar that opens Settings; ⌘, unchanged (plan 14).
- Settings split into General, Transcription, Recording, Audio and Storage pages (plan 14).
- The window remembers its size between launches; the done page folds its sidebar in a narrow window (plan 14).
- Settings › Storage: cache, speech models and settings with sizes, clear buttons and a full reset; meetings and Keychain keys are never touched (plan 15).
- Recording timer: stop after a length, or start and stop at clock times; ⌘T; countdown in the status line (plan 15).
- Settings › Audio: choose the microphone and record all apps or only chosen ones (plan 15; `momr-audio mic --device`, `system --bundle`).
- Plans folder: `plans/archives/` with an index for finished plans; plan 16 on Windows and Linux versions.

### Changed
- Timer rules live in core (`Plan::from_choices`), so every shell applies the same length, DST and overnight rules (plan 15).
- Process groups are a type (`process::Group`), so only a child `spawn_detached` made a group leader can be group-killed (plan 12).
- The playback output comes from `momr-platform` instead of each shell naming `audiotoolbox`; empty playlists and negative starts are refused or clamped (plans 12, 16).
- One clock format (`timer::clock`) for the recording clock, countdown and player (plan 12).
- Socket commands (`start`, `stop`, `pause`, `compact`, `watch`) no longer run `defaults` for the language unless they print (plan 12).
- AppKit shell: `Source` and recorder state enums, a meter that stops its timer off-window, and a test target (plan 12).
- AppKit shell writes the `recording.json` staging note, so the GTK shell's recovery finishes its crashed recordings (plan 12).
- Compact strip reworked: the header bar stays (clock as the title, Pause, Stop, Expand), one two-lane wave under it (plan 14).
- Transcribing scene: the title is measured and fitted to the window, in Menlo (plan 14).
- API key rows are expanders with the status and the where-to-get-it hint in full (plan 14).
- Rust workspace: `momr-core` holds cleanup, export, helper and locales with no UI code; the GTK app behaves identically, and the core checked on Windows MSVC until ureq joined it (plans 12, 16).
- `momr-platform` seam crate with de-glibbed `paths` (`$HOME`/`%USERPROFILE%`, absolute `XDG_*`, `~/Documents`); both new crates check on Windows MSVC, lib and tests (plans 12, 16).
- Recording timer and filename sanitizer in `momr-core` (chrono clock, DST gaps yield None); shell behavior unchanged (plan 12).
- Transcribe chain in `momr-core` (transcribe, models, meeting, provider, nemotron, diarize, settings, appearance enum); CLI exits convert at the shell boundary, behavior unchanged (plan 12).
- Platform `process`, `fs` and `sock` seams; audio, IPC, agent and chapters in the core; capture restart signals the single pid, not the group (plan 12).
- Playback mechanics in `momr-core` with the output sink as a parameter; widget and colors stay in the shell (plan 12).
- Native AppKit shell started: ready window with live meters from `momr-audio`, app menu and About (plan 12).
- AppKit recording: staging in the shared layout, pause with clock, stop exports the meeting folder and transcribes (plan 12).

### Retired
- The unused `libc` dependency of the GTK shell (plan 12).
- `open_no_follow`'s Linux flag values, wrong on aarch64; other targets refuse to build until plan 16 (plan 16).
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
- AppKit meetings match GTK ones: Settings' meetings folder and format, mono `.tracks`, your name, a proper transcript heading (plan 12).
- Two recordings in the same minute with the same title get their own folders (`… 2`) in both shells (plan 12).
- AppKit shell: failed writes, failed resumes and the transcriber's reason are shown; the meeting folder is offered even when the transcript failed (plan 12).
- AppKit shell: capture state lives on one queue (no data race on pause and stop); `momr` is found at Start, through the login shell's PATH as a Finder launch needs (plan 12).
- AppKit shell: ⌘Q or closing the window while recording asks, and can stop, save and quit (plan 12).
- The guarded-playback test runs again from the workspace (it skipped silently) (plan 12).
- Timer: times near a DST change resolve instead of silently turning the timer off; a skipped time says so (plan 15).
- Stopping playback, captures and agents calls kill(2) directly and reports failures; grok's binary link error is reported again (plan 12).
- Agent workdirs are created fresh (never reused) with 0700 on every level (plan 12).
- The dialog corner radius rule in `macos.css` is back (plan 14).
- A time the clock repeats when DST ends reads as its first pass: chrono lists the later instant first on macOS, so `earliest()` is not trusted (plan 15).
- AppKit meters use the GTK shell's scale (both channels, -60 dB), so the two shells' meters read alike (plan 12).
- The home directory never falls back to a relative path, so no file lands relative to wherever the app runs (plan 06).
- Module docs, AGENTS.md, the porting map and plans 12, 14 and 16 describe the workspace as built (plans 12, 16).
- Settings dialog: wider (820), the stock × in the header hidden (Escape closes it; a display check decides whether that is enough), tighter page-switcher padding, so Indonesian tab titles fit (plan 14 follow-up).
- Swift helper builds on the macOS 27 SDK: the per-app tap passes process object ids straight to `stereoMixdownOfProcesses` (plan 15).
- Clippy clean again: the Audio settings process list sorts by key (plan 15).

## 1.1.1 and earlier

The macOS port from the Linux original: plans 02 to 13 in `plans/`. See the plan board for what each delivered.
