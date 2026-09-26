import Foundation

/// Finding the command-line tools the shell runs (`momr-audio`, `momr`).
///
/// A Finder or Spotlight launch brings only `/usr/bin:/bin:/usr/sbin:/sbin`,
/// without Homebrew or the user's tool bins, so a plain PATH lookup finds
/// nothing a terminal launch would. The search order is the Rust shell's
/// (`src/main.rs` `extend_path`): next to this executable, the bundle's
/// MacOS dir, the usual tool bins, then the login shell's PATH. Each tool is
/// looked up once and kept: the login shell can take seconds.
enum Tools {
    private static var found: [String: URL] = [:]
    private static let lock = NSLock()

    /// The tool called `name`, or nil when no search place has it.
    static func url(_ name: String) -> URL? {
        lock.lock()
        defer { lock.unlock() }
        if let url = found[name] {
            return url
        }
        guard let url = search(name) else { return nil }
        found[name] = url
        return url
    }

    private static func search(_ name: String) -> URL? {
        let exe = (Bundle.main.executableURL ?? URL(fileURLWithPath: CommandLine.arguments[0]))
            .resolvingSymlinksInPath()
            .deletingLastPathComponent()
        var dirs = [exe.path, exe.deletingLastPathComponent().appendingPathComponent("MacOS").path]
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        dirs += ["/opt/homebrew/bin", "/usr/local/bin", "\(home)/.local/bin", "\(home)/.cargo/bin"]
        dirs += (ProcessInfo.processInfo.environment["PATH"] ?? "").split(separator: ":").map(String.init)
        for dir in dirs {
            let candidate = URL(fileURLWithPath: dir).appendingPathComponent(name)
            if FileManager.default.isExecutableFile(atPath: candidate.path) {
                return candidate
            }
        }
        for dir in shellPath {
            let candidate = URL(fileURLWithPath: dir).appendingPathComponent(name)
            if FileManager.default.isExecutableFile(atPath: candidate.path) {
                return candidate
            }
        }
        return nil
    }

    /// The login shell's PATH, asked once with a three-second limit so a
    /// slow rc file cannot hang the app, as `login_shell_path` does in Rust.
    private static let shellPath: [String] = {
        guard let shell = ProcessInfo.processInfo.environment["SHELL"] else { return [] }
        let task = Process()
        task.executableURL = URL(fileURLWithPath: shell)
        task.arguments = ["-lc", "printf %s \"$PATH\""]
        let pipe = Pipe()
        task.standardOutput = pipe
        task.standardError = FileHandle.nullDevice
        task.standardInput = FileHandle.nullDevice
        do {
            try task.run()
        } catch {
            return []
        }
        let done = DispatchSemaphore(value: 0)
        var data = Data()
        DispatchQueue.global().async {
            data = pipe.fileHandleForReading.readDataToEndOfFile()
            done.signal()
        }
        if done.wait(timeout: .now() + 3) == .timedOut {
            task.terminate()
            return []
        }
        task.waitUntilExit()
        return String(decoding: data, as: UTF8.self).split(separator: ":").map(String.init)
    }()

    /// Runs `url` with `arguments` to its end on the calling thread. Stdout
    /// and stderr are read while it runs, so neither pipe can fill and stall
    /// the child; the result carries stdout and the last stderr line.
    static func run(_ url: URL, _ arguments: [String]) throws -> (status: Int32, output: Data, reason: String) {
        let process = Process()
        process.executableURL = url
        process.arguments = arguments
        let out = Pipe()
        let err = Pipe()
        process.standardOutput = out
        process.standardError = err
        process.standardInput = FileHandle.nullDevice
        try process.run()
        var errData = Data()
        let errDone = DispatchSemaphore(value: 0)
        DispatchQueue.global().async {
            errData = err.fileHandleForReading.readDataToEndOfFile()
            errDone.signal()
        }
        let output = out.fileHandleForReading.readDataToEndOfFile()
        errDone.wait()
        process.waitUntilExit()
        let reason = String(decoding: errData, as: UTF8.self)
            .split(whereSeparator: \.isNewline)
            .last
            .map { $0.trimmingCharacters(in: .whitespaces) } ?? ""
        return (process.terminationStatus, output, reason)
    }
}
