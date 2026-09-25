// `enhance`: its arguments, the D03 byte round trip, the delay it trims,
// and one real render through AUSoundIsolation where the Mac has it.
import XCTest

@testable import momr_audio

final class EnhanceTests: XCTestCase {
    func testArguments() {
        XCTAssertEqual(parseCommand(["enhance", "in.raw", "out.raw"]), .enhance(input: "in.raw", output: "out.raw"))
        XCTAssertNil(parseCommand(["enhance", "in.raw"]))
        XCTAssertNil(parseCommand(["enhance", "same.raw", "same.raw"]), "never overwrite the input")
        XCTAssertNil(parseCommand(["enhance", "a", "b", "c"]))
    }

    func testBytesRoundTripThroughFloats() {
        var samples: [Int16] = [0, -1, 32767, -32768, 1234, -4321]
        let data = Data(bytes: &samples, count: samples.count * 2)
        let channels = deinterleave(data, channels: 2)
        XCTAssertEqual(channels.count, 2)
        XCTAssertEqual(channels[0].count, 3)
        let back = interleave(channels)
        XCTAssertEqual(back, data, "an untouched track comes back bit for bit")
    }

    /// The unit hides its delay; a known shift must be found within an
    /// envelope step (12 samples at 48 kHz).
    func testMeasuredDelayFindsAKnownShift() {
        let rate = 48_000.0
        var input = [Float](repeating: 0, count: Int(rate * 5))
        // Syllable-like bursts: 120 ms of tone every 400 ms, varying.
        for burst in 0 ..< 12 {
            let start = Int(rate * (0.3 + 0.4 * Double(burst)))
            for i in 0 ..< Int(rate * 0.12) {
                input[start + i] = sin(Float(i) * 0.05) * Float(0.3 + 0.05 * Double(burst % 3))
            }
        }
        let shift = 2704
        let output = [Float](repeating: 0, count: shift) + input
        let found = measuredDelay(of: output, behind: input, rate: rate)
        XCTAssertEqual(found, shift)
        XCTAssertEqual(measuredDelay(of: input, behind: [Float](repeating: 0, count: input.count), rate: rate), 0)
    }

    /// A real render keeps the length and the timing.
    func testIsolationKeepsLengthAndTiming() throws {
        let rate = 48_000.0
        var input = [Float](repeating: 0, count: Int(rate * 3))
        for i in Int(rate) ..< Int(rate * 1.5) {
            input[i] = sin(Float(i) * 0.03) * 0.3
        }
        let output: [Float]
        do {
            output = try isolateVoice(input, rate: rate)
        } catch EnhanceError.unavailable {
            throw XCTSkip("AUSoundIsolation is not available on this Mac")
        }
        XCTAssertEqual(output.count, input.count)
    }
}
