import Foundation

/// One capture source: a `momr-audio mic|system` child writing raw s16le
/// 48 kHz stereo on stdout (the D03 contract), read in 20 ms chunks with a
/// peak per chunk for the meter. Mirrors `audio.rs`: the peak is taken over
/// both channels and shown on the same -60 dB scale (`to_meter`), so the
/// two shells' meters read alike.
///
/// Like `audio.rs`, the child runs for the life of the app and is started
/// again a second after it exits on its own (tap refused, device unplugged,
/// crash). Its last stderr line becomes the source's note, shown until audio
/// flows again: a capture that ends without a word would lose half a
/// meeting while the clock keeps running.
///
/// The pipe delivers on a background queue while the recorder swaps the
/// recording file on the main queue, so the file and the partial chunk live
/// on one serial queue (`queue`) and are only touched there. A failed write
/// (a full disk) stops that track's recording and is reported once, since
/// the meeting is saved from what reached the disk.
/// Which side a capture records: the `momr-audio` subcommand, and what the
/// status line calls it.
enum Source: String {
    case mic
    case system

    var label: String {
        switch self {
        case .mic: "Microphone"
        case .system: "Computer audio"
        }
    }
}

final class SourceCapture {
    typealias LevelHandler = (Float) -> Void
    typealias NoteHandler = (String?) -> Void
    typealias WriteErrorHandler = (String) -> Void

    private let source: Source
    private var label: String { source.label }
    private var process: Process?
    /// Whether the app wants this source running: set by `start`, cleared by
    /// `stop`, so an exit after `stop` is not restarted or reported.
    private var wanted = false
    /// Owns `leftover` and `recordFile`: the reader and the recorder meet here.
    private let queue: DispatchQueue
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
    /// Called on the main queue when a write to the recording file failed.
    var onWriteError: WriteErrorHandler?

    /// Set while recording (and not paused): every byte lands here too.
    /// Only touched on `queue`.
    private var recordFile: FileHandle?
    init(_ source: Source) {
        self.source = source
        queue = DispatchQueue(label: "momr.capture.\(source.rawValue)")
    }

    /// Starts or ends teeing into `file`. Returns once no more bytes go to
    /// the previous file, so the caller may close it right after.
    func record(into file: FileHandle?) {
        queue.sync { recordFile = file }
    }

    /// Whether a capture child is running now. Main queue only.
    var isRunning: Bool {
        process?.isRunning ?? false
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
        queue.async { self.leftover.removeAll() }
    }

    private func launch() {
        guard wanted else { return }
        guard let helper = Tools.url("momr-audio") else {
            // No retry: a missing helper does not appear by itself, and each
            // look-up runs a shell.
            setNote("momr-audio helper not found — put it next to MomrApp or on PATH.")
            return
        }
        let process = Process()
        process.executableURL = helper
        process.arguments = [source.rawValue]
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
            guard let self else { return }
            self.queue.async { self.received(data) }
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

    /// On `queue`: tees `data` into the recording and turns it into levels.
    private func received(_ data: Data) {
        if let file = recordFile {
            do {
                try file.write(contentsOf: data)
            } catch {
                // Stop writing, so one error is one report, not fifty a second.
                recordFile = nil
                let message = "\(label): could not write the recording (\(error.localizedDescription))."
                DispatchQueue.main.async { [weak self] in self?.onWriteError?(message) }
            }
        }
        leftover.append(data)
        while leftover.count >= Self.chunkBytes {
            let chunk = leftover.prefix(Self.chunkBytes)
            leftover.removeFirst(Self.chunkBytes)
            let peak = Self.meterLevel(Self.peak(chunk))
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                // Audio flows again, so whatever the note said is over.
                self.setNote(nil)
                self.onLevel?(peak)
            }
        }
    }
}

extension SourceCapture {
    /// The loudest sample in `chunk` of interleaved s16le, both channels,
    /// from 0 to 1: a sound on one side only still moves the meter.
    static func peak(_ chunk: Data) -> Float {
        var top: UInt16 = 0
        chunk.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
            // Byte pairs, not a bound Int16 buffer: a Data slice need not be
            // aligned for Int16.
            var i = raw.startIndex
            while i + 1 < raw.endIndex {
                let sample = Int16(bitPattern: UInt16(raw[i]) | UInt16(raw[i + 1]) << 8)
                top = max(top, sample.magnitude)
                i += 2
            }
        }
        return Float(top) / 32768
    }

    /// `audio::to_meter`: a linear peak on a -60 dB..0 dB scale, 0..1.
    static func meterLevel(_ peak: Float) -> Float {
        guard peak > 0 else { return 0 }
        let floorDB: Float = -60
        return min(max(1 - 20 * log10(peak) / floorDB, 0), 1)
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
