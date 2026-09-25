// The recording file handling and the meter math, the parts of the shell
// that decide what reaches the disk and what the meters show.
import Foundation
import XCTest

@testable import MomrApp

final class RecordingTests: XCTestCase {
    private var dir: URL!

    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("momr-appkit-tests-\(ProcessInfo.processInfo.processIdentifier)")
        try? FileManager.default.removeItem(at: dir)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: dir)
    }

    /// Pause closes the tracks and resume reopens them: what was recorded
    /// before the pause must still be there after it (it once was not).
    func testResumeAppendsToWhatWasRecorded() throws {
        let track = dir.appendingPathComponent("mic.raw")
        let first = try Recorder.appending(to: track)
        try first.write(contentsOf: Data(repeating: 1, count: 1000))
        try first.close()
        let again = try Recorder.appending(to: track)
        try again.write(contentsOf: Data(repeating: 2, count: 10))
        try again.close()
        let bytes = try Data(contentsOf: track)
        XCTAssertEqual(bytes.count, 1010)
        XCTAssertEqual(bytes.first, 1)
        XCTAssertEqual(bytes.last, 2)
    }

    /// The loudest sample of either channel, like audio.rs.
    func testPeakReadsBothChannels() {
        var samples: [Int16] = [0, 0, 100, -16384, 5, 0]
        let chunk = Data(bytes: &samples, count: samples.count * 2)
        XCTAssertEqual(SourceCapture.peak(chunk), 0.5)
        XCTAssertEqual(SourceCapture.peak(Data([0x00, 0x80])), 1, "Int16.min is full scale")
        // A slice that starts at an odd offset still reads.
        let odd = Data([0xFF] + [0x00, 0x40])[1...]
        XCTAssertEqual(SourceCapture.peak(odd), 0.5)
    }

    /// `audio::to_meter`'s scale: silence 0, full scale 1, -60 dB the floor.
    func testMeterLevelMatchesTheRustScale() {
        XCTAssertEqual(SourceCapture.meterLevel(0), 0)
        XCTAssertEqual(SourceCapture.meterLevel(1), 1)
        XCTAssertEqual(SourceCapture.meterLevel(0.001), 0, accuracy: 1e-6)
        XCTAssertEqual(SourceCapture.meterLevel(0.1), 2.0 / 3.0, accuracy: 1e-6)
    }

    /// The keys and the rule match core `audio::Sources`, so a choice saved
    /// by the GTK shell means the same here.
    func testSourcesMatchTheCore() {
        XCTAssertEqual(Sources.allCases.map(\.rawValue), ["both", "mic", "computer"])
        XCTAssertTrue(Sources.both.records(.mic) && Sources.both.records(.system))
        XCTAssertTrue(Sources.mic.records(.mic) && !Sources.mic.records(.system))
        XCTAssertTrue(!Sources.computer.records(.mic) && Sources.computer.records(.system))
    }
}
