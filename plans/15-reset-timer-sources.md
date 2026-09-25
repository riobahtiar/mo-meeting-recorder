# 15 Reset and cleanup, recording timer, audio sources

## Goal

Three things a person asked for after the first session on a display: a way to clear what the app has accumulated and to start over; a recording that stops itself after a set time or runs between two clock times; and a say in which microphone and which apps are recorded, instead of "the default input" and "everything the Mac plays".

## Done when

- [ ] Settings › Storage shows the size of the cache, of the downloaded speech models and of the settings, with a button each to clear them and one "Reset MOM Recorder" that does all three; every button confirms first, says what it will remove, and none of them ever touches a meeting folder or a Keychain key.
- [ ] On the ready page a Timer row opens a dialog with "Stop after" (hours and minutes), "Start at" and "Stop at" (clock times). A recording stops on its own when its time is up, the status line counts down, a toast warns one minute before, and a scheduled start begins the recording at that time from the ready page. ⌘T opens the same dialog from the Recording menu.
- [ ] Settings › Audio lets the user pick the microphone (system default or one device by name) and the computer audio (all apps, or a chosen set of running apps by name); a change restarts the capture within two seconds and the meters follow; the choice survives a relaunch.
- [ ] `cargo test` covers the cleanup rules (what is kept), the timer arithmetic and the capture arguments for a chosen device and chosen apps; the Swift argument tests cover `--device` and `--bundle`.

## Prerequisites

Plan 14 (the Settings pages these rows live in).

## Background

- **Storage.** The app writes to three places (`paths.rs`): `~/Library/Caches/momr` (staging folders named by start time, import scratch, the socket), `~/Library/Application Support/momr/models` (whisper `ggml-*.bin`, up to 3 GB each) and `~/Library/Application Support/momr` (`settings.json`, `config.toml`). Meetings live in `~/Documents/Meetings` and are the user's documents; nothing here deletes them. A staging folder with audio in it is an unfinished recording the next launch offers to save (`ui.rs` `offer_recovery`), so clearing the cache must say how many of those it will discard, and must never remove the staging folder of the recording in progress or the socket the running app serves on.
- **Timer.** `ui.rs` already ticks every 500 ms (`tick`) and knows the elapsed time without pauses (`elapsed`). A timer is a small plan checked from that tick: a maximum length, a start time and a stop time, any of them unset. Times of day are resolved to the next occurrence (today if still ahead, else tomorrow) when the plan is set, so "stop at 15:00" set at 16:00 means tomorrow and the row says so. The plan is per session: it is cleared when the recording stops, and the last "stop after" length is remembered as the dialog's default.
- **Sources.** `momr-audio mic` uses `AVAudioEngine`'s input node, which follows the default input; a specific device is chosen by setting `kAudioOutputUnitProperty_CurrentDevice` on the input node's audio unit before the engine starts. `momr-audio system` creates `CATapDescription(stereoGlobalTapButExcludeProcesses: [])`, every process; the per-app form is `CATapDescription(stereoMixdownOfProcesses:)` with process audio objects, found from bundle identifiers through `kAudioHardwarePropertyProcessObjectList` and `kAudioProcessPropertyBundleID`. Bundle identifiers are what the app saves, since pids change with every launch of the other app. An app that is not running when the tap is created contributes nothing until the helper notices it (`kAudioHardwarePropertyProcessObjectList` changes) and rebuilds the tap. `list` gains the device UIDs and the running audio processes so Settings can offer them by name.

## Steps

### 1. `cleanup.rs`

Pure functions over paths, so tests run on a temporary directory:

- `dir_size(dir) -> u64`.
- `clear_cache(cache, keep: &[PathBuf]) -> io::Result<Cleared>` removes every entry of the cache except the paths in `keep` (the live staging folder, the socket) and reports bytes and the number of unfinished recordings discarded.
- `delete_models(models_dir) -> io::Result<u64>`.
- `reset_settings(settings_file, config_file) -> io::Result<()>` removes both files; the app re-reads defaults on the next access, so nothing else changes.

Settings › Storage: three `adw::ActionRow`s with the size as subtitle and a button, plus a destructive "Reset MOM Recorder" `adw::ButtonRow`; each confirms with an `adw::AlertDialog` whose body says what goes and what stays. After clearing the cache the model banner and sizes refresh.

### 2. `timer.rs`

```rust
pub struct Plan { pub max_secs: Option<i64>, pub start_at: Option<i64>, pub stop_at: Option<i64> }
impl Plan {
    pub fn due_start(&self, now: i64) -> bool;
    pub fn due_stop(&self, now: i64, elapsed: i64) -> bool;
    pub fn remaining(&self, now: i64, elapsed: i64) -> Option<i64>;   // the sooner of the two limits
    pub fn describe(&self, now: i64) -> String;                       // the row subtitle
}
pub fn next_occurrence(hour: u32, minute: u32, now_local: &glib::DateTime) -> i64;
```

`ui.rs`: a `timer: RefCell<Plan>` on the recorder, a Timer `adw::ActionRow` under the language row, `win.timer` (⌘T) opening `TimerDialog` (an `adw::Dialog` with two `adw::SwitchRow`s and `adw::SpinRow`s for hours and minutes, and one for each clock time). `tick()` starts when `due_start`, stops when `due_stop`, updates the status line with `remaining`, and toasts once at 60 s left. Stopping clears the plan; `settings.json` keeps `timer_minutes` as the dialog's default.

### 3. Sources, Swift side

- `List.swift`: each input gets `"uid"`; a new `"processes"` array of `{pid, bundle, name, playing}` from the process object list (`name` from `NSRunningApplication`).
- `main.swift`: `mic [--device UID]`, `system [--bundle ID]…` (repeatable); the parsed command carries them; `ArgsTests` cover both.
- `Mic.swift`: with a device UID, look the device up (`kAudioHardwarePropertyTranslateUIDToDevice`) and set it on the input unit before `installTap`; an unknown UID exits 5 with the UID on stderr, and the app falls back to the default by clearing the setting after a toast.
- `SystemTap.swift`: with bundle ids, resolve them to process objects and use `stereoMixdownOfProcesses`; listen for `kAudioHardwarePropertyProcessObjectList` and rebuild the tap when the resolved set changes, so an app launched mid-meeting is heard.

### 4. Sources, Rust side

- `settings.rs`: `mic_device: Option<String>` (UID), `computer_sources: Vec<String>` (bundle ids, empty means all).
- `audio.rs`: `capture_args` takes a `Selection` (device UID, bundle ids) and appends the flags; `Source::restart()` kills the running child so the loop respawns with the new arguments at once instead of after its 1 s sleep.
- `helper.rs`: `list_info` parses `uid` and `processes`.
- Settings › Audio: Microphone `adw::ComboRow` ("System default" plus each input by name), Computer audio `adw::ComboRow` ("All apps", "Only chosen apps") and, when chosen, one `adw::SwitchRow` per running audio process, plus rows for saved bundle ids that are not running now ("not running"), so a saved Zoom is not lost between calls.

## Verify

1. Storage: record a 10 s meeting, quit during transcription to leave staging behind, open Settings › Storage: the cache row names one unfinished recording; clear it; the meetings folder is untouched (`ls`), the socket still answers `momr watch`.
2. Timer: "Stop after 1 minute": the status line counts down, a toast at 0:60, the recording stops itself and transcribes. "Start at" two minutes ahead: the recording starts on its own while the window sits on the ready page.
3. Sources: pick a second microphone in Settings: the mic meter follows it within two seconds. Choose "Only chosen apps" with Music: the computer meter moves for Music and stays flat for Safari.
4. `cargo test`; `swift test --package-path helpers/momr-audio`.

Coded 2026-09-25 in a Linux container: the cleanup rules, the timer
arithmetic, the capture flags and the restart are covered by `cargo test`;
the Swift changes (`Devices.swift`, `--device`, `--bundle`, the rebuilt tap)
have not been compiled, since the container has no Swift toolchain. The
first macOS session runs `swift build` and `swift test`, then the Verify
list.

## Status

- [x] Step 1 `cleanup.rs` and Storage page
- [x] Step 2 `timer.rs` and the Timer dialog
- [x] Step 3 Swift: `--device`, `--bundle`, richer `list`
- [x] Step 4 Rust: selection, restart, Audio page
