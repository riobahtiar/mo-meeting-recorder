import Foundation

/// Recording in the AppKit shell: both captures tee raw s16le into a staging
/// folder shaped exactly like the GTK shell's (`<cache>/<started-at>/` with
/// `mic.raw` and `system.raw`), so either shell recovers the other's crash.
/// Stop encodes both tracks, writes the meeting folder and runs the core's
/// `transcribe` CLI for the transcript.
final class Recorder {
    private let mic: SourceCapture
    private let computer: SourceCapture
    private let staging: URL
    private let startedAt: Int64
    private var micFile: FileHandle?
    private var systemFile: FileHandle?
    private var pausedBegan: Date?
    private var pausedTotal: TimeInterval = 0
    private(set) var paused = false

    /// Seconds on the clock (pauses excluded).
    var elapsed: Int {
        Int(Date().timeIntervalSince1970) - Int(startedAt) - Int(pausedTotal) - pausedSoFar
    }

    private var pausedSoFar: Int {
        pausedBegan.map { Int(Date().timeIntervalSince($0)) } ?? 0
    }

    init?(mic: SourceCapture, computer: SourceCapture) {
        self.mic = mic
        self.computer = computer
        self.startedAt = Int64(Date().timeIntervalSince1970)
        let cache: URL
        if let xdg = ProcessInfo.processInfo.environment["XDG_CACHE_HOME"], xdg.hasPrefix("/") {
            cache = URL(fileURLWithPath: xdg).appendingPathComponent("momr")
        } else {
            cache = FileManager.default.homeDirectoryForCurrentUser
                .appendingPathComponent("Library/Caches/momr")
        }
        staging = cache.appendingPathComponent(String(startedAt))
        do {
            try FileManager.default.createDirectory(at: staging, withIntermediateDirectories: true)
            micFile = try Self.appending(to: staging.appendingPathComponent("mic.raw"))
            systemFile = try Self.appending(to: staging.appendingPathComponent("system.raw"))
        } catch {
            return nil
        }
        mic.recordFile = micFile
        computer.recordFile = systemFile
    }

    private static func appending(to url: URL) throws -> FileHandle {
        FileManager.default.createFile(atPath: url.path, contents: nil)
        return try FileHandle(forWritingTo: url)
    }

    func pause() {
        guard !paused else { return }
        paused = true
        pausedBegan = Date()
        try? micFile?.synchronize()
        try? systemFile?.synchronize()
        micFile?.closeFile()
        systemFile?.closeFile()
        micFile = nil
        systemFile = nil
        mic.recordFile = nil
        computer.recordFile = nil
    }

    func resume() {
        guard paused else { return }
        paused = false
        if let began = pausedBegan {
            pausedTotal += Date().timeIntervalSince(began)
        }
        pausedBegan = nil
        micFile = try? Self.appending(to: staging.appendingPathComponent("mic.raw"))
        systemFile = try? Self.appending(to: staging.appendingPathComponent("system.raw"))
        // A missing file after pause still records the other side.
        mic.recordFile = micFile
        computer.recordFile = systemFile
    }

    /// Duration from the longer raw track, like the GTK shell's recovery scan.
    private func durationSecs() -> Int64 {
        let bytesPerSec: Int64 = 48_000 * 2 * 2
        let sizes = ["mic.raw", "system.raw"].map { name in
            (try? FileManager.default.attributesOfItem(atPath: staging.appendingPathComponent(name).path)[.size] as? Int64) ?? 0
        }
        return (sizes.max() ?? 0) / bytesPerSec
    }

    // MARK: - Stop

    /// Encodes, manifests, transcribes and cleans up on a background queue;
    /// reports the meeting folder (or an error) on the main queue.
    func stop(title: String, completion: @escaping (URL?, String?) -> Void) {
        mic.recordFile = nil
        computer.recordFile = nil
        try? micFile?.synchronize()
        try? systemFile?.synchronize()
        micFile?.closeFile()
        systemFile?.closeFile()
        micFile = nil
        systemFile = nil
        let staging = staging
        let startedAt = startedAt
        let duration = durationSecs()
        DispatchQueue.global(qos: .userInitiated).async {
            let (url, error) = Self.export(staging: staging, startedAt: startedAt, duration: duration, title: title)
            DispatchQueue.main.async { completion(url, error) }
        }
    }

    private static func export(staging: URL, startedAt: Int64, duration: Int64, title: String) -> (URL?, String?) {
        guard let ffmpeg = tool("ffmpeg") else { return (nil, "ffmpeg not found") }
        let stamp: String = {
            let f = DateFormatter()
            f.dateFormat = "yyyyMMddHHmm"
            return f.string(from: Date(timeIntervalSince1970: TimeInterval(startedAt)))
        }()
        let safe = title.safeTitle()
        let meeting = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Documents/Meetings/\(stamp) \(safe)")
        let tracks = meeting.appendingPathComponent(".tracks")
        do {
            try FileManager.default.createDirectory(at: tracks, withIntermediateDirectories: true)
        } catch {
            return (nil, error.localizedDescription)
        }
        let micRaw = staging.appendingPathComponent("mic.raw").path
        let systemRaw = staging.appendingPathComponent("system.raw").path
        let micOgg = tracks.appendingPathComponent("mic.ogg").path
        let computerOgg = tracks.appendingPathComponent("computer.ogg").path
        for (raw, ogg) in [(micRaw, micOgg), (systemRaw, computerOgg)] {
            let p = Process()
            p.executableURL = ffmpeg
            p.arguments = ["-y", "-v", "error", "-f", "s16le", "-ar", "48000", "-ac", "2", "-i", raw,
                           "-c:a", "libopus", ogg]
            try? p.run()
            p.waitUntilExit()
            guard p.terminationStatus == 0 else { return (nil, "ffmpeg could not encode \(URL(fileURLWithPath: ogg).lastPathComponent)") }
        }
        // Top-level pair, like a `separate` meeting from the GTK shell.
        try? FileManager.default.copyItem(atPath: micOgg, toPath: meeting.appendingPathComponent("mic.ogg").path)
        try? FileManager.default.copyItem(atPath: computerOgg, toPath: meeting.appendingPathComponent("computer.ogg").path)
        let manifest: [String: Any?] = [
            "app": "momr",
            "version": 1,
            "title": title,
            "started_at": startedAt,
            "duration_secs": duration,
            "format": "separate",
            "language": "auto",
            "speakers": ["You", "Remote"],
            "imported": nil,
            "speaker_count": nil,
            "model": nil,
            "provider": nil,
            "chapters": [],
            "chapters_by": nil,
        ]
        do {
            let data = try JSONSerialization.data(withJSONObject: manifest, options: [.prettyPrinted, .sortedKeys])
            try data.write(to: meeting.appendingPathComponent("\(safe).meeting-recorder"))
        } catch {
            return (nil, error.localizedDescription)
        }
        // The transcript through the core: configured provider and model, as chosen.
        guard let momr = tool("momr") else { return (nil, "momr binary not found") }
        let p = Process()
        p.executableURL = momr
        p.arguments = ["transcribe", micOgg, computerOgg, "--language", "auto"]
        let out = Pipe()
        p.standardOutput = out
        p.standardError = FileHandle.nullDevice
        do {
            try p.run()
        } catch {
            return (nil, error.localizedDescription)
        }
        p.waitUntilExit()
        guard p.terminationStatus == 0 else { return (nil, "transcription failed") }
        let markdown = String(data: out.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
        try? markdown.write(to: meeting.appendingPathComponent("transcript.md"), atomically: true, encoding: .utf8)
        try? FileManager.default.removeItem(at: staging)
        return (meeting, nil)
    }

    /// A sibling tool (`momr`, `ffmpeg`): next to this executable, in the
    /// bundle's MacOS dir, or on PATH.
    static func tool(_ name: String) -> URL? {
        if name == "momr-audio" { return SourceCapture.helperURL() }
        let exe = URL(fileURLWithPath: CommandLine.arguments[0]).deletingLastPathComponent()
        for candidate in [exe.appendingPathComponent(name),
                          exe.deletingLastPathComponent().appendingPathComponent("MacOS/\(name)")] {
            if FileManager.default.isExecutableFile(atPath: candidate.path) {
                return candidate
            }
        }
        let task = Process()
        task.executableURL = URL(fileURLWithPath: "/bin/sh")
        task.arguments = ["-c", "command -v \(name)"]
        let pipe = Pipe()
        task.standardOutput = pipe
        try? task.run()
        task.waitUntilExit()
        guard let found = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8)?
            .trimmingCharacters(in: .whitespacesAndNewlines), !found.isEmpty
        else {
            return nil
        }
        return URL(fileURLWithPath: found)
    }
}

private extension String {
    /// Filename-safe title shared with the GTK shell's `safe_name`.
    func safeTitle() -> String {
        let bad: Set<Character> = ["/", "\\", ":", "*", "?", "\"", "<", ">", "|"]
        let cleaned = String(map { bad.contains($0) ? "-" : $0 })
        let trimmed = cleaned.trimmingCharacters(in: CharacterSet(charactersIn: " ."))
        return trimmed.isEmpty ? "Meeting" : trimmed
    }
}
