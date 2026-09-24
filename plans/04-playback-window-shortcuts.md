# 04 Playback, window and shortcuts

## Goal

A saved meeting plays back and seeks; every child process the app starts dies with it, even after a crash; the compact strip works without Hyprland; the shortcuts use ⌘. `pacat` and `hyprctl` are gone.

## Done when

- [ ] Open a meeting, press play, click in the waveform: sound follows, the playhead moves, the transcript scrolls.
- [ ] Quit while playing: sound stops at once. `kill -9` the app while playing: sound stops within a second.
- [ ] ⇧⌘M shrinks the window to the strip and back; the strip can be dragged; ⌘W and ⌘Q ask while recording.
- [ ] Recording keeps the Mac from idle-sleeping.
- [ ] `grep -n "pacat\|hyprctl" src/` finds nothing.

## Prerequisites

Plan 02. The `run` subcommand lands in the helper from plan 03, but this plan can start with the ffmpeg change while the helper is being written.

## Background

`player.rs` `Playback::start()` spawns `ffmpeg` (decode, seek with `-ss`, mix several files with `amix`) piping raw s16le into `pacat --playback`. `Playback` holds both children, `ended()` polls `pacat`, `Drop` kills both. `die_with_parent()` (stubbed in plan 02) set `PR_SET_PDEATHSIG` on each child so the meeting could not keep playing after a crash.

Homebrew's ffmpeg has the `audiotoolbox` output device. `… -f s16le -ar 48000 -ac 2 -i - -f audiotoolbox -` played on this machine, so a single ffmpeg can decode and play.

`ui.rs` `set_compact()` toggles the strip and calls `hyprctl_dispatch` to resize and re-centre a Hyprland floating window (around lines 2895 to 2950: `hyprctl_json`, `hyprctl_dispatch`, `own_window`, `on_screen`). Verify: whether GTK alone resizes the mapped window on macOS, or whether it only shrinks the content inside a window that keeps its size.

Accelerators are set in `ui.rs` near line 86 with `<Control>`. GTK maps `<Primary>` to ⌘. ⌘M is Minimize in every Mac app once a Window menu exists (plan 07 adds one), hence D10.

## Steps

### 1. One-process playback

`Playback` holds one child:

```rust
// sketch
struct Playback {
    ffmpeg: Child,
    started: Instant,
    from_us: i64,
}
fn ended(&mut self) -> bool { matches!(self.ffmpeg.try_wait(), Ok(Some(_))) }
```

Build the ffmpeg command as today up to the input and filter arguments, then `ffmpeg.args(["-f", "audiotoolbox", "-"]).stdout(Stdio::null())`. Delete the `pacat` spawn and the `-f s16le` output arguments. `-nostdin` stays so ffmpeg never waits on the terminal. Verify: `-ss` before `-i` still seeks accurately with the audiotoolbox sink; if the first half-second is clipped, add `-af aresample=async=1`.

Rewrite the module doc: playback is ffmpeg to the default output through AudioToolbox; pausing stops the process and playing starts it again at the position; a meeting saved as separate files is mixed on the fly.

### 2. `momr-audio run`: die with parent

In the helper (plan 03 layout), `Run.swift`:

```swift
// sketch
// momr-audio run -- <program> <args…>
let parent = getppid()
let child = Process(); child.executableURL = resolve(program); child.arguments = args
child.standardInput = FileHandle.standardInput; child.standardOutput = FileHandle.standardOutput; child.standardError = FileHandle.standardError
try child.run()

let kq = kqueue()
var ev = kevent(ident: UInt(parent), filter: Int16(EVFILT_PROC), flags: UInt16(EV_ADD | EV_ONESHOT), fflags: UInt32(NOTE_EXIT), data: 0, udata: nil)
kevent(kq, &ev, 1, nil, 0, nil)
DispatchQueue.global().async {
    var out = kevent(); kevent(kq, nil, 0, &out, 1, nil)      // blocks until the parent exits
    kill(child.processIdentifier, SIGTERM); usleep(500_000); kill(child.processIdentifier, SIGKILL); exit(0)
}
signal(SIGTERM) { _ in kill(child.processIdentifier, SIGTERM); exit(0) }   // the app's own kill on Drop
child.waitUntilExit(); exit(child.terminationStatus)
```

`resolve(program)` walks `PATH` when the name has no slash, so `ffmpeg` resolves the same way it does from Rust.

In `player.rs`, `die_with_parent` becomes a constructor, because the wrapper has to be the program:

```rust
// sketch
/// A command for `program` whose process dies when this app does, even after a crash.
fn guarded(program: &str, helper: Option<&Path>) -> Command {
    match helper {
        Some(helper) => { let mut c = Command::new(helper); c.args(["run", "--", program]); c }
        None => Command::new(program),   // no watchdog; Drop still kills on a clean exit
    }
}
```

Use `guarded("ffmpeg", helper::path().as_deref())` in `Playback::start`. `Drop` keeps killing the outer process; the wrapper forwards `SIGTERM` to ffmpeg.

### 3. Compact strip on GTK alone

Delete `hyprctl_json`, `hyprctl_dispatch`, `own_window`, `on_screen` and the block in `set_compact` that calls them. In their place, after toggling the strip content:

```rust
// sketch, inside set_compact
let (w, h) = if compact { (STRIP_WIDTH, STRIP_HEIGHT) } else { (FULL_WIDTH, FULL_HEIGHT) };
window.set_default_size(w, h);
```

Verify: on GTK 4 macOS, `set_default_size` on a mapped window changes its size when the content's minimum allows it. If it does not shrink, `set_resizable(false)` then `true` around it forces a re-layout to the natural size. The strip has no header bar; check that it can still be dragged (`gtk::WindowHandle` around the strip is how upstream does it) and that macOS keeps it on screen.

### 4. Accelerators

```rust
// sketch
app.set_accels_for_action("window.close", &["<Primary>w"]);
app.set_accels_for_action("app.quit", &["<Primary>q"]);
app.set_accels_for_action("win.compact", &["<Primary><Shift>m"]);
```

Update the README's keyboard line in the same change. Plan 07 adds the rest of D18 with the menu.

### 5. No idle sleep while recording

macOS ships `caffeinate`. Spawn `caffeinate -i -w <our pid>` when recording starts and kill it when recording stops. Keep the `Child` in the recording state next to the staging paths.

### 6. Tests

- `guarded()` builds a wrapper command when a helper path is given and a bare command when it is not.
- The accel set for `win.compact` is `<Primary><Shift>m`.

## Verify

1. Open a meeting from the terminal: `target/release/momr ~/Documents/Meetings/<folder>`; play, seek, watch the line highlight.
2. Quit with ⌘Q while playing: silence at once.
3. `kill -9 $(pgrep -x momr)` while playing: silence within a second; `pgrep ffmpeg` shows nothing left.
4. ⇧⌘M twice; drag the strip.
5. Start a recording, `pmset -g assertions | grep -i caffeinate` shows the assertion; stop, it is gone.

## Risks and notes

- `Process` in Foundation resets signal dispositions; the `SIGTERM` handler must be installed after `run()`.
- If the audiotoolbox sink introduces noticeable start latency, `position_us()` will lead the sound. Measure by playing a click track; compensate with a constant if needed.

## Status

- [ ] Step 1 audiotoolbox playback, `pacat` gone
- [ ] Step 2 `momr-audio run` and `guarded()`
- [ ] Step 3 compact strip, `hyprctl` gone
- [ ] Step 4 accelerators
- [ ] Step 5 caffeinate
- [ ] Step 6 tests
