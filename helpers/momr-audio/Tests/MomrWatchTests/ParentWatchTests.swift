import Foundation
import XCTest

@testable import MomrWatch

final class ParentWatchTests: XCTestCase {
    func testParentPidComesFromTheEnvironment() {
        XCTAssertEqual(parentPid(environment: ["MOMR_PARENT_PID": "4242"]), 4242)
        XCTAssertNil(parentPid(environment: [:]))
        XCTAssertNil(parentPid(environment: ["MOMR_PARENT_PID": "abc"]))
        // launchd is never the app.
        XCTAssertNil(parentPid(environment: ["MOMR_PARENT_PID": "1"]))
    }

    func testExitOfAWatchedProcessIsSeen() throws {
        let child = Process()
        child.executableURL = URL(fileURLWithPath: "/bin/sleep")
        child.arguments = ["0.3"]
        try child.run()
        let ended = expectation(description: "exit seen")
        let queue = DispatchQueue(label: "watch")
        let source = watchProcessExit(child.processIdentifier, queue: queue) {
            ended.fulfill()
        }
        wait(for: [ended], timeout: 5)
        source.cancel()
    }

    func testAProcessAlreadyGoneIsSeenAtOnce() throws {
        let child = Process()
        child.executableURL = URL(fileURLWithPath: "/usr/bin/true")
        try child.run()
        child.waitUntilExit()
        let ended = expectation(description: "exit seen")
        let source = watchProcessExit(
            child.processIdentifier, queue: DispatchQueue(label: "gone")
        ) {
            ended.fulfill()
        }
        wait(for: [ended], timeout: 2)
        source.cancel()
    }
}
