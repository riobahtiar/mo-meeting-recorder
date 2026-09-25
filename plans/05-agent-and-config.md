# 05 Agent and config

## Goal

Chapters work with an agent the user names in `config.toml`, run under the same no-tools boundary, process group, size limits and timeout as before. The `ask` command works from the terminal. Nothing asks for `omarchy-default-agent` any more.

## Done when

- [x] `momr ask --agent` prints the configured agent.
- [x] `echo "Say hi" | momr ask "Answer in one word"` returns text within the timeout.
- [ ] A three-minute-plus meeting gets chapters; the Chapters header regenerates them.
- [x] A hung agent is killed with its whole process group at the timeout (fake agent: `sh -c 'sleep 1000'`).
- [x] `grep -n omarchy src/agent.rs` finds nothing.

Observed 2026-09-25: with `agent = "pi"` in config.toml, `ask --agent`
prints `Pi (pi)` and a live run repeats its stdin through the new
`pre_exec(setsid)` wrapper in 2.5 s; `agent = "crush"` refuses with the
reason; a missing key names the config file path. `ulimit -f` caps writes on
macOS `sh` as the wrapper assumes. No `gtimeout` is installed here, so the
inner bound is covered by the argv-shape test only. Chapters on a long
meeting still need a display session (record three minutes, check chapters).

## Prerequisites

Plan 02. Plan 06 makes the config file live under `~/Library`; until then it is `~/.config/momr/config.toml`.

## Background

`agent.rs` is a careful piece of work: the module doc explains that the transcript is untrusted input and the only boundary is running the agent with **no tools**, through each agent's own single switch. Read it in full before touching anything.

What has to change:

- `status()` runs `omarchy-default-agent` to learn the agent id, and `Unavailable::Unset` tells the user to run `omarchy default agent`.
- `run()` wraps the agent in `setsid sh -c 'ulimit -f "$1" && … exec timeout -k 5 "$secs" "$@"'`: a new session (own process group, no controlling terminal), a file-size limit, and GNU `timeout` as an inner bound. macOS has neither `setsid` nor GNU `timeout` (Homebrew `coreutils` installs them as `gsetsid`, `gtimeout`).
- The module doc's first lines credit a port from an Omarchy text-transform plugin. Keep the credit to the original author's runner, drop the Omarchy framing.

What stays: `workdir()` prefers `$XDG_RUNTIME_DIR`, else `temp_dir()`; on macOS `$TMPDIR` is a per-user, mode 700 directory, which is the property the code wants. `kill_group()` uses `kill -- -<pid>`, which works. `read_bounded()`'s constants were fixed in plan 02.

`models.rs` `configured()` parses `model = "…"` from `config.toml` by hand (no TOML crate). The agent key follows the same one-line style.

## Steps

### 1. A small config reader

In `models.rs`, generalise the parser:

```rust
// sketch
/// The value of `key = "…"` in config.toml, comments stripped; None when absent.
pub fn config_value(key: &str) -> Option<String> { /* the body of configured(), parameterised */ }

pub fn configured() -> String {
    OVERRIDE… .or_else(|| config_value("model")).unwrap_or_else(|| DEFAULT.to_owned())
}
```

Add a test with a two-key file (`model`, `agent`) and a commented line.

### 2. Agent selection

In `agent.rs` `status()`, replace the `omarchy-default-agent` call with `crate::models::config_value("agent")`. `Unavailable::Unset` becomes: `No agent set. Add agent = "claude" to <config path>` using the path function so it stays right after plan 06. Update the doc comments on `Agent::id` and `Unavailable::Unset`, and the module doc's opening lines.

### 3. Process group, file limit, timeout without `setsid`

Replace the wrapper in `run()`:

```rust
// sketch
use std::os::unix::process::CommandExt;
let mut command = Command::new("sh");
command
    .arg("-c")
    .arg(r#"ulimit -f "$1" && shift && exec "$@""#)
    .arg("sh")
    .arg(built.file_limit_kb.to_string())
    .arg(&built.program)
    .args(&built.args)
    .envs(…).current_dir(dir).stdin(…).stdout(stdout).stderr(stderr);
// SAFETY: setsid is async-signal-safe and touches only the child's own state.
unsafe { command.pre_exec(|| { libc::setsid(); Ok(()) }); }
```

`setsid()` in `pre_exec` is the direct equivalent of the `setsid` binary: a new session, own process group (pid == pgid, so `kill_group()` works as before), no controlling terminal, so an agent that touches the inherited terminal is not stopped by SIGTTIN.

GNU `timeout` was defence in depth for the case where this process dies before the loop can kill the agent. Keep it when available: look for `gtimeout` on `PATH` at run time and insert `exec gtimeout -k 5 "$secs"` when found; otherwise rely on the loop. Say so in the module doc.

### 4. Agents on macOS

The flag table needs no change: `claude`, `codex`, `opencode`, `pi`, `copilot`, `goose`, `grok` all exist on macOS as npm or Homebrew installs. What differs is **where they are**: `~/.npm-global/bin`, `/opt/homebrew/bin`, `~/.bun/bin`, `~/.volta/bin`, `~/.local/bin`. `which()` in `agent.rs` walks `PATH`, so plan 06 step 4 (PATH for GUI launches) is what makes chapters work from a Finder-launched app. From a terminal it works as soon as the shell can find the agent.

### 5. Tests

`agent.rs` already tests `build_command` per agent. Add:

- `configured_id()` from the config reader passed as a closure; empty and missing cases give `Unset`.
- The wrapper's argv shape: `sh -c <ulimit script> sh <kb> <program> <args…>` when no `gtimeout` is found; with `gtimeout -k 5 <secs>` inserted when it is.
- Timeout path with a fake agent `sh -c 'sleep 1000'`: the group is gone after the timeout (a test-only shorter timeout via a parameter, not a global).

## Verify

1. `echo 'agent = "claude"' >> <config.toml>`; `momr ask --agent` prints `claude`.
2. `echo "MOM Recorder" | momr ask "Repeat the input"` prints the text.
3. Record or import a meeting over three minutes (the `say` fixtures from plan 02, concatenated): chapters appear.
4. Set `agent = "crush"` (refused by design): the error says why.
5. Remove the key: the message names the config file path.

## Risks and notes

- A Finder-launched app has no agent on `PATH` until plan 06 step 4. Until then, test from a terminal.
- `ulimit -f` in macOS `sh` takes 512-byte blocks like Linux's; the existing KiB conversion holds. Verify: `sh -c 'ulimit -f 10 && dd if=/dev/zero of=/tmp/x bs=1k count=20'` fails at the limit.

## Status

- [x] Step 1 `config_value`
- [x] Step 2 selection from config, Omarchy lookup gone
- [x] Step 3 `pre_exec(setsid)`, optional `gtimeout`
- [x] Step 4 agents found (depends on plan 06 for GUI)
- [x] Step 5 tests
