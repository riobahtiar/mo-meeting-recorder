# 03 Audio capture

## Goal

On the ready page the microphone meter moves when you speak and the computer meter moves when the Mac plays sound. A recording writes both raw tracks to the staging directory and, on stop, the upstream meeting folder layout, with "You" and "Remote" told apart by the two tracks. `parec` is gone.

## Done when

- [ ] Both meters move on the ready page, from a terminal launch.
- [ ] A 30-second recording with a video playing produces `.tracks/mic.ogg` and `.tracks/computer.ogg` of equal length (`ffprobe`), and a transcript with lines from both You and Remote.
- [ ] Switching the default input or output device during a recording keeps both tracks going (helper path).
- [ ] With the tap unavailable, the ready page says what to install (BlackHole) and the mic still records.
- [ ] `cargo test` covers the command construction per device.
- [ ] `grep -n parec src/` finds nothing.

## Prerequisites

Plan 02.

## Background

`audio.rs` runs one `parec` per source for the life of the app. `capture()` spawns it with `--raw --format=s16le --rate=48000 --channels=2 --latency-msec=20 -d <device>`, reads `CHUNK_BYTES` (20 ms) at a time, records the peak for the meter, and when recording tees the bytes into the staging file. When the child exits, the outer loop in `Source::spawn` restarts it a second later. The contract every replacement must keep: **a child process, raw interleaved s16le, 48 000 Hz, 2 channels, on stdout, until killed.**

`ui.rs` creates the two sources with the PulseAudio names `@DEFAULT_SOURCE@` and `@DEFAULT_MONITOR@` (around line 208). Nothing else in the app knows about devices.

macOS has no monitor source. Since 14.2, an app can create a **process tap** on the audio of all processes and read it through a private **aggregate device**; the OS asks once for "System Audio Recording" permission. Before 14.2, or when refused, a loopback device such as [BlackHole](https://github.com/ExistentialAudio/BlackHole) receives whatever the user routes to it through a Multi-Output Device, and can be captured like a microphone.

For the microphone, Homebrew's ffmpeg has the `avfoundation` input device, and `-i ":default"` picks the default input (confirmed in `libavdevice/avfoundation.m`). It binds to the device at start, so a headset plugged in mid-call is not followed until the process restarts. The Swift helper fixes that later in this plan.

Reference implementation for the tap: [insidegui/AudioCap](https://github.com/insidegui/AudioCap) (MIT), a small Swift app that does exactly this.

## Steps

### 1. A `Device` enum in place of PulseAudio strings

In `audio.rs`:

```rust
// sketch
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    /// The default microphone.
    Mic,
    /// What the computer plays.
    Computer,
}

impl Source {
    pub fn spawn(device: Device) -> Self { /* as today, passing `device` to capture */ }
}
```

In `ui.rs` change the two call sites to `Source::spawn(Device::Mic)` and `Source::spawn(Device::Computer)`. That is the only change to `ui.rs` in this plan besides the banner in step 7.

### 2. Command builders, testable

A pure function returns `(program, args)` so tests can check it; `capture()` turns it into a `Command`:

```rust
// sketch
pub fn capture_args(device: Device, helper: Option<&Path>) -> (String, Vec<String>) {
    match (device, helper) {
        (Device::Mic, _) => ("ffmpeg".into(), vec![
            "-hide_banner".into(), "-loglevel".into(), "error".into(), "-nostdin".into(),
            "-f".into(), "avfoundation".into(), "-i".into(), ":default".into(),
            "-f".into(), "s16le".into(), "-ar".into(), RATE.to_string(), "-ac".into(), CHANNELS.to_string(), "-".into()]),
        (Device::Computer, Some(helper)) => (helper.display().to_string(), vec![
            "system".into(), "--rate".into(), RATE.to_string(), "--channels".into(), CHANNELS.to_string()]),
        (Device::Computer, None) => blackhole_args(),   // step 6
    }
}
```

Delete the `parec` invocation and rewrite the module doc: capture is ffmpeg and `momr-audio`, not `parec`.

### 3. Microphone through ffmpeg (fast path)

Build, run, speak: the mic meter should move. First launch triggers macOS's microphone prompt for the *terminal app* (the responsible process). If the meter stays flat with no prompt, check System Settings › Privacy & Security › Microphone for the terminal.

Notes:
- ffmpeg's avfoundation delivers buffers of a few hundred samples; `read_exact` of 20 ms chunks smooths that.
- If `:default` fails on some ffmpeg build, `ffmpeg -f avfoundation -list_devices true -i ""` lists indices; `-audio_device_index N` selects one. Verify: `:default` works with the Homebrew build installed here.
- ffmpeg exits if the device disappears; the existing restart loop covers it.

### 4. The helper: `helpers/momr-audio`

A Swift package, macOS 14 minimum, one executable target `momr-audio` with subcommands:

| Subcommand | Does |
|---|---|
| `list` | Prints input devices, output devices and whether a process tap can be created, as JSON |
| `mic [--rate 48000] [--channels 2]` | Default microphone through `AVAudioEngine`, converted to interleaved s16le, follows default-device changes |
| `system [--rate 48000] [--channels 2]` | Process tap on all processes through a private aggregate device, converted to interleaved s16le |
| `run -- <program> <args…>` | Plan 04: runs a program and kills it when the parent exits |

Exit codes: `0` normal end (stdout closed), `2` bad arguments, `3` tap unsupported on this macOS, `4` permission denied, `5` no device. `audio.rs` reads the code to decide on the BlackHole fallback and the banner.

Layout:

```
helpers/momr-audio/
├── Package.swift            // swift-tools-version 5.9, platforms: [.macOS(.v14)]
└── Sources/momr-audio/
    ├── main.swift           // argument parsing (hand-rolled; no ArgumentParser dependency), dispatch
    ├── Output.swift         // s16le conversion, resampling with AVAudioConverter, write to stdout, SIGPIPE → exit 0
    ├── Mic.swift            // AVAudioEngine input tap, restart on AVAudioEngineConfigurationChange
    ├── SystemTap.swift      // CATapDescription, aggregate device, IOProc
    └── Run.swift            // plan 04
```

`SystemTap.swift`, in outline (the AudioCap repository shows every call in context):

```swift
// sketch
let desc = CATapDescription(stereoGlobalTapButExcludeProcesses: [])
desc.name = "MOM Recorder"
desc.isPrivate = true
desc.muteBehavior = .unmuted
var tapID = AudioObjectID(kAudioObjectUnknown)
try check(AudioHardwareCreateProcessTap(desc, &tapID))          // OSStatus; permission prompt here

let aggregate: [String: Any] = [
    kAudioAggregateDeviceNameKey: "MOM Recorder Tap",
    kAudioAggregateDeviceUIDKey: UUID().uuidString,
    kAudioAggregateDeviceIsPrivateKey: true,
    kAudioAggregateDeviceTapAutoStartKey: true,
    kAudioAggregateDeviceTapListKey: [[kAudioSubTapUIDKey: desc.uuid.uuidString]],
]
var aggregateID = AudioObjectID(kAudioObjectUnknown)
try check(AudioHardwareCreateAggregateDevice(aggregate as CFDictionary, &aggregateID))

// The tap's stream format (kAudioTapPropertyFormat on tapID) is Float32 at the output device's rate.
var procID: AudioDeviceIOProcID?
try check(AudioDeviceCreateIOProcIDWithBlock(&procID, aggregateID, nil) { _, input, _, _, _ in
    output.write(convert(input))   // Float32 → Int16 interleaved, resample to --rate when the rates differ
})
try check(AudioDeviceStart(aggregateID, procID))
RunLoop.main.run()
```

Cleanup on exit: `AudioDeviceStop`, `AudioDeviceDestroyIOProcID`, `AudioHardwareDestroyAggregateDevice`, `AudioHardwareDestroyProcessTap`. Install a `SIGTERM` handler that does this and exits, because the app kills the child on quit.

Behaviours to get right:
- **Sample rate.** The tap runs at the output device's rate (44.1 or 48 kHz; Bluetooth can change it). Always resample to `--rate` with `AVAudioConverter`, so the app's contract holds.
- **Format.** The tap may hand non-interleaved Float32. Interleave and clamp to Int16.
- **Silence.** When nothing plays, the IOProc still fires with zeros. Good: the meter shows a flat line, not a stalled one.
- **Default output changes.** A global tap follows the mix, not one device. Verify: audio keeps flowing after switching output to a headset. If it does not, listen for `kAudioHardwarePropertyDefaultOutputDevice` and rebuild the tap.
- **Permission.** The first `AudioHardwareCreateProcessTap` triggers the System Audio Recording prompt for the responsible process. Refusal shows as an `OSStatus` error; map it to exit code 4.
- **Unsupported.** `CATapDescription` is unavailable before 14.2; guard with `if #available(macOS 14.2, *)`, otherwise exit 3.

`Mic.swift`: `AVAudioEngine`, `inputNode.installTap`, convert to the requested format, write. Observe `AVAudioEngineConfigurationChange` and restart the engine so a new default microphone is followed. When this works, switch `Device::Mic` to the helper (D04) and keep ffmpeg as the fallback when the helper is missing.

`Output.swift`: a single writer that converts to Int16 and writes with `fwrite` to `stdout`; `signal(SIGPIPE, SIG_IGN)` and treat a write error as "the app went away", exit 0.

### 5. Build script and locating the helper

`scripts/build-macos.sh`:

```bash
#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --release "$@"
swift build -c release --package-path helpers/momr-audio
cp helpers/momr-audio/.build/release/momr-audio target/release/momr-audio
```

In Rust, a `helper::path()` function looks for `momr-audio` next to `std::env::current_exe()`, then in `../MacOS/` of an app bundle, then on `PATH`; when none is found, `Device::Computer` gets the BlackHole command and the ready page shows the banner from step 7.

### 6. BlackHole fallback

When the helper exits with code 3 or 4, or is missing, try `ffmpeg -f avfoundation -i ":BlackHole 2ch"` (device name match; also accept `BlackHole 16ch` and `BlackHole 64ch`). `momr-audio list` tells whether such a device exists. If it does not, keep the computer source idle and let the banner explain.

### 7. Banner on the ready page

`Source` gains a `status()` returning `Ok` or a short reason (`"System Audio Recording permission was refused"`, `"Install BlackHole to record the computer audio on this version of macOS"`, `"momr-audio helper not found"`). `ui.rs` shows it under the computer meter, the way the model banner is shown. Keep the texts in `audio.rs` so the platform knowledge stays there.

### 8. Echo and levelling

Unchanged: both tracks are levelled at export (`export.rs`), and speaker attribution reads the louder track (`transcribe.rs`). With speakers instead of headphones the mic hears the other side quieter than the tap does. Nothing to do, but check it in Verify.

### 9. Tests

- `capture_args` for both devices, with and without a helper path.
- Exit-code mapping to fallback and banner text, with the spawn taken as a closure.
- In the Swift package, a small unit test for the Float32 → Int16 conversion and interleaving.

## Verify

1. `scripts/build-macos.sh`, then `target/release/momr-audio list` prints devices and `"tap": true` on macOS 14.2+.
2. `target/release/momr-audio system | ffmpeg -f s16le -ar 48000 -ac 2 -i - -t 5 /tmp/system.wav` while a video plays; `afplay /tmp/system.wav` plays the video's sound.
3. `target/release/momr`: both meters move. Record 30 seconds with a video playing and yourself talking. Stop. In the meeting folder: `ffprobe .tracks/mic.ogg` and `.tracks/computer.ogg` have equal duration; `transcript.md` has You and Remote lines.
4. Plug in or switch to a headset mid-recording (helper path): both tracks continue; the computer track has no gap longer than a second.
5. Deny System Audio Recording once (System Settings), relaunch: banner shows; mic records.

## Risks and notes

- The tap gives the mix *after* per-app volume, at output level; a muted Mac means a silent computer track. Say so in the README's troubleshooting when the time comes.
- A helper started from a terminal inherits the terminal's TCC identity for both prompts. Once the app is a bundle (plan 08) the prompts name MOM Recorder and carry the `Info.plist` usage strings.
- `swift build` needs the full Xcode or the command line tools with a matching SDK. CI (plan 10) uses the `macos-14` runner, which has both.

## Status

- [ ] Step 1 `Device` enum
- [ ] Step 2 command builders with tests, `parec` gone
- [ ] Step 3 mic via ffmpeg moves the meter
- [ ] Step 4 helper: `list`, `system`, `mic`
- [ ] Step 5 build script and lookup
- [ ] Step 6 BlackHole fallback
- [ ] Step 7 banner
- [ ] Step 8 echo check
- [ ] Step 9 tests
