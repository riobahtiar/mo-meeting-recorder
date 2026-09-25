# Archived plans

Plans whose **Status** list is fully ticked and whose **Done when** lines were all observed move here, in the same commit that ticks the last box. They stay readable for their decisions and their observations; nothing in them is worked on any more. Links from `00-overview.md` and `01-decisions.md` point here.

| Plan | Description | Archived | Last observed |
|---|---|---|---|
| [02 Compile on macOS](02-compile-on-macos.md) | `cargo build`, `cargo test`, clippy and fmt pass on macOS; the Omarchy leftovers that needed a compiler to remove (bar widget, `prctl`, Linux `O_NOFOLLOW`, `vulkan`) are gone; `transcribe-file` works on CPU and with `--features metal`. | 2026-09-25 | Build, tests, clippy, fmt and `transcribe-file --model tiny` verified on Apple silicon |
| [06 Paths and environment](06-paths-and-environment.md) | One `paths.rs` for `~/Library/Application Support/momr`, `~/Library/Caches/momr` and `~/Documents/Meetings`, honouring absolute `XDG_*`; a Finder launch finds ffmpeg, the helper and the agents through `extend_path`. | 2026-09-25 | Fresh `~/Library` homes created on first run; socket, staging and `watch` verified; `open` launch spawns both helpers (D22 records the GLib finding) |

## How to archive a plan

1. Every box in its Status list and its Done when list is `[x]`, and the observation paragraph says what was verified and where.
2. `git mv plans/NN-name.md plans/archives/NN-name.md`, add a row above, and change the plan's row in `plans/README.md` to point here with status `archived`.
3. Fix any link to it in `00-overview.md`, `01-decisions.md` and `README.md`.
