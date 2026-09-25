# 18 Player

## Goal

The done page's player grows from a play button beside a thin waveform into a proper audio player: the controls people expect (play and pause, back and forward, previous and next line, speed, volume and mute), a waveform that is the progress bar and the seek control, and a little life while it plays. It stays where it was, above the transcript, so it is always in view while the transcript scrolls (asked for 2026-09-26 with a screenshot; placement chosen by the maintainer).

## Done when

- [ ] The player card shows the two-lane waveform (microphone above, computer audio below) with the played part in full colour, a playhead with a knob, chapter markers, and a hover line whose tooltip says the time there (and the chapter on a marker).
- [ ] Under it: elapsed time, a legend for the two lanes, and remaining time; then previous line, back 15 s, play or pause, forward 15 s and next line, and on the right a level animation, the speed (0.5× to 2×, pitch kept) and volume with mute (0 to 150 %, a notch at 100 %). Speed and volume are kept across meetings and launches.
- [ ] While playing the playhead glides on the frame clock and the level bars follow the loudness at the playhead; paused, nothing ticks and the bars rest.
- [ ] Space plays or pauses and ← and → skip 5 s on the done page, except while typing.
- [ ] The duration is right in the bundled app (it read 00:00 when the bundled ffprobe could not start).

## Background

- Playback is one ffmpeg into audiotoolbox (`crates/momr-core/src/playback.rs`); pausing stops it and playing starts it at the position. Speed and volume are ffmpeg filters after the mix (`atempo`, which keeps pitch and takes 0.5–2×, and `volume`), always in the graph so they can change live: ffmpeg reads `c<filter> -1 <command> <value>` lines on stdin, which `momr-audio run` passes through. A first version restarted ffmpeg on every change and the slider stuttered ("volume control are bad", 2026-09-26); measured since, a live `volume 0.1` takes the level down 20 dB mid-stream with no restart, also through the wrapper. The position is the start plus the wall clock times the speed, rebased on a speed change. A restart at the position is the fallback when ffmpeg no longer listens.
- The screenshot's 00:00 / 00:00 was not the player: the bundle's `ffprobe` is signed with the hardened runtime and, ad hoc, has no Team ID, so library validation refused the bundled ffmpeg dylibs and it died at launch. `ffmpeg` has the entitlement that turns validation off; `ffprobe` did not. It now gets `packaging/macos/entitlements-tools.plist` (library validation off, no microphone), and the player falls back to the length `peaks` decodes when ffprobe gives none.
- A terminal-run dev build has no `XDG_DATA_DIRS`, so GTK finds only its built-in icons, which lack the seek and skip icons; the bundle points GTK at its own copy of every Adwaita symbolic icon. Run a dev build with `XDG_DATA_DIRS=$(brew --prefix)/share` to see them.

## Steps

### 1. Core

`playback::Sound { speed, volume }` with `SPEEDS`, `MAX_VOLUME` and `clamped`; `Playback::start(files, from_us, sound)`; the filters in the pure `args` (tested); `peaks` also returns the decoded length; `settings::{load,save}_player_sound`.

### 2. GTK player

`src/player.rs`: the card (waveform; elapsed, legend and remaining; one row on a shared centre line with the level bars and a speed menu on the left, the transport centred with a halo behind play, and mute, a thin slider and its percentage on the right), `set_lines` from the transcript for previous and next line (media-player rule: within 2 s of a line's start, previous goes to the line before), `handle_key` for the done page's capture-phase key controller in `ui.rs`, a frame-clock tick while playing for the playhead and the eased level bars. `macos.css` sizes the round play button and the controls.

### 3. Bundle

`bundle-macos.sh` signs `ffprobe` with `entitlements-tools.plist`.

## Verify

1. Open a meeting: the duration is right; hover the waveform: the line and the time follow the pointer.
2. Play: the playhead glides, the level bars move with the voices and rest in silence; pause: they settle.
3. 1.5×: the voices keep their pitch and the clock runs faster; volume to 0: the icon shows muted; Mute again restores the level; relaunch: speed and volume are kept.
4. Previous and next line jump between transcript lines, and the current line follows; Space, ← and → work unless a text field has the focus.
5. In the bundled app: `Contents/MacOS/ffprobe -version` runs.

## Status

- [x] Step 1 core (tested: filter shapes, clamping)
- [x] Step 2 GTK player (tested: line steps, speed labels; seen on screen with an invented meeting, paused: layout, icons, legend, round play button, the aligned row after the second pass; playing is Verify 2–4)
- [x] Step 3 bundle (Verify 5 seen 2026-09-26: the bundled ffprobe runs and reads durations)
