// A live round trip over a real Unix socket: a stub server speaks one state
// line, the client parses it, and a command sent back arrives intact.
import Darwin
import Foundation
import XCTest

@testable import MomrWatch

final class SocketTests: XCTestCase {
    var dir: URL!
    var listener: Int32 = -1
    var serverSock: String { dir.appendingPathComponent("momr/momr.sock").path }

    override func setUp() {
        dir = URL(fileURLWithPath: NSTemporaryDirectory())
            .appendingPathComponent("momr-watchtest-\(ProcessInfo.processInfo.processIdentifier)")
        try! FileManager.default.createDirectory(
            at: dir.appendingPathComponent("momr"),
            withIntermediateDirectories: true)
        setenv("XDG_CACHE_HOME", dir.path, 1)
        // A socket passed down by a running app would win over the test's.
        unsetenv("MOMR_SOCKET")
    }

    override func tearDown() {
        if listener >= 0 { Darwin.close(listener) }
        unsetenv("XDG_CACHE_HOME")
        try? FileManager.default.removeItem(at: dir)
    }

    func listen() throws {
        listener = Darwin.socket(AF_UNIX, SOCK_STREAM, 0)
        XCTAssertGreaterThanOrEqual(listener, 0)
        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let bytes = [UInt8](serverSock.utf8) + [0]
        XCTAssertLessThan(bytes.count, MemoryLayout.size(ofValue: addr.sun_path))
        for i in bytes.indices {
            withUnsafeMutableBytes(of: &addr.sun_path) { raw in
                raw[i] = bytes[i]
            }
        }
        let bound = withUnsafePointer(to: addr) { ptr in
            ptr.withMemoryRebound(to: sockaddr.self, capacity: 1) { sock in
                Darwin.bind(listener, sock, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        XCTAssertEqual(bound, 0)
        XCTAssertEqual(Darwin.listen(listener, 5), 0)
    }

    func testRoundTrip() throws {
        try listen()
        let line =
            #"{"state":"recording","elapsed":9,"title":"T","mic":0.5,"computer":0.25,"progress":0.0}"# + "\n"
        let received = LockedValue<String?>(nil)
        Thread.detachNewThread {
            // Connection 1 (the client's reader): one state line, then EOF.
            var peer = sockaddr()
            var len = socklen_t(MemoryLayout<sockaddr>.size)
            let fd1 = Darwin.accept(self.listener, &peer, &len)
            line.withCString { ptr in
                _ = Darwin.send(fd1, ptr, strlen(ptr), 0)
            }
            Darwin.close(fd1)
            // Later connections: the client's reconnects stay silent, the
            // command comes through `send`. select() keeps a silent peer
            // from blocking the wait.
            let end = Date().addingTimeInterval(5)
            while received.get() == nil, Date() < end {
                var peer2 = sockaddr()
                var len2 = socklen_t(MemoryLayout<sockaddr>.size)
                let fd = Darwin.accept(self.listener, &peer2, &len2)
                guard fd >= 0 else { continue }
                var rfds = fd_set()
                // fd_set is a fixed tuple; FD_SET works through it directly.
                withUnsafeMutablePointer(to: &rfds) { set in
                    __darwin_fd_set(fd, set)
                    var tv = timeval(tv_sec: 1, tv_usec: 0)
                    if Darwin.select(fd + 1, set, nil, nil, &tv) > 0 {
                        var buf = [UInt8](repeating: 0, count: 64)
                        let n = buf.withUnsafeMutableBytes { ptr in
                            Darwin.recv(fd, ptr.baseAddress, 64, 0)
                        }
                        if n > 0 {
                            received.set(String(bytes: buf.prefix(n), encoding: .utf8))
                        }
                    }
                }
                Darwin.close(fd)
            }
        }
        let client = WatchClient()
        let seen = LockedValue<[WatchState]>([])
        client.onState = { seen.set(seen.get() + [$0]) }
        Thread.detachNewThread { client.run() }
        let deadline = Date().addingTimeInterval(5)
        while !seen.get().contains(where: { $0.state == "recording" }), Date() < deadline {
            Thread.sleep(forTimeInterval: 0.05)
        }
        let recording = seen.get().first(where: { $0.state == "recording" })
        XCTAssertEqual(recording?.elapsed, 9)
        WatchClient.send("pause")
        let cmdDeadline = Date().addingTimeInterval(5)
        while received.get() == nil, Date() < cmdDeadline {
            Thread.sleep(forTimeInterval: 0.05)
        }
        XCTAssertEqual(received.get(), "pause\n")
        client.stop()
    }
}

/// A box for passing values out of detached threads.
final class LockedValue<T>: @unchecked Sendable {
    private let lock = NSLock()
    private var value: T
    init(_ value: T) { self.value = value }
    func set(_ new: T) { lock.withLock { value = new } }
    func get() -> T { lock.withLock { value } }
}
