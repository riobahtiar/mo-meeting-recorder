// Shared watch-protocol code for momr-menubar: the socket path rule (same as
// src/ipc.rs), one NDJSON line in, commands out.

import Darwin
import Foundation

/// The app's live-state socket: `~/Library/Caches/momr/momr.sock`, or under
/// `XDG_CACHE_HOME` when that is set, the same rule as the Rust side.
public func socketPath() -> String {
    if let xdg = ProcessInfo.processInfo.environment["XDG_CACHE_HOME"],
        !xdg.isEmpty
    {
        return (xdg as NSString).appendingPathComponent("momr/momr.sock")
    }
    return (NSHomeDirectory() as NSString)
        .appendingPathComponent("Library/Caches/momr/momr.sock")
}

/// One state line from `momr watch`.
public struct WatchState: Equatable {
    public var state: String
    public var elapsed: Int
    public var title: String
    public var mic: Double
    public var computer: Double
    public var progress: Double

    public static let off = WatchState(
        state: "off", elapsed: 0, title: "", mic: 0, computer: 0, progress: 0)
}

public func parseWatchLine(_ line: String) -> WatchState? {
    guard let data = line.data(using: .utf8),
        let json = try? JSONSerialization.jsonObject(with: data)
            as? [String: Any],
        let state = json["state"] as? String
    else { return nil }
    return WatchState(
        state: state,
        elapsed: json["elapsed"] as? Int ?? 0,
        title: json["title"] as? String ?? "",
        mic: json["mic"] as? Double ?? 0,
        computer: json["computer"] as? Double ?? 0,
        progress: json["progress"] as? Double ?? 0)
}

/// A rolling window of meter levels for the waveform.
public final class LevelHistory {
    public private(set) var mic: [Double] = []
    public private(set) var computer: [Double] = []
    private let capacity: Int

    public init(capacity: Int = 60) {
        self.capacity = capacity
    }

    public func append(mic: Double, computer: Double) {
        self.mic.append(mic)
        self.computer.append(computer)
        if self.mic.count > capacity {
            self.mic.removeFirst(self.mic.count - capacity)
            self.computer.removeFirst(self.computer.count - capacity)
        }
    }

    public func clear() {
        mic.removeAll()
        computer.removeAll()
    }
}

/// A connected Unix socket, or -1. One access at a time, which keeps the
/// exclusivity checker happy around `sun_path`.
func openSocket() -> Int32 {
    let fd = Darwin.socket(AF_UNIX, SOCK_STREAM, 0)
    guard fd >= 0 else { return -1 }
    var addr = sockaddr_un()
    addr.sun_family = sa_family_t(AF_UNIX)
    let bytes = [UInt8](socketPath().utf8) + [0]
    guard bytes.count <= MemoryLayout.size(ofValue: addr.sun_path) else {
        Darwin.close(fd)
        return -1
    }
    for i in bytes.indices {
        withUnsafeMutableBytes(of: &addr.sun_path) { raw in
            raw[i] = bytes[i]
        }
    }
    let connected = withUnsafePointer(to: addr) { ptr in
        ptr.withMemoryRebound(to: sockaddr.self, capacity: 1) { sock in
            Darwin.connect(fd, sock, socklen_t(MemoryLayout<sockaddr_un>.size))
        }
    }
    guard connected == 0 else {
        Darwin.close(fd)
        return -1
    }
    return fd
}

/// Reads state lines on a background thread, reconnecting while the app is
/// off, and sends one-line commands back on new connections.
public final class WatchClient {
    public var onState: ((WatchState) -> Void)?
    private var running = true

    public init() {}

    public func stop() {
        running = false
    }

    public func run() {
        while running {
            let fd = openSocket()
            if fd < 0 {
                onState?(.off)
            } else {
                readLines(fd: fd)
                Darwin.close(fd)
            }
            if running {
                Thread.sleep(forTimeInterval: 1)
            }
        }
    }

    private func readLines(fd: Int32) {
        var buffer = Data()
        var chunk = [UInt8](repeating: 0, count: 4096)
        while running {
            let n = chunk.withUnsafeMutableBytes { ptr in
                Darwin.recv(fd, ptr.baseAddress, 4096, 0)
            }
            if n <= 0 {
                onState?(.off)
                return
            }
            buffer.append(contentsOf: chunk.prefix(n))
            while let newline = buffer.firstIndex(of: UInt8(ascii: "\n")) {
                let line = buffer.prefix(upTo: newline)
                buffer.removeSubrange(...newline)
                if let text = String(data: line, encoding: .utf8),
                    let state = parseWatchLine(text)
                {
                    onState?(state)
                }
            }
        }
    }

    /// Sends one command (`start`, `stop`, `pause`, `compact`) on a fresh
    /// connection, the way `momr pause` does.
    public static func send(_ command: String) {
        let fd = openSocket()
        guard fd >= 0 else { return }
        defer { Darwin.close(fd) }
        (command + "\n").withCString { ptr in
            _ = Darwin.send(fd, ptr, strlen(ptr), 0)
        }
    }
}
