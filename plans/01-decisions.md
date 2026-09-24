# 01 Decisions

Each entry: the context, the decision, what was rejected and why, and what follows from it. Status is **Accepted** (implement as written), **Proposed** (implement unless the maintainer objects; confirm in the pull request) or **Open** (needs a call before the step that depends on it).

## D01 Keep GTK 4 and libadwaita for the first release

**Context.** The UI is 3300 lines of GTK in `ui.rs` plus a Cairo animation and player. GTK 4 runs on macOS with a native menu bar, native file panels, HiDPI and system appearance.
**Decision.** Port the existing UI. Make it feel native through the chrome (plan 07) rather than by replacing it.
**Rejected.** A SwiftUI rewrite first: it would rebuild the UI before the platform seams are even proven. It stays available as plan 12.
**Consequences.** Some Mac idioms are out of reach (Liquid Glass, NSToolbar, full VoiceOver). Plan 12 defines when that matters enough.
**Status.** Accepted.

## D02 macOS only; Linux code is removed, not gated

**Context.** The app came from a Linux distribution and every seam has a Linux implementation. Keeping both behind `cfg(target_os)` would preserve a Linux build nobody in this project ships or tests.
**Decision.** MOM Recorder targets macOS. When a seam's macOS replacement lands, the Linux code at that seam is deleted in the same change. Upstream is the origin: fixes to the shared core are cherry-picked when they still apply.
**Rejected.** Dual-platform with `cfg` gating: double the test surface, and Omarchy integrations (theme files, bar widget, default-agent command, Hyprland calls) would linger as "Linux paths".
**Consequences.** Simpler code, one CI target. Meetings stay file-compatible with upstream (D13) so users can move recordings.
**Status.** Accepted.

## D03 Capture stays a child process writing raw s16le 48 kHz stereo to stdout

**Context.** `audio.rs` reads 20 ms chunks from a child's stdout, keeps a level history, tees to the staging file, and restarts the child when it exits. `ui.rs` only knows `Source`.
**Decision.** Every macOS capture path honours the same contract. `Source::spawn` takes a `Device` enum, and one function builds the `Command` per device.
**Rejected.** In-process capture with `cpal` or `coreaudio-rs`: Core Audio callbacks inside the app, and the crash isolation of a child process is lost.
**Consequences.** One Swift helper binary (`momr-audio`) ships with the app.
**Status.** Accepted.

## D04 Microphone through ffmpeg first, then through the helper

**Context.** Homebrew's ffmpeg has the `avfoundation` input; `-i ":default"` selects the default microphone (confirmed in `libavdevice/avfoundation.m`). It binds to the device at start and does not follow a default-device change, which `parec @DEFAULT_SOURCE@` did.
**Decision.** Ship the ffmpeg path to get a moving meter with no new code. Once `momr-audio` exists for the tap, add a `mic` subcommand that follows the default input device and switch to it.
**Status.** Accepted.

## D05 Computer audio through a Core Audio process tap, BlackHole as fallback

**Context.** macOS has no monitor source. Since 14.2 an app can create a process tap (`AudioHardwareCreateProcessTap`) on all processes and read it through a private aggregate device. Before that, or when permission is refused, a loopback virtual device such as BlackHole works with a Multi-Output Device.
**Decision.** Tap first, in the Swift helper. If the tap cannot be created, the helper exits with a distinct code and the app tries a BlackHole device by name, and tells the user on the ready page what to install if neither works.
**Rejected.** ScreenCaptureKit audio: needs screen-recording permission, heavier, and its audio is a side channel of screen capture.
**Status.** Accepted.

## D06 Playback through `ffmpeg -f audiotoolbox -`

**Context.** Linux piped ffmpeg into `pacat`. Homebrew ffmpeg has the `audiotoolbox` output device; piping raw s16le into `ffmpeg -f s16le -i - -f audiotoolbox -` was exercised on this machine and works.
**Decision.** One ffmpeg process decodes and plays. `Playback` holds one child.
**Status.** Accepted.

## D07 Die-with-parent through a wrapper, since `prctl` does not exist

**Context.** `player.rs` `die_with_parent()` set `PR_SET_PDEATHSIG` so a meeting stops playing when the app dies, even by crash. macOS has no equivalent flag.
**Decision.** `momr-audio run -- <program> <args>` spawns the program, watches the parent pid with kqueue `EVFILT_PROC`/`NOTE_EXIT`, and kills the child when the parent goes. `die_with_parent` becomes "wrap in `momr-audio run`".
**Rejected.** Relying on `Drop` alone: fine for a clean exit, useless after a crash, which is exactly the case upstream guards against.
**Status.** Accepted.

## D08 GLib directories everywhere

**Context.** Three modules hand-roll `XDG_*` lookups with `~/.local/…` fallbacks. `glib::user_data_dir()` and friends return `~/Library/Application Support` and `~/Library/Caches` on macOS (Homebrew's GLib is built with Cocoa support) and still honour `XDG_*` when set.
**Decision.** One `paths.rs` wraps GLib; the hand-rolled copies go. The folder name under each is `momr`.
**Status.** Accepted.

## D09 Agent chosen by a `config.toml` key

**Context.** `agent.rs` asks `omarchy-default-agent` for the id. That command does not exist here.
**Decision.** `agent = "claude"` in `config.toml` (the file that already holds `model`). The Omarchy lookup and its message are removed. The per-agent flag table and the no-tools boundary do not change.
**Status.** Accepted.

## D10 `<Primary>` accelerators; compact on ⇧⌘M

**Context.** GTK maps `<Primary>` to ⌘ on macOS. ⌘M is Minimize in every Mac app once the app has a Window menu.
**Decision.** Replace `<Control>` with `<Primary>`. The compact accelerator is `<Primary><Shift>m`.
**Status.** Accepted.

## D11 Native menu bar

**Context.** GTK 4's quartz backend turns the `GMenuModel` from `set_menubar` into the NSMenu bar and adds the standard app menu (About, Preferences, Services, Hide, Quit) when `app.about`, `app.preferences` and `app.quit` exist.
**Decision.** Build the menu model and call `set_menubar`. Every menu item is a `GAction`; buttons in the UI trigger the same actions so enabled state is shared.
**Status.** Accepted.

## D12 macOS look through a CSS layer and Apple's palette, not a new widget set

**Context.** libadwaita's structure (header bar, sidebar, sheets, toasts) maps well to macOS. Its details (button radii, spacing, button glyphs on the right) do not.
**Decision.** A `.macos` style class on the window and a `macos.css` provider adjust radii, spacing and window buttons. `theme.rs` supplies Apple system colours for speakers, waves and the animation, following light and dark. No attempt at translucency or Liquid Glass.
**Status.** Accepted.

## D13 Identity: MOM Recorder, MOMR, `momr`; the meeting format stays upstream's

**Context.** The maintainer named the app **MOM Recorder**, short **MOMR**. The code carried Omarchy's names in the crate, the app id, the slug, packaging and a bar plugin.
**Decision.** Display name "MOM Recorder"; "MOMR" where space is short; `momr` for the crate, binary, helper prefix (`momr-audio`, `momr-menubar`), folders under `~/Library` and the socket (`momr.sock`). App id and bundle identifier `io.github.riobahtiar.MOMRecorder`, a namespace the maintainer controls through GitHub; a domain-based id can replace it in plan 11 **before the first DMG**, since the bundle id is also the TCC identity. UTI `io.github.riobahtiar.momr.meeting`. The file extension `.meeting-recorder`, the manifest JSON and `transcript.md` stay exactly as upstream: they are descriptive, not branded, and keep meetings portable. Done in code for crate, app id and slug; Omarchy-only files (pacman packaging, installer, bar plugin, desktop entry, MIME type) deleted.
**Rejected.** `.momr` as the extension: breaks opening upstream meetings for no gain.
**Status.** Accepted; the bundle id is Open until plan 11 confirms it.

## D14 Homebrew formula first, signed DMG second

**Context.** A formula gets GTK, libadwaita, ffmpeg and cmake for free and is the fastest path to a clean install. A DMG needs bundled dylibs, signing and notarization.
**Decision.** Ship the formula `momr` in a personal tap as soon as phase 1 works. Build the DMG in plan 08 after it.
**Status.** Accepted.

## D15 The helper is a Swift package, built by a script, found next to the executable

**Context.** Core Audio taps and AVAudioEngine are Objective-C/Swift APIs. Binding them from Rust means a large dependency tree for a few hundred lines.
**Decision.** `helpers/momr-audio` is a Swift package. `scripts/build-macos.sh` builds it and places it next to the Rust binary; the app looks in its own directory, then in the bundle's `Contents/MacOS`, then on `PATH`. Cargo stays pure Rust.
**Status.** Accepted.

## D16 Menu bar item outside the Rust binary

**Context.** GTK has no `NSStatusItem`. The app already publishes NDJSON through `watch`.
**Decision.** First a SwiftBar streaming plugin script; then `momr-menubar`, a small Swift status item launched by the app. Both consume `watch` and send `start`, `stop`, `pause`, `compact` over the socket.
**Status.** Accepted.

## D17 Whisper on Metal as an opt-in feature; CoreML deferred; Vulkan removed

**Context.** whisper-rs 0.16 exposes `metal` and `coreml`. Metal needs nothing extra; CoreML needs a separately converted encoder model per whisper model. The `vulkan` feature was for Linux GPUs.
**Decision.** Add `metal = ["whisper-rs/metal"]` with the same comment style. Remove `vulkan`. CoreML waits for a request.
**Status.** Accepted.

## D18 Shortcut table

**Context.** Upstream had Ctrl+M compact, Ctrl+W close, Ctrl+Q quit, Enter copies the transcript. macOS users expect a full menu with shortcuts.
**Decision.** Proposed table; confirm before implementing plan 07 step 1.

| Action | Shortcut | Note |
|---|---|---|
| New Recording | ⌘N | From the done page |
| Open Meeting… | ⌘O | Folder or `.meeting-recorder` file |
| Import Audio File… | ⇧⌘I | |
| Reveal in Finder | ⌥⌘R | The meeting folder |
| Close Window | ⌘W | Asks while recording or transcribing |
| Start Recording | ⌘R | |
| Pause / Resume | ⇧⌘R | |
| Stop Recording | ⌘. | ⌘. is the macOS "stop" idiom |
| Compact Strip | ⇧⌘M | ⌘M stays Minimize |
| Copy Transcript | ⇧⌘C | Enter keeps working, as upstream |
| Enter Full Screen | ⌃⌘F | Standard |
| Preferences… | ⌘, | Standard |
| Quit | ⌘Q | Standard |

**Status.** Proposed.

## D19 Two architecture-specific DMGs, no universal binary

**Context.** Homebrew dylibs are built per architecture; a universal app would need both trees merged with `lipo` for every library.
**Decision.** Build arm64 on `macos-14` and x86_64 on `macos-13` runners and publish two DMGs.
**Status.** Accepted.
