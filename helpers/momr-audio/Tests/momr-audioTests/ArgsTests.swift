// Argument parsing: every subcommand shape, good and bad.
import AVFoundation
import XCTest

@testable import momr_audio

final class ArgsTests: XCTestCase {
    func testCaptureDefaults() {
        XCTAssertEqual(parseCommand(["mic"]), .mic(rate: 48000, channels: 2, device: nil))
        XCTAssertEqual(
            parseCommand(["system"]), .system(rate: 48000, channels: 2, bundles: []))
        XCTAssertEqual(parseCommand(["list"]), .list)
    }

    func testCaptureFlags() {
        XCTAssertEqual(
            parseCommand(["mic", "--rate", "44100", "--channels", "1"]),
            .mic(rate: 44100, channels: 1, device: nil))
    }

    func testSourceSelection() {
        XCTAssertEqual(
            parseCommand(["mic", "--device", "BuiltInMicrophoneDevice"]),
            .mic(rate: 48000, channels: 2, device: "BuiltInMicrophoneDevice"))
        XCTAssertEqual(
            parseCommand(["system", "--bundle", "us.zoom.xos", "--bundle", "com.apple.Safari"]),
            .system(rate: 48000, channels: 2, bundles: ["us.zoom.xos", "com.apple.Safari"]))
        XCTAssertNil(parseCommand(["mic", "--device"]))
        XCTAssertNil(parseCommand(["mic", "--device", "a", "--device", "b"]))
        XCTAssertNil(parseCommand(["system", "--bundle"]))
        XCTAssertNil(parseCommand(["system", "--bundle", ""]))
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
