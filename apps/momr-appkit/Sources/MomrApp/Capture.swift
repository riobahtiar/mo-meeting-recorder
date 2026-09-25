import Foundation

/// One capture source: a `momr-audio mic|system` child writing raw s16le
/// 48 kHz stereo on stdout (the D03 contract), read in 20 ms chunks with a
/// peak per chunk for the meter. Mirrors `audio.rs` chunk math.
///
/// Like `audio.rs`, the child runs for the life of the app and is started
/// again a second after it exits on its own (tap refused, device unplugged,
/// crash). Its last stderr line becomes the source's note, shown until audio
/// flows again: a capture that ends without a word would lose half a
/// meeting while the clock keeps running.
final class SourceCapture {
    typealias LevelHandler = (Float) -> Void
    typealias NoteHandler = (String?) -> Void

    private let subcommand: String
    private let label: String
    private var process: Process?
    /// Whether the app wants this source running: set by `start`, cleared by
    /// `stop`, so an exit after `stop` is not restarted or reported.
    private var wanted = false
    private var leftover = Data()
    /// 20 ms of stereo s16le at 48 kHz.
    private static let chunkBytes = 48_000 * 2 * 2 / 50
    /// How long an ended child waits before it is started again, as in
    /// `audio.rs`: a helper that dies at once must not spin a core.
    private static let retryDelay: TimeInterval = 1
    var onLevel: LevelHandler?
    /// Called on the main queue when `note` changes.
    var onNote: NoteHandler?
    /// Why this source is not capturing, or nil while it is. Main queue only.
    private(set) var note: String?

    /// Set while recording (and not paused): every byte lands here too.
    var recordFile: FileHandle?
    init(_ subcommand: String, label: String) {
        self.subcommand = subcommand
        self.label = label
    }

    /// Whether a capture child is running now. Main queue only.
    var isRunning: Bool {
        process?.isRunning ?? false
    }

    /// The helper next to this executable, in the bundle's MacOS dir, or on
    /// PATH — the same order the Rust shell searches.
    static func helperURL() -> URL? {
        let name = "momr-audio"
        let exe = URL(fileURLWithPath: CommandLine.arguments[0]).deletingLastPathComponent()
        let nextToExe = exe.appendingPathComponent(name)
        if FileManager.default.isExecutableFile(atPath: nextToExe.path) {
            return nextToExe
        }
        let inBundle = exe.deletingLastPathComponent().appendingPathComponent("MacOS/\(name)")
        if FileManager.default.isExecutableFile(atPath: inBundle.path) {
            return inBundle
        }
        let task = Process()
        task.executableURL = URL(fileURLWithPath: "/bin/sh")
        task.arguments = ["-c", "command -v \(name)"]
        let pipe = Pipe()
        task.standardOutput = pipe
        try? task.run()
        task.waitUntilExit()
        let found = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8)?
            .trimmingCharacters(in: .whitespacesAndNewlines)
        if let found, !found.isEmpty {
            return URL(fileURLWithPath: found)
        }
        return nil
    }

    func start() {
        stop()
        wanted = true
        launch()
    }

    func stop() {
        wanted = false
        guard let process else { return }
        self.process = nil
        // Detach first, so the exit this causes is neither reported nor
        // restarted, and no handler outlives its pipe.
        process.terminationHandler = nil
        (process.standardOutput as? Pipe)?.fileHandleForReading.readabilityHandler = nil
        (process.standardError as? Pipe)?.fileHandleForReading.readabilityHandler = nil
        if process.isRunning {
            process.terminate()
        }
        leftover.removeAll()
    }

    private func launch() {
        guard wanted else { return }
        guard let helper = Self.helperURL() else {
            // No retry: a missing helper does not appear by itself, and each
            // look-up runs a shell.
            setNote("momr-audio helper not found — put it next to MomrApp or on PATH.")
            return
        }
        let process = Process()
        process.executableURL = helper
        process.arguments = [subcommand]
        let out = Pipe()
        let err = Pipe()
        process.standardOutput = out
        process.standardError = err
        let reason = LastLine()
        out.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            guard !data.isEmpty else {
                // End of stream. Left in place, the handler fires again at
                // once, forever, with empty data.
                handle.readabilityHandler = nil
                return
            }
            self?.received(data)
        }
        err.fileHandleForReading.readabilityHandler = { handle in
            let data = handle.availableData
            guard !data.isEmpty else {
                handle.readabilityHandler = nil
                return
            }
            reason.feed(data)
        }
        process.terminationHandler = { [weak self] ended in
            DispatchQueue.main.async { self?.ended(ended, reason: reason.value) }
        }
        do {
            try process.run()
        } catch {
            out.fileHandleForReading.readabilityHandler = nil
            err.fileHandleForReading.readabilityHandler = nil
            setNote("\(label) could not start: \(error.localizedDescription)")
            retry()
            return
        }
        self.process = process
    }

    /// A child ended that nobody stopped: say why, then start it again.
    private func ended(_ ended: Process, reason: String) {
        guard wanted, ended === process else { return }
        process = nil
        let why = reason.isEmpty ? "exit code \(ended.terminationStatus)" : reason
        setNote("\(label) stopped (\(why)); retrying.")
        retry()
    }

    private func retry() {
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.retryDelay) { [weak self] in
            guard let self, self.process == nil else { return }
            self.launch()
        }
    }

    private func setNote(_ text: String?) {
        guard note != text else { return }
        note = text
        onNote?(text)
    }

    private func received(_ data: Data) {
        try? recordFile?.write(contentsOf: data)
        leftover.append(data)
        while leftover.count >= Self.chunkBytes {
            let chunk = leftover.prefix(Self.chunkBytes)
            leftover.removeFirst(Self.chunkBytes)
            let peak = chunk.withUnsafeBytes { (ptr: UnsafeRawBufferPointer) -> Float in
                let samples = ptr.bindMemory(to: Int16.self)
                var top: Int32 = 0
                // Left channel only is enough for a meter; both lanes move together.
                for i in stride(from: 0, to: samples.count, by: 2) {
                    let v = abs(Int32(samples[i]))
                    if v > top { top = v }
                }
                return Float(top) / 32768
            }
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                // Audio flows again, so whatever the note said is over.
                self.setNote(nil)
                self.onLevel?(peak)
            }
        }
    }
}

/// The last line a child wrote to stderr: the helper's reason for exiting,
/// as `audio.rs` keeps it. Fed on the pipe's queue, read on the main queue.
private final class LastLine: @unchecked Sendable {
    private let lock = NSLock()
    private var buffer = Data()
    private var last = ""

    func feed(_ data: Data) {
        lock.lock()
        defer { lock.unlock() }
        buffer.append(data)
        while let newline = buffer.firstIndex(of: UInt8(ascii: "\n")) {
            let line = String(decoding: buffer[buffer.startIndex ..< newline], as: UTF8.self)
                .trimmingCharacters(in: .whitespaces)
            buffer.removeSubrange(buffer.startIndex ... newline)
            if !line.isEmpty { last = line }
        }
        // A reason is one line; a helper that never ends one is cut short.
        if buffer.count > 4096 { buffer.removeAll() }
    }

    var value: String {
        lock.lock()
        defer { lock.unlock() }
        let tail = String(decoding: buffer, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
        return tail.isEmpty ? last : tail
    }
}
