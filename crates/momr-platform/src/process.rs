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
    // SAFETY: setsid always succeeds in a child that has forked.
    unsafe {
        sys::setsid();
    }
}

/// libc by hand: this crate takes no dependency for two syscalls. Called
/// directly rather than through `/bin/kill`, which would fork a process per
/// stop (every seek) and resolve `kill` through a PATH that puts user bins
/// first.
#[cfg(unix)]
mod sys {
    unsafe extern "C" {
        pub fn setsid() -> i32;
        pub fn kill(pid: i32, signal: i32) -> i32;
    }
    /// The same numbers on every Unix the app targets.
    pub const SIGTERM: i32 = 15;
    pub const SIGKILL: i32 = 9;
}

/// Sends `signal` to `pid` (a negative pid is a process group).
#[cfg(unix)]
fn send(pid: i64, signal: Signal) -> io::Result<()> {
    let pid = i32::try_from(pid).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let signal = match signal {
        Signal::Term => sys::SIGTERM,
        Signal::Kill => sys::SIGKILL,
    };
    // SAFETY: kill(2) takes plain integers and touches no memory of ours.
    if unsafe { sys::kill(pid, signal) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Whether `error` only says the process is gone already, which is what a
/// stop wanted anyway: callers ignore that one and report the rest.
pub fn already_gone(error: &io::Error) -> bool {
    // ESRCH on every Unix; Windows' taskkill reports no such code.
    error.raw_os_error() == Some(3)
}

/// Stops one process by pid, gracefully: SIGTERM on Unix. Unlike
/// `kill_group` this addresses the single pid, for children that share our
/// own process group (capture restarts); group-killing those would signal
/// the whole group, us included.
pub fn terminate(pid: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        send(i64::from(pid), Signal::Term)
    }
    #[cfg(not(unix))]
    {
        kill_group(pid, Signal::Term)
    }
}

/// Stops a process tree by pid: the whole group on Unix (the pid is a pgid
/// through `spawn_detached`), the one process on Windows until Job Objects
/// land with the Windows shell.
pub fn kill_group(pid: u32, signal: Signal) -> io::Result<()> {
    #[cfg(unix)]
    {
        send(-i64::from(pid), signal)
    }
    #[cfg(not(unix))]
    {
        let mut args = vec!["/PID".to_owned(), pid.to_string()];
        if signal == Signal::Kill {
            args.push("/F".to_owned());
        }
        let status = Command::new("taskkill").args(&args).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!("taskkill exited with {status}")))
        }
    }
}
