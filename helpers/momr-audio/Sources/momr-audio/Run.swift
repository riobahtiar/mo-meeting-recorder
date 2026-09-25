// `run`: the macOS answer to prctl's parent-death signal. Runs a program and
// kills it when the parent exits, so a meeting cannot keep playing after a
// crash. Watches the parent pid with kqueue EVFILT_PROC/NOTE_EXIT, and on
// the parent's exit sends the child SIGTERM, then SIGKILL half a second
// later. Exits with the child's status.
//
// On a clean stop the app's `Playback` Drop sends the wrapper SIGTERM, which
// is forwarded to the child, and only sends SIGKILL after a grace period.
// The order matters: SIGKILL on the wrapper cannot be caught, so it would
// leave the child running with nobody to stop it. The app must use SIGTERM.
//
// kqueue only reports an exit that happens after the registration, so a
// parent that dies between our start and `kevent(EV_ADD)` would go unseen.
// The parent pid is therefore read before anything else, and once the watch
// is in place (or has failed) a changed `getppid()` means we were already
// handed to launchd and the child is stopped at once.

import Darwin
import Foundation

private var runChild: pid_t = -1

private func runSigHandler(_ sig: Int32) {
    if runChild > 0 {
        kill(runChild, SIGTERM)
    }
    exit(0)
}

/// The parent is gone: ask the child to stop, give it half a second to
/// flush, then kill it and leave.
private func stopChildAndExit() -> Never {
    if runChild > 0 {
        kill(runChild, SIGTERM)
        usleep(500_000)
        if kill(runChild, 0) == 0 {
            kill(runChild, SIGKILL)
        }
    }
    exit(0)
}

private func resolveOnPath(_ program: String) -> String? {
    let paths = (ProcessInfo.processInfo.environment["PATH"] ?? "")
        .split(separator: ":").map(String.init)
    for dir in paths {
        let candidate = (dir as NSString).appendingPathComponent(program)
        if access(candidate, X_OK) == 0 {
            var isDir: ObjCBool = false
            if FileManager.default.fileExists(
                atPath: candidate, isDirectory: &isDir), !isDir.boolValue
            {
                return candidate
            }
        }
    }
    return nil
}

func runRun(program: String, args: [String]) -> Int32 {
    // Read first, so a parent that dies while the child starts still shows
    // up as a changed getppid() below.
    let parent = getppid()
    let resolved: String
    if program.contains("/") {
        resolved = program
    } else if let found = resolveOnPath(program) {
        resolved = found
    } else {
        fputs("momr-audio: \(program): not found on PATH\n", stderr)
        return 2
    }
    let child = Process()
    child.executableURL = URL(fileURLWithPath: resolved)
    child.arguments = args
    do {
        try child.run()
    } catch {
        fputs("momr-audio: could not run \(program): \(error)\n", stderr)
        return 2
    }
    runChild = child.processIdentifier

    // The signal dispositions are installed after run(): Foundation resets
    // them when the child starts.
    signal(SIGTERM, runSigHandler)
    signal(SIGINT, runSigHandler)

    let kq = kqueue()
    var watching = false
    var watchErrno = errno
    if kq >= 0 {
        var change = kevent()
        change.ident = UInt(parent)
        change.filter = Int16(EVFILT_PROC)
        change.flags = UInt16(EV_ADD | EV_ONESHOT)
        change.fflags = UInt32(NOTE_EXIT)
        if kevent(kq, &change, 1, nil, 0, nil) == 0 {
            watching = true
            DispatchQueue.global().async {
                var out = kevent()
                _ = kevent(kq, nil, 0, &out, 1, nil)
                stopChildAndExit()
            }
        } else {
            watchErrno = errno
        }
    }
    // Registering on a parent that exited a moment earlier either fails with
    // ESRCH or is never reported, depending on how far its exit got. Either
    // way we have already been reparented, which this check catches.
    if getppid() != parent {
        stopChildAndExit()
    }
    if !watching {
        fputs(
            "momr-audio: could not watch the parent process (errno \(watchErrno)); the child will outlive a crash\n",
            stderr)
    }
    child.waitUntilExit()
    return child.terminationStatus
}
