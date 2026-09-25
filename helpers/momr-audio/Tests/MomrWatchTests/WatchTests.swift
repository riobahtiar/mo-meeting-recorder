// The watch protocol: line parsing and the level history window.
import XCTest

@testable import MomrWatch

final class WatchTests: XCTestCase {
    func testParsesAStateLine() {
        let state = parseWatchLine(
            #"{"state":"recording","elapsed":754,"title":"Weekly","mic":0.62,"computer":0.31,"progress":0.0}"#)
        XCTAssertEqual(state?.state, "recording")
        XCTAssertEqual(state?.elapsed, 754)
        XCTAssertEqual(state?.title, "Weekly")
        XCTAssertEqual(state?.mic, 0.62)
        XCTAssertEqual(state?.computer, 0.31)
        XCTAssertEqual(state?.progress, 0.0)
    }

    func testGarbageIsNil() {
        XCTAssertNil(parseWatchLine("not json"))
        XCTAssertNil(parseWatchLine(#"{"elapsed":3}"#))
        XCTAssertNil(parseWatchLine(""))
    }

    func testHistoryKeepsItsWindow() {
        let history = LevelHistory(capacity: 4)
        for i in 0..<6 {
            history.append(mic: Double(i) / 10, computer: 0)
        }
        XCTAssertEqual(history.mic, [0.2, 0.3, 0.4, 0.5])
        XCTAssertEqual(history.computer, [0, 0, 0, 0])
        history.clear()
        XCTAssertTrue(history.mic.isEmpty)
    }
}
