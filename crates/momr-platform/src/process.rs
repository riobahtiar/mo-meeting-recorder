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

/// A process group this app started: only `spawn_detached` makes one, so a
/// group kill can never be aimed at a pid that shares our own group (a
/// capture child), which would signal the app itself. The mix-up happened
/// once (plan 12 slice 5); the type keeps it from happening again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Group(u32);

impl Group {
    /// Stops the whole tree: the group on Unix (pid == pgid after
    /// `setsid`), the one process on Windows until Job Objects land with
    /// the Windows shell.
    pub fn kill(self, signal: Signal) -> io::Result<()> {
        #[cfg(unix)]
        {
            send(-i64::from(self.0), signal)
        }
        #[cfg(not(unix))]
        {
            taskkill(self.0, signal)
        }
    }
}

/// Spawns `command` detached from this process's terminal and group: its own
/// session on Unix (so `Group::kill` reaches the whole tree and no terminal
/// stops it), a plain spawn where the OS has no sessions.
pub fn spawn_detached(command: &mut Command) -> io::Result<(Child, Group)> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe and touches only the child's
        // own state: a new session, so its own process group (pid == pgid,
        // which `Group` relies on) and no controlling terminal.
        unsafe {
            command.pre_exec(|| {
                libc_sets_id();
                Ok(())
            });
        }
    }
    let child = command.spawn()?;
    let group = Group(child.id());
    Ok((child, group))
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
/// `Group::kill` this addresses the single pid, for children that share our
/// own process group (capture restarts, playback).
pub fn terminate(pid: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        send(i64::from(pid), Signal::Term)
    }
    #[cfg(not(unix))]
    {
        taskkill(pid, Signal::Term)
    }
}

#[cfg(not(unix))]
fn taskkill(pid: u32, signal: Signal) -> io::Result<()> {
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn gone_within(child: &mut Child, limit: Duration) -> Option<std::process::ExitStatus> {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if let Ok(Some(status)) = child.try_wait() {
                return Some(status);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }

    fn pgid(pid: u32) -> Option<u32> {
        let out = Command::new("ps")
            .args(["-o", "pgid=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }

    #[test]
    fn a_detached_child_leads_its_own_group() {
        let (mut child, group) = spawn_detached(Command::new("sleep").arg("30")).unwrap();
        assert_eq!(pgid(child.id()), Some(child.id()));
        assert_eq!(group, Group(child.id()));
        group.kill(Signal::Kill).unwrap();
        assert!(gone_within(&mut child, Duration::from_secs(2)).is_some());
    }

    /// The point of a group: the grandchild an agent's shell started dies
    /// with it, instead of outliving the app.
    #[test]
    fn a_group_kill_reaches_the_grandchild() {
        let marker = format!("{}", 900_000 + std::process::id() % 1000);
        let script = format!("sleep {marker} & wait");
        let (mut child, group) = spawn_detached(Command::new("sh").args(["-c", &script])).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        group.kill(Signal::Term).unwrap();
        assert!(gone_within(&mut child, Duration::from_secs(2)).is_some());
        std::thread::sleep(Duration::from_millis(200));
        let survivors = Command::new("pgrep")
            .args(["-f", &format!("^sleep {marker}$")])
            .output()
            .unwrap();
        assert!(
            survivors.stdout.is_empty(),
            "the grandchild outlived its group"
        );
    }

    #[test]
    fn terminate_sends_sigterm_and_a_dead_pid_is_already_gone() {
        use std::os::unix::process::ExitStatusExt;
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        terminate(child.id()).unwrap();
        let status = gone_within(&mut child, Duration::from_secs(2)).unwrap();
        assert_eq!(status.signal(), Some(sys::SIGTERM));
        let error = terminate(child.id()).unwrap_err();
        assert!(already_gone(&error), "{error}");
    }
}
