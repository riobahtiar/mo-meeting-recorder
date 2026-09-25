// `run`: the macOS answer to prctl's parent-death signal. Runs a program and
// kills it when the parent exits, so a meeting cannot keep playing after a
// crash. Watches the parent pid with kqueue EVFILT_PROC/NOTE_EXIT; forwards
// our own SIGTERM to the child (the app's Drop kills the wrapper on a clean
// exit). Exits with the child's status.

import Darwin
import Foundation

private var runChild: pid_t = -1

private func runSigHandler(_ sig: Int32) {
    if runChild > 0 {
        kill(runChild, SIGTERM)
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

    let parent = getppid()
    let kq = kqueue()
    if kq >= 0 {
        var change = kevent()
        change.ident = UInt(parent)
        change.filter = Int16(EVFILT_PROC)
        change.flags = UInt16(EV_ADD | EV_ONESHOT)
        change.fflags = UInt32(NOTE_EXIT)
        if kevent(kq, &change, 1, nil, 0, nil) == 0 {
            DispatchQueue.global().async {
                var out = kevent()
                _ = kevent(kq, nil, 0, &out, 1, nil)
                kill(runChild, SIGTERM)
                usleep(500_000)
                if runChild > 0, kill(runChild, 0) == 0 {
                    kill(runChild, SIGKILL)
                }
                exit(0)
            }
        }
    }
    child.waitUntilExit()
    return child.terminationStatus
}
