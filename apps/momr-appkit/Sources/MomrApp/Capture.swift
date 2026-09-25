import Foundation

/// One capture source: a `momr-audio mic|system` child writing raw s16le
/// 48 kHz stereo on stdout (the D03 contract), read in 20 ms chunks with a
/// peak per chunk for the meter. Mirrors `audio.rs` chunk math.
final class SourceCapture {
    typealias LevelHandler = (Float) -> Void

    private let subcommand: String
    private var process: Process?
    private var leftover = Data()
    /// 20 ms of stereo s16le at 48 kHz.
    private static let chunkBytes = 48_000 * 2 * 2 / 50
    var onLevel: LevelHandler?

    /// Set while recording (and not paused): every byte lands here too.
    var recordFile: FileHandle?
    init(_ subcommand: String) {
        self.subcommand = subcommand
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
        guard let helper = Self.helperURL() else {
            return
        }
        let process = Process()
        process.executableURL = helper
        process.arguments = [subcommand]
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice
        pipe.fileHandleForReading.readabilityHandler = { [weak self] handle in
            self?.received(handle.availableData)
        }
        do {
            try process.run()
        } catch {
            pipe.fileHandleForReading.readabilityHandler = nil
            return
        }
        self.process = process
    }

    func stop() {
        process?.terminate()
        process = nil
        leftover.removeAll()
    }

    private func received(_ data: Data) {
        guard !data.isEmpty else { return }
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
            let handler = onLevel
            DispatchQueue.main.async { handler?(peak) }
        }
    }
}
