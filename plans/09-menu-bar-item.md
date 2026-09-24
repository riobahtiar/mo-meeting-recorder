# 09 Menu bar item

## Goal

While a meeting is being recorded, the menu bar shows a pulsing dot, the elapsed time, a small two-lane waveform (mic above, computer below), "paused" when paused, and the progress while transcribing. Clicking it brings the window back; a menu offers Pause, Stop and Compact.

## Done when

- [ ] Start a recording: the item appears within a second with a live waveform and clock; stop: it goes back to idle or disappears.
- [ ] Pause from the item pauses the app and the item says "paused 01:23".
- [ ] While transcribing the item shows the percentage; when done it shows a check for a few seconds.
- [ ] The item survives app restarts (it reconnects) and disappears when the app quits, if launched by the app.

## Prerequisites

Plan 03 (real levels). Plan 08 for launching the native item from the bundle.

## Background

`ipc.rs` serves the state on a Unix socket and `momr watch` relays it as NDJSON, one object per line, 20 times a second while recording:

```json
{"state":"recording","elapsed":754,"title":"Weekly","mic":0.62,"computer":0.31,"progress":0.0}
```

`state` is `idle`, `recording`, `paused`, `stopping`, `transcribing`, `done`, or `off` when the app is not running. A client may write `start`, `stop`, `pause` or `compact` on the same socket, one per line, which is what `momr pause` does. The original bar widget (upstream's `plugin/Widget.qml`, in this repository's history before the rename) is the reference for how the levels were drawn.

GTK has no `NSStatusItem`, so the item lives outside the Rust binary (D16).

## Steps

### 1. SwiftBar streaming plugin (quick win)

[SwiftBar](https://github.com/swiftbar/SwiftBar) runs scripts as menu bar items and supports **streaming** plugins: the script keeps running and every `~~~`-separated block replaces the item. `extras/swiftbar/momr.stream.sh`:

```bash
#!/bin/bash
# <swiftbar.type>streaming</swiftbar.type>
# <swiftbar.hideAbout>true</swiftbar.hideAbout>
momr watch | while read -r line; do
  state=$(jq -r .state <<<"$line"); elapsed=$(jq -r .elapsed <<<"$line")
  mic=$(jq -r .mic <<<"$line"); computer=$(jq -r .computer <<<"$line"); progress=$(jq -r .progress <<<"$line")
  case "$state" in
    off|idle|done) echo "~~~"; echo "" ;;                                  # hidden
    recording)     printf '~~~\n● %02d:%02d %s\n---\nPause | bash=momr param1=pause terminal=false\nStop | bash=momr param1=stop terminal=false\n' $((elapsed/60)) $((elapsed%60)) "$(bars "$mic" "$computer")" ;;
    paused)        printf '~~~\n❚❚ paused %02d:%02d\n---\nResume | bash=momr param1=pause terminal=false\n' $((elapsed/60)) $((elapsed%60)) ;;
    transcribing)  printf '~~~\n⟳ %d%%\n' "$(awk "BEGIN{print int($progress*100)}")" ;;
  esac
done
```

`bars` maps the two levels to Unicode block characters (`▁▂▃▄▅▆▇█`) so the item has a tiny meter. Throttle to 5 updates a second with a counter to keep SwiftBar calm. Document it in the README under an "Optional" heading; the user installs SwiftBar and links the script into its plugin folder.

### 2. Native status item: `momr-menubar`

A second executable target in the Swift package (rename the package to `helpers/momr-helpers` with two products, or keep it as `helpers/momr-audio` with a second target): an `LSUIElement` (no Dock icon) that:

- connects to the socket path the app uses (same rule as `ipc.rs` `socket_path()`: `~/Library/Caches/momr.sock`, or `$XDG_RUNTIME_DIR` when set), retries every second while the app is off;
- keeps a 3-second history of `mic` and `computer` and draws them in a `NSStatusItem` custom view: 60 × 18 pt, mic above the midline and computer below, in the accent colour, a red dot pulsing at 1 Hz, the time in monospaced digits (`NSFont.monospacedDigitSystemFont`);
- shows a menu on click: Show MOM Recorder (`momr compact` when compact, else activates the app through `NSRunningApplication`), Pause/Resume, Stop, and Quit Item;
- hides itself (`isVisible = false`) when the state is `off` or `idle`, unless an "Always show" default is set.

Draw with `NSBezierPath` in `draw(_:)` of an `NSView` set as the status item's `button.subviews`, or render an `NSImage` each tick and set it as the button image (simpler, and template images pick up light and dark).

### 3. Launch from the app

`ui.rs` startup spawns `momr-menubar` if found next to the executable (plan 06 lookup), and kills it on shutdown. A `menubar = false` key in `config.toml` disables it. Later, a Preferences toggle (plan 07 step 9) writes that key.

### 4. Bundle

Plan 08's script copies `momr-menubar` into `Contents/MacOS` and signs it. It needs no usage strings.

## Verify

1. Link the SwiftBar script; start a recording; the item shows a clock and bars; Pause from the item pauses the app.
2. Native item: same, plus the waveform is drawn and the dot pulses; the item vanishes on quit.
3. `kill -9` the app: the item goes to hidden within a second (`watch` prints `off`), and reappears on the next start.

## Risks and notes

- `jq` per line in the shell script is fine at 5 Hz; at 20 Hz it is not. Throttle.
- The socket protocol is shared with upstream's widget; add fields if needed but never rename or remove them, so a meeting recorded here and a client written there still understand each other.

## Status

- [ ] Step 1 SwiftBar script
- [ ] Step 2 native item
- [ ] Step 3 launched by the app
- [ ] Step 4 in the bundle
