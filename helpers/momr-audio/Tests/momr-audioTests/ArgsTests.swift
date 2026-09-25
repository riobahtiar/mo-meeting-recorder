// Argument parsing: every subcommand shape, good and bad.
import AVFoundation
import XCTest

@testable import momr_audio

final class ArgsTests: XCTestCase {
    func testCaptureDefaults() {
        XCTAssertEqual(parseCommand(["mic"]), .mic(rate: 48000, channels: 2))
        XCTAssertEqual(parseCommand(["system"]), .system(rate: 48000, channels: 2))
        XCTAssertEqual(parseCommand(["list"]), .list)
    }

    func testCaptureFlags() {
        XCTAssertEqual(
            parseCommand(["mic", "--rate", "44100", "--channels", "1"]),
            .mic(rate: 44100, channels: 1))
    }

    func testRunSubcommand() {
        XCTAssertEqual(
            parseCommand(["run", "--", "ffmpeg", "-version"]),
            .run(program: "ffmpeg", args: ["-version"]))
    }

    func testBadArguments() {
        XCTAssertNil(parseCommand([]))
        XCTAssertNil(parseCommand(["bogus"]))
        XCTAssertNil(parseCommand(["mic", "--rate", "banana"]))
        XCTAssertNil(parseCommand(["mic", "--channels", "3"]))
        XCTAssertNil(parseCommand(["run"]))
        XCTAssertNil(parseCommand(["run", "--"]))
        XCTAssertNil(parseCommand(["run", "ffmpeg"]))
    }
}
