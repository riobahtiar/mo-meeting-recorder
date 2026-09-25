// Ending with the app. The app starts momr-menubar and stops it on a clean
// quit, but a crash or a signal (kill, a force quit) skips that, and the item
// would sit in the menu bar reading "off" forever. So the item watches the
// pid it is given in `MOMR_PARENT_PID` and quits when that process ends. An
// item started by hand gets no pid and stays until quit from its menu.

import Darwin
import Foundation

/// The pid to follow, from `MOMR_PARENT_PID`; nil when unset or not a pid.
public func parentPid(environment: [String: String]) -> pid_t? {
    guard let text = environment["MOMR_PARENT_PID"], let pid = pid_t(text), pid > 1
    else { return nil }
    return pid
}

/// Calls `onExit` once, on `queue`, when process `pid` ends, including when
/// it has already ended by the time the watch is set up (kqueue refuses to
/// watch a pid that is gone, so that case is checked by hand). Keep the
/// returned source alive for as long as the watch should last.
public func watchProcessExit(
    _ pid: pid_t, queue: DispatchQueue = .main, onExit: @escaping () -> Void
) -> DispatchSourceProcess {
    let source = DispatchSource.makeProcessSource(
        identifier: pid, eventMask: .exit, queue: queue)
    var fired = false
    let fire = {
        guard !fired else { return }
        fired = true
        source.cancel()
        onExit()
    }
    source.setEventHandler(handler: fire)
    source.resume()
    // After resume, so an exit between the check and the watch is not lost:
    // either the source sees it or this check does.
    queue.async {
        if kill(pid, 0) != 0 && errno == ESRCH {
            fire()
        }
    }
    return source
}
