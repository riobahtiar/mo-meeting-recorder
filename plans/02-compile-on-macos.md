# 02 Compile on macOS

## Goal

`cargo build --release` and `cargo test` pass on macOS, `cargo clippy --all-targets -- -D warnings` is clean, and the Omarchy leftovers that only a compiler can safely remove are gone: the bar widget module and its offer dialog, the Linux-only `prctl` call, the Linux `O_NOFOLLOW` literal, the `vulkan` feature. Nothing has to *work* yet beyond command-line transcription, which needs no audio device.

## Done when

- [ ] `cargo build --release` succeeds on an Apple silicon Mac with the Homebrew libraries.
- [ ] `cargo test` is green.
- [ ] `cargo clippy --all-targets -- -D warnings` is clean.
- [ ] `cargo build --release --features metal` succeeds.
- [ ] `target/release/momr transcribe-file <file> --model tiny` prints a transcript.
- [ ] `src/bar_widget.rs` is gone, `ui.rs` no longer offers a bar widget, `settings.rs` has no `bar_widget_offered`.
- [ ] `grep -rn -i omarchy src/` lists only `theme.rs` (plan 07), `agent.rs` `status()` (plan 05), `player.rs` (plan 04) and `animation.rs`'s palette comment (plan 07).
- [ ] The root `README.md` status table row for this plan says what was observed.

## Prerequisites

None. This is the first plan.

## Background

The identity rename is done: the crate is `momr`, `APP_NAME` is `momr`, `APP_ID` is `io.github.riobahtiar.MOMRecorder`. The pacman packaging, the installer script, the bar plugin and the freedesktop files were deleted. What remains of Omarchy is inside Rust modules, where a change needs a compiler to be safe.

Two things stop the build today:

- `player.rs` `die_with_parent()` calls `libc::prctl(libc::PR_SET_PDEATHSIG, …)`. Neither symbol exists in the `libc` crate for macOS. Its `use std::os::unix::process::CommandExt` import is only for `pre_exec`.
- `agent.rs` `read_bounded()` defines `O_NOFOLLOW` and `O_NONBLOCK` as Linux literals (`0o400000`, `0o4000`). They compile anywhere but are the wrong values on macOS (`0x0100`, `0x0004`), so `open()` would silently follow symlinks.

`bar_widget.rs` links an Omarchy bar plugin into `~/.config/omarchy/plugins` and enables it through `omarchy-shell`. `ui.rs` `offer_bar_widget()` (around line 1416) shows the dialog once, guarded by `settings::bar_widget_offered()`. None of it has a macOS counterpart; plan 09 builds a menu bar item on the `watch` protocol instead.

Everything else Linux-only (`parec`, `pacat`, `hyprctl`, `setsid`, `timeout`, `omarchy-default-agent`, the theme reader) is a child process or a file read at run time, so it compiles and simply fails. Later plans replace each.

`whisper-rs-sys` compiles whisper.cpp with CMake during the build, and `ort` downloads a prebuilt ONNX Runtime for the target during the build, so the first build needs CMake, the Xcode command line tools and the network.

## Steps

### 1. Toolchain

```bash
xcode-select --install
brew install gtk4 libadwaita adwaita-icon-theme cmake pkgconf ffmpeg
rustup update stable          # or mise use rust@stable; edition 2024 needs a current stable
pkg-config --modversion gtk4 libadwaita-1
```

libadwaita must report 1.6 or newer (the crate is built with `v1_6`). If `pkg-config` cannot find them, Homebrew's `PKG_CONFIG_PATH` is not exported; `brew shellenv` fixes that.

### 2. Stub `die_with_parent`

In `player.rs`, replace the body and the `CommandExt` import with a stub that plan 04 fills in:

```rust
// sketch
/// macOS has no parent-death signal. Plan 04 wraps every child in
/// `momr-audio run`, which kills it when this process goes away.
fn die_with_parent(command: &mut Command) -> &mut Command {
    command
}
```

Rewrite the module doc's rationale sentence: playback is ffmpeg into an audio sink because the app already depends on ffmpeg; the GStreamer comparison and the Omarchy mention go.

### 3. Use `libc` constants in `read_bounded`

In `agent.rs` `read_bounded()`, replace the two literals with `libc::O_NOFOLLOW` and `libc::O_NONBLOCK`. `libc` is already a dependency. One line of comment: the values differ per platform, which is why they come from `libc`.

### 4. Remove the bar widget

- Delete `src/bar_widget.rs` and its `mod bar_widget;` line in `main.rs`.
- In `ui.rs`, delete `offer_bar_widget()` and its call (around lines 1381 and 1416 to 1450). Read the surrounding state machine first: the call sits in the first-start path after the model banner; nothing else depends on it.
- In `settings.rs`, delete `bar_widget_offered()` and `set_bar_widget_offered()`, and the mention in the module doc.
- `ipc.rs` and `main.rs` keep `watch`: the protocol is the seam plan 09 builds on. Reword their doc comments from "the bar widget" to "a menu bar item or any other client".

### 5. Features

In `Cargo.toml`, replace the `vulkan` feature:

```toml
[features]
default = []
# GPU transcription through whisper.cpp's Metal backend, for Apple silicon and
# recent Intel Macs. Needs nothing beyond the Xcode command line tools.
metal = ["whisper-rs/metal"]
```

whisper-rs turns Metal off unless the feature is on, so the default build stays on the CPU.

### 6. Build and fix warnings

```bash
cargo build --release 2>&1 | tee build.log
```

Expect several minutes the first time (whisper.cpp, then the ONNX Runtime download). Fix every warning at its cause, not with `#[allow]`. The `hyprctl_*` helpers stay for now (plan 04 removes them with the compact-strip rewrite).

### 7. Tests and lints

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

The tests are pure logic with fakes for external commands (`agent.rs` tests build commands without running them), so they should pass. The five `bar_widget.rs` tests go with the module. If a test shells out on macOS, make it take the command as a closure the way `bar_widget.rs` `enable()` did rather than skipping it.

### 8. Smoke test the pipeline without audio devices

```bash
say -o /tmp/hello.aiff "Hello from MOM Recorder. This is a test of the transcription pipeline."
target/release/momr transcribe-file /tmp/hello.aiff --model tiny
```

`say` is macOS's speech synthesiser, so this is invented content and allowed by the fixture rule. The first run downloads the tiny model (75 MB). Expect one or two lines of transcript with `Speaker 1`. Then try the default model once to see the 1.6 GB download and the CPU speed:

```bash
target/release/momr transcribe-file /tmp/hello.aiff
```

### 9. Metal build

```bash
cargo build --release --features metal
time target/release/momr transcribe-file /tmp/hello.aiff
```

Record the CPU and Metal timings in the pull request so the README can say what to expect.

### 10. Open the window once

```bash
target/release/momr
```

It should open on the ready page with flat meters (no capture yet). If the window is blank or flickers, try `GSK_RENDERER=cairo` and note it; plan 07 decides whether to set it programmatically. Missing icons mean `adwaita-icon-theme` is not installed or `XDG_DATA_DIRS` does not include `/opt/homebrew/share`.

## Verify

- `cargo build --release`, `cargo test`, `cargo clippy --all-targets -- -D warnings`: all green.
- `transcribe-file` on the `say` fixture prints a transcript.
- `grep -rn -i omarchy src/` matches the Done-when list and nothing more.

## Risks and notes

- **CMake against a new macOS SDK.** whisper.cpp tracks Apple SDKs closely; if the pinned `whisper-rs-sys` fails to compile against macOS 27's SDK, bump `whisper-rs` in `Cargo.toml` rather than patching the build.
- **`ort` download.** `ort-sys` fetches a tarball from pyke's CDN during the build. Behind a proxy or offline this fails with a clear error; there is no offline path without vendoring the library.
- **Renderer.** GTK's default `ngl` renderer had macOS bugs in 4.14 and 4.16. `GSK_RENDERER=cairo` is the fallback.

## Status

- [ ] Step 1 toolchain
- [ ] Step 2 `die_with_parent` stub
- [ ] Step 3 `libc` constants
- [ ] Step 4 bar widget removed
- [ ] Step 5 features
- [ ] Step 6 build clean
- [ ] Step 7 tests and lints
- [ ] Step 8 transcription smoke test
- [ ] Step 9 Metal timing recorded
- [ ] Step 10 window opens
