//! Child processes across targets: spawning detached and stopping by pid.
//!
//! The app isolates crashes by running audio and agents in child processes,
//! then has to stop whole process trees (an agent that ignores TERM, a tap
//! that outlives a crash). Unix does that with sessions and process groups;
//! Windows needs Job Objects, which arrive with the Windows shell (plan 16)
//! — until then the Windows branches below do the portable subset.

use std::io;
use std::process::{Child, Command};

/// How hard to stop a process tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    /// Graceful: SIGTERM on Unix, `taskkill` on Windows.
    Term,
    /// Uncatchable: SIGKILL on Unix, `taskkill /F` on Windows.
    Kill,
}

/// Spawns `command` detached from this process's terminal and group: its own
/// session on Unix (so `kill_group` reaches the whole tree and no terminal
/// stops it), a plain spawn where the OS has no sessions.
pub fn spawn_detached(command: &mut Command) -> io::Result<Child> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe and touches only the child's
        // own state: a new session, so its own process group (pid == pgid,
        // which `kill_group` relies on) and no controlling terminal.
        unsafe {
            command.pre_exec(|| {
                libc_sets_id();
                Ok(())
            });
        }
    }
    command.spawn()
}

#[cfg(unix)]
fn libc_sets_id() {
    // libc by hand: this crate takes no dependency for one syscall, and the
    // call cannot fail once the process exists.
    unsafe extern "C" {
        fn setsid() -> i32;
    }
    // SAFETY: setsid always succeeds in a child that has forked.
    unsafe {
        setsid();
    }
}

/// Stops one process by pid, gracefully: SIGTERM on Unix. Unlike
/// `kill_group` this addresses the single pid, for children that share our
/// own process group (capture restarts); group-killing those would signal
/// the whole group, us included.
pub fn terminate(pid: u32) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(unix))]
    {
        kill_group(pid, Signal::Term);
    }
}

/// Stops a process tree by pid: the whole group on Unix (the pid is a pgid
/// through `spawn_detached`), the one process on Windows until Job Objects
/// land with the Windows shell.
pub fn kill_group(pid: u32, signal: Signal) {
    #[cfg(unix)]
    {
        let flag = match signal {
            Signal::Term => "-TERM",
            Signal::Kill => "-KILL",
        };
        let _ = Command::new("kill")
            .args([flag, "--", &format!("-{pid}")])
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(unix))]
    {
        let mut args = vec!["/PID".to_owned(), pid.to_string()];
        if signal == Signal::Kill {
            args.push("/F".to_owned());
        }
        let _ = Command::new("taskkill").args(&args).status();
    }
}
