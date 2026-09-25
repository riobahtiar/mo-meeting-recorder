// `run` kills the child when the parent exits: spawn a shell (the parent)
// that execs the wrapper around a sleep, kill the parent, and require the
// sleep to be gone within two seconds.
import Darwin
import Foundation
import XCTest

final class RunTests: XCTestCase {
    func helperURL() -> URL? {
        var dir = URL(fileURLWithPath: #filePath)
        dir.deleteLastPathComponent() // Tests/momr-audioTests
        dir.deleteLastPathComponent() // Tests
        dir.deleteLastPathComponent() // package root
        for config in ["debug", "release"] {
            let url = dir.appendingPathComponent(".build/\(config)/momr-audio")
            if FileManager.default.isExecutableFile(atPath: url.path) {
                return url
            }
        }
        return nil
    }

    func sleepPids() -> [Int32] {
        let out = Pipe()
        let task = Process()
        task.executableURL = URL(fileURLWithPath: "/usr/bin/pgrep")
        task.arguments = ["-x", "sleep"]
        task.standardOutput = out
        task.standardError = FileHandle.nullDevice
        try? task.run()
        task.waitUntilExit()
        let text =
            String(
                data: out.fileHandleForReading.readDataToEndOfFile(),
                encoding: .utf8) ?? ""
        return text.split(separator: "\n").compactMap { Int32($0) }
    }

    func testRunKillsChildWhenParentDies() throws {
        let helper = try XCTUnwrap(helperURL(), "helper binary")
        let before = Set(sleepPids())
        let middle = Process()
        middle.executableURL = URL(fileURLWithPath: "/bin/sh")
        // An extra level: the middle shell spawns the wrapper (not exec, so
        // the wrapper keeps a parent to watch) and waits. Killing the middle
        // must kill the sleep through the surviving wrapper.
        middle.arguments = ["-c", "\(helper.path) run -- /bin/sleep 30 & wait"]
        try middle.run()
        let parent = middle.processIdentifier
        var target: Int32?
        for _ in 0..<30 {
            if let found = Set(sleepPids()).subtracting(before).first {
                target = found
                break
            }
            Thread.sleep(forTimeInterval: 0.1)
        }
        guard let target else {
            kill(parent, SIGKILL)
            return XCTFail("the wrapper spawned no sleep")
        }
        kill(parent, SIGKILL)
        let deadline = Date().addingTimeInterval(2)
        var gone = false
        while Date() < deadline {
            if !sleepPids().contains(target) {
                gone = true
                break
            }
            Thread.sleep(forTimeInterval: 0.1)
        }
        if !gone {
            kill(target, SIGKILL)
        }
        XCTAssertTrue(gone, "the grandchild survived the parent's death")
    }
}
