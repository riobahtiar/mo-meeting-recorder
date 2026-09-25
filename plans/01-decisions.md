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
**Status.** Superseded by D22. Measured 2026-09-25, Homebrew's GLib 2.90 has no Cocoa support and returns the Linux `~/.local` paths, so `paths.rs` builds the `~/Library` locations itself. The single `paths.rs` and the `momr` folder name stand.

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
**Status.** Accepted; the bundle id is Open until plan 11 confirms it.

Confirmed 2026-09-25 (plan 11): the id stays
`io.github.riobahtiar.MOMRecorder` with UTI
`io.github.riobahtiar.momr.meeting`, and the Application Support folder stays
`momr`. No domain-based id, no rename.

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

**Status.** Accepted; implemented as proposed in plan 07 (menu shortcuts match this table).

## D19 Two architecture-specific DMGs, no universal binary

**Context.** Homebrew dylibs are built per architecture; a universal app would need both trees merged with `lipo` for every library.
**Decision.** Build arm64 on `macos-14` and x86_64 on `macos-13` runners and publish two DMGs.
**Status.** Accepted.

## D20 Follow Apple's Liquid Glass design language within the GTK shell

**Context.** Apple introduced Liquid Glass as the platform material ([overview](https://developer.apple.com/documentation/technologyoverviews/liquid-glass), [adoption guide](https://developer.apple.com/documentation/technologyoverviews/adopting-liquid-glass)), and the [Human Interface Guidelines](https://developer.apple.com/design/human-interface-guidelines) now describe glass materials, rounder concentric controls, layered icons and edge-to-edge content as the platform look. D12 said "no attempt at translucency or Liquid Glass".
**Decision.** The macOS chrome (plan 07) follows the Liquid Glass-era HIG: a translucent header bar and sidebar treatment, concentric corner radii, the system palette in light and dark with Reduce Transparency respected, and a layered app icon composed in Icon Composer (plan 07 step 10 already allows `.icon`). Standard AppKit and SwiftUI components adopt the material automatically when built with the latest SDK; GTK widgets do not, so the GTK shell approximates with CSS alpha layering and never fakes refraction or blur it cannot render.
**Rejected.** Pixel-perfect Liquid Glass inside GTK: the real material (`NSGlassEffectView`, glass button styles, scroll-edge effects) is AppKit and SwiftUI only. Chasing it in Cairo and CSS would produce an imitation that breaks under Reduce Transparency.
**Consequences.** Plan 07 carries explicit Liquid Glass acceptance notes (translucency that degrades gracefully, icon layers); whether the approximation satisfies is judged by its side-by-side verify, and falling short routes to the plan 12 native shell, whose criteria gain a Liquid Glass line.
**Status.** Accepted.

## D21 Optional transcription providers; English and Indonesian UI

**Context.** After the core port, MOM Recorder should offer cloud speech-to-text alongside the built-in local transcription, and the UI should read in English and Indonesian. The first sketch also named the omnilingual-asr model.
**Decision.** Local whisper.cpp stays the default, and a provider that sends audio out runs only on explicit opt-in: the provider picker in Preferences, or `transcribe-file --provider` for one run. Plan 13 shipped three providers: ElevenLabs speech-to-text, Google Cloud Speech-to-Text, and OpenRouter's audio transcriptions endpoint, whose model is `openrouter_model` in `config.toml` (`openai/whisper-1` unless set). API keys live in the macOS Keychain, one service per provider (`momr-elevenlabs`, `momr-google`, `momr-openrouter`), written through `security -i` with the command on stdin so the key never appears in a process's argv where `ps` could read it; `config.toml` only names the provider. Preferences gives each provider a password row, short instructions naming where the key comes from, and a line saying what leaves the Mac. UI strings go through `locales.rs`, English and Indonesian, switched in Preferences.
**Rejected.** Replacing the local default; keys in config files, where they end up in backups and dotfile repositories; omnilingual-asr, because it is a Python fairseq2 research stack with no local runtime this app can ship (plan 13 revisits it if an ONNX or CoreML export appears).
**Consequences.** The README privacy section names what each provider receives. Cloud chunks number their speakers independently, which the README states rather than hides. Transcript content stays English (D23).
**Status.** Accepted; implemented in plan 13.

## D22 `paths.rs` builds `~/Library` locations itself

**Context.** D08 said one `paths.rs` wraps GLib, whose `user_data_dir()` and
friends return `~/Library/Application Support` and `~/Library/Caches` on
macOS. Measured 2026-09-25 with Homebrew GLib 2.90 and no `XDG_*` set, they
return `~/.local/share`, `~/.config`, `~/.cache` and `~/.local/state`:
Linux paths, no Cocoa support. Only `user_special_dir(Documents)` resolves
to `~/Documents` as hoped.
**Decision.** `paths.rs` honours `XDG_*` when set and otherwise builds the
macOS locations from the home directory itself: config, data and state under
`~/Library/Application Support/momr`, cache under `~/Library/Caches/momr`,
meetings from `user_special_dir(Documents)` with a home fallback. No
`cfg(target_os)`: the codebase is macOS-only (D02), so these are the paths.
**Rejected.** Trusting GLib and landing back in `~/.local`: wrong folders,
and the plan 06 goal names `~/Library` explicitly.
**Consequences.** Plan 06 steps 1 and 3 change shape (hardcoded defaults,
same `XDG_*` overrides, so its tests still apply); the formula (plan 08)
does not need to fix GLib.
**Status.** Accepted.

## D23 Transcript content stays English

**Context.** Plan 13 translated the interface into Indonesian, and the first pass sent the default speaker labels and the language line of `transcript.md` through the locales table too, so a meeting recorded with the Indonesian interface wrote "Kamu" and "Pembicara 1" where upstream writes "You" and "Speaker 1", and its language line in Indonesian too. `transcript.md` is an interface: users' scripts read it and upstream's app opens the same folders.
**Decision.** `transcript.md` speaker defaults ("You", "Remote", "Remote N", "Speaker N") and its language line are written in English whatever the interface language. Only the chrome translates; names a user types are kept as typed. On the command line the same line runs through stdout: `--help`, errors and progress on stderr translate, the transcript Markdown on stdout and the `watch` lines stay English. The interface language is resolved once per launch: `ui_language` in `settings.json`, else the first macOS preferred language (`AppleLanguages`), else `$LANG`. The app passes the result to `momr-menubar` as `MOMR_LANG`, so the menu bar item and the window always agree.
**Rejected.** Localised transcript labels: every script that looks for a `You:` line breaks on an Indonesian install, and a meeting moved to upstream shows mixed labels. Letting `momr-menubar` read `AppleLanguages` on its own: it would ignore the Preferences choice and disagree with the window.
**Consequences.** A meeting from either app, and from either interface language, carries the same default labels, so renaming and scripts behave the same way. A language change in Preferences takes effect on the next launch, for the window and the menu bar item together.
**Status.** Accepted.

## D24 Modular render engine; macOS shell is AppKit, GTK retires

**Context.** The GTK port proved the seams (capture, playback, providers,
timer, sources) but a display session confirmed plan 12's criterion 2: the
app reads as non-native unprompted (CSS-chrome look, client-drawn borders,
CPU-drawn UI). Patching GTK further cannot reach AppKit materials, the
compositor frame, or full VoiceOver.
**Decision.** The render engine is modular per build target: a UI-free Rust
core crate plus one shell per target, selected at build time. For macOS the
shell is AppKit (SwiftUI views embedded inside AppKit windows where that is
cheaper, not a second UI). The GTK app ships until the AppKit shell reaches
parity on the plan 10 smoke checklist, then GTK is deleted per D02's
same-change rule. Plans 12 (shell order) and 16 (workspace, seam trait)
are entered and normative for the shape; where they say SwiftUI-only, AppKit
with embedded SwiftUI governs.
**Rejected.** Keeping GTK as the macOS shell with more CSS: bounded,
documented shortfall, no path to the frame, materials or accessibility.
Rewriting the core in Swift: whisper.cpp, ONNX Runtime and the audio helpers
are the app, and they stay Rust.
**Consequences.** First step is a pure refactor (workspace split, GTK app
behaves identically, CI proves it). `momr-audio` and `momr-menubar` stay:
the tap helper and the status item are already native. The DMG then carries
only the AppKit app; the Homebrew formula keeps `momr-gtk` for a release or
two.
**Status.** Accepted; decided 2026-09-25 with the maintainer.

## D25 The core compiles for Windows 11+, macOS and Linux; seams live in one place

**Context.** D24 splits a UI-free core from per-target shells, with macOS
first. The same split is the Windows and Linux route (plan 16), but only if
the core never absorbs a platform API by accident.
**Decision.** `momr-core` builds on Windows 11+, macOS and Linux from day
one: std plus the transcription crates only, no GTK, no glib, no
`std::os::*` and no `cfg(target_os)` inside it — with one documented
exception, a leaf OS-API shim where std has no portable spelling (today:
the executable bit in `helper.rs`, which tries the spawn on Windows).
`momr-platform` is the bottom crate: std-only and portable, so the core
may use it for paths and, later, the other seams, and the shells use both.
`cfg(target_os)` lives in `momr-platform` and nowhere else. macOS is the
only tested target until a platform is scheduled; the others must at least
keep compiling the core.
**Rejected.** Gating platform code inside the core: D02 already showed where
that ends — two test surfaces and lingering per-OS paths.
**Consequences.** Slice 1 moves only modules that already satisfy the rule;
anything with a glib or Unix import stays in the GTK app until its seam is
cut. The `metal`/`vulkan`/`cuda` features stay whisper-rs features selected
per target, never core code.
**Status.** Accepted; decided 2026-09-25 with the maintainer.
