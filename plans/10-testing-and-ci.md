# 10 Testing and CI

## Goal

Every platform seam has a unit test that runs without audio hardware, the transcription pipeline has an end-to-end check on an invented fixture, a manual smoke checklist exists for what only a human can verify, and CI runs the whole thing on macOS for every push.

## Done when

- [x] `cargo test` covers the command builders, the paths module, config parsing, agent selection, the menu model's action names and the socket-path fallback.
- [x] An end-to-end test transcribes a `say`-generated fixture and finds the expected words.
- [x] `.github/workflows/ci.yml` runs fmt, clippy with `-D warnings`, tests, a release build and the Swift package's tests on `macos-14`, green.
- [ ] The smoke checklist below has been walked once per milestone and the results noted in the pull request.

Observed 2026-09-25: 55 unit tests pass; `tests/transcribe.rs` (`#[ignore]`)
passes with `-- --ignored` in ~3 s; 6 Swift tests pass; the meeting fixture
(`tests/fixtures/meeting`, invented Maya/Tom lines) opens in both manifest
shapes. CI is written but has never run — the first push turns it green or
red. The smoke list is walked in headless slices only (socket-driven record,
tap loopback, kill -9 safety); the on-screen lines need a display session.

## Prerequisites

Plan 02. Extend as each later plan lands.

## Background

Upstream has 29 unit tests in `#[cfg(test)] mod tests` blocks: agent flag tables, chapter parsing, diarization maths, export format keys, manifest round-trips, model name resolution, theme CSS, and the bar widget's `enable()` with faked commands (removed in plan 02). They run without GTK windows but the crate links GTK, so a test machine needs the libraries.

The pattern to copy for anything that touches the operating system: take the effect as a closure or a value, test the decision logic, and keep the real spawn in a thin untested wrapper.

## Steps

### 1. Unit tests per seam (added by the plan that introduces the seam)

| Test | Plan | Shape |
|---|---|---|
| `capture_args(Device, helper)` program and arguments | 03 | pure function |
| Helper exit code → fallback and banner text | 03 | closure for spawn |
| `guarded(program, helper)` wraps with `momr-audio run` when a helper path is given | 04 | parameter |
| Accel for `win.compact` | 04 | read back from the app |
| `config_value(key)` with comments and two keys | 05 | string input |
| `configured_id()` from config; empty gives `Unset` | 05 | closure |
| Agent wrapper argv with and without `gtimeout` | 05 | pure function |
| `paths::*` suffixes, `XDG_*` override (mutex-guarded) | 06 | env var |
| Socket path falls back when over 100 bytes | 06 | base dir as parameter |
| `extend_path` order and dedup | 06 | PATH string in and out |
| Every action name in the menu model exists on app or window | 07 | build the menu, walk it, compare to a registered set |
| `Theme` has the seven colours and follows `dark` | 07 | pure |
| `config.toml` writer keeps unknown lines and comments | 07 | string round-trip |

### 2. End-to-end transcription test

`tests/transcribe.rs` (integration test, `#[ignore]` by default so `cargo test` stays fast; CI runs `cargo test -- --ignored` on a job that has the tiny model cached):

1. Generate a fixture at test time: `say -o fixture.aiff "The quick brown fox jumps over the lazy dog"`.
2. Run the binary: `transcribe-file fixture.aiff --model tiny --language en`.
3. Assert the output contains `fox` and `dog` (case-insensitive) and has the Markdown shape (`**Speaker 1**` or the timestamp column).

Cache `~/Library/Application Support/momr/models` in CI so the 75 MB download happens once.

### 3. Meeting folder round trip

A checked-in fixture folder in `tests/fixtures/meeting/` (short Opus tracks generated with `say` and ffmpeg, a manifest, a transcript, all invented). Tests: `meeting.rs` loads it; `export::tracks()` finds `.tracks/`; the loader `ui::run` calls yields the same manifest fields. Include one manifest written by upstream's Linux build (from the demo scripts) to guard the "opens upstream meetings" promise.

### 4. Swift helper tests

`helpers/momr-audio/Tests/`: Float32 → Int16 clamping and interleaving; argument parsing; `run` kills the child when the parent exits (spawn a `sleep`, kill the test's own child wrapper, assert the grandchild is gone within a second).

### 5. Smoke checklist (manual, per milestone)

Walk it after plans 03 to 07 and before every release; record date, macOS version, hardware and outcome in the pull request.

- [ ] Fresh launch from a terminal: window appears in under three seconds, meters flat, model banner visible when no model.
- [ ] Speak: mic meter moves. Play a video: computer meter moves.
- [ ] Start, pause, resume, stop a 60-second recording with both sides active.
- [ ] Transcribing animation runs; the done page has You and Remote lines; play from a clicked line; seek in the waveform.
- [ ] Rename a speaker; rename the meeting; the folder and transcript follow.
- [ ] Edit, swap and delete a line; Undo works.
- [ ] Copy transcript with Enter and with ⇧⌘C; paste elsewhere.
- [ ] Import an mp3 with two voices: two speakers in two colours.
- [ ] ⇧⌘M to the strip and back; drag the strip.
- [ ] ⌘W while recording asks; cancel; ⌘Q while transcribing offers to finish in the background.
- [ ] Kill the app with `kill -9` while recording; relaunch: recovery dialog; Save produces a meeting.
- [ ] Switch system appearance and accent: the app follows.
- [ ] Menu bar items enable and disable with the page (plan 07).
- [ ] From the `.app` (plan 08): double-click a `.meeting-recorder` file; mic prompt shows the app's name.
- [ ] A meeting folder recorded with upstream on Linux opens on the done page.

### 6. CI workflow

`.github/workflows/ci.yml`:

```yaml
# sketch
name: ci
on: [push, pull_request]
jobs:
  macos:
    runs-on: macos-14
    steps:
      - uses: actions/checkout@v4
      - run: brew install gtk4 libadwaita adwaita-icon-theme cmake pkgconf ffmpeg
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test
      - run: cargo build --release --features metal
      - run: swift build -c release --package-path helpers/momr-audio
      - run: swift test --package-path helpers/momr-audio
```

Add a nightly job that runs `cargo test -- --ignored` with the model cache for the end-to-end test. Upload the binary and helper as workflow artifacts so a reviewer can try a branch without building.

## Verify

- Push a branch: the job is green. Break a test on purpose in a scratch branch: it goes red.
- The end-to-end test passes locally with `cargo test -- --ignored` on this Mac.

## Risks and notes

- `macos-14` runners are Apple silicon; Intel coverage comes only from the release job on `macos-13` (plan 08) until GitHub retires it. Keep an Intel Mac or VM in the loop for releases.
- Homebrew on CI installs the newest GTK; a breaking change shows up in CI before it shows up for users, which is the point.

## Status

- [x] Step 1 unit tests (grows with each plan)
- [x] Step 2 end-to-end transcription
- [x] Step 3 meeting folder fixture
- [x] Step 4 Swift tests
- [ ] Step 5 smoke checklist walked
- [x] Step 6 CI workflow
