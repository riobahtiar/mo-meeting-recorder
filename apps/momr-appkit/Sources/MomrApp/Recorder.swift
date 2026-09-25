import Foundation

/// Recording in the AppKit shell: both captures tee raw s16le into a staging
/// folder shaped like the GTK shell's (`<cache>/<started-at>/` with
/// `mic.raw`, `system.raw` and the `recording.json` note), so the GTK
/// shell's recovery finishes a recording this shell never stopped. Stop runs
/// `momr finish`, the core's one writer of the meeting folder: audio in the
/// format chosen in Settings, the kept tracks, the manifest and the
/// transcript, exactly as a GTK recording gets them.
final class Recorder {
    private let mic: SourceCapture
    private let computer: SourceCapture
    private let staging: URL
    private let startedAt: Int64
    private var micFile: FileHandle?
    private var systemFile: FileHandle?
    private var pausedTotal: TimeInterval = 0

    /// Where the recording is. `stopped` is terminal: the staging folder is
    /// being finished, so pause and resume must not reopen files in it.
    private enum State {
        case recording
        case paused(since: Date)
        case stopped
    }

    private var state = State.recording

    var paused: Bool {
        if case .paused = state { return true }
        return false
    }

    /// Seconds on the clock (pauses excluded).
    var elapsed: Int {
        Int(Date().timeIntervalSince1970) - Int(startedAt) - Int(pausedTotal) - pausedSoFar
    }

    private var pausedSoFar: Int {
        if case let .paused(since) = state { return Int(Date().timeIntervalSince(since)) }
        return 0
    }

    /// Opens the staging folder and starts both tracks. Throws with the
    /// reason (disk full, no permission) for the status line.
    init(mic: SourceCapture, computer: SourceCapture, title: String) throws {
        self.mic = mic
        self.computer = computer
        startedAt = Int64(Date().timeIntervalSince1970)
        staging = Self.cache().appendingPathComponent(String(startedAt))
        try FileManager.default.createDirectory(at: staging, withIntermediateDirectories: true)
        micFile = try Self.appending(to: staging.appendingPathComponent("mic.raw"))
        systemFile = try Self.appending(to: staging.appendingPathComponent("system.raw"))
        try writeNote(title: title)
        mic.record(into: micFile)
        computer.record(into: systemFile)
    }

    /// `momr_platform::paths::cache()`: an absolute `XDG_CACHE_HOME`, else
    /// `~/Library/Caches`, with the app folder under it.
    private static func cache() -> URL {
        if let xdg = ProcessInfo.processInfo.environment["XDG_CACHE_HOME"], xdg.hasPrefix("/") {
            return URL(fileURLWithPath: xdg).appendingPathComponent("momr")
        }
        return FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Caches/momr")
    }

    /// The staging note `finish::read_note` reads. Format and language are
    /// left out: this shell has no pickers yet, so the saved settings apply.
    private func writeNote(title: String) throws {
        let note: [String: Any] = ["title": title, "started_at": startedAt]
        let data = try JSONSerialization.data(withJSONObject: note)
        try data.write(to: staging.appendingPathComponent("recording.json"))
    }

    /// A handle that writes after what `url` already holds: the file is
    /// created only when missing, and resume reopens it at its end, so a
    /// pause never costs the audio recorded before it.
    static func appending(to url: URL) throws -> FileHandle {
        if !FileManager.default.fileExists(atPath: url.path),
           !FileManager.default.createFile(atPath: url.path, contents: nil) {
            throw CocoaError(.fileWriteUnknown, userInfo: [NSFilePathErrorKey: url.path])
        }
        let handle = try FileHandle(forWritingTo: url)
        try handle.seekToEnd()
        return handle
    }

    func pause() {
        guard case .recording = state else { return }
        state = .paused(since: Date())
        closeFiles()
    }

    /// Reopens both tracks. Returns why a track could not be reopened (that
    /// side records nothing from here on), or nil when both run again.
    func resume() -> String? {
        guard case let .paused(since) = state else { return nil }
        state = .recording
        pausedTotal += Date().timeIntervalSince(since)
        var problems: [String] = []
        do {
            micFile = try Self.appending(to: staging.appendingPathComponent("mic.raw"))
        } catch {
            problems.append("Microphone: \(error.localizedDescription)")
        }
        do {
            systemFile = try Self.appending(to: staging.appendingPathComponent("system.raw"))
        } catch {
            problems.append("Computer audio: \(error.localizedDescription)")
        }
        mic.record(into: micFile)
        computer.record(into: systemFile)
        return problems.isEmpty ? nil : "Could not resume recording. " + problems.joined(separator: " ")
    }

    /// Ends the tee first, so no write can land on a handle being closed.
    private func closeFiles() {
        mic.record(into: nil)
        computer.record(into: nil)
        for file in [micFile, systemFile] {
            try? file?.synchronize()
            try? file?.close()
        }
        micFile = nil
        systemFile = nil
    }

    // MARK: - Stop

    /// What Stop produced: the meeting folder when one was written (it may
    /// hold the audio even when the transcript failed), and why anything
    /// failed.
    struct Outcome {
        var folder: URL?
        var problem: String?
    }

    /// Finishes through `momr finish` on a background queue and reports on
    /// the main queue.
    func stop(title: String, completion: @escaping (Outcome) -> Void) {
        state = .stopped
        closeFiles()
        let staging = staging
        DispatchQueue.global(qos: .userInitiated).async {
            let outcome = Self.finish(staging: staging, title: title)
            DispatchQueue.main.async { completion(outcome) }
        }
    }

    private static func finish(staging: URL, title: String) -> Outcome {
        guard let momr = Tools.url("momr") else {
            return Outcome(folder: nil, problem: "momr not found; the recording stays in \(staging.path).")
        }
        let result: (status: Int32, output: Data, reason: String)
        do {
            result = try Tools.run(momr, ["finish", staging.path, "--title", title])
        } catch {
            return Outcome(folder: nil, problem: "Could not run momr: \(error.localizedDescription)")
        }
        // The folder is the first stdout line, printed as soon as it exists.
        let folder = String(decoding: result.output, as: UTF8.self)
            .split(whereSeparator: \.isNewline)
            .first
            .map { URL(fileURLWithPath: String($0)) }
        if result.status == 0 {
            return Outcome(folder: folder, problem: nil)
        }
        let reason = result.reason.isEmpty ? "momr finish exited with \(result.status)" : result.reason
        return Outcome(folder: folder, problem: reason)
    }
}
