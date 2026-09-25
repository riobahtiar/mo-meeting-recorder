// The Float32 to Int16 conversion and interleaving behind every capture
// path: a planar Float32 sine at 44.1 kHz must come out an interleaved Int16
// sine at 48 kHz, and out-of-range samples must saturate, not wrap. Then the
// failure rule that turns a converter which never succeeds into exit 7.
import AVFoundation
import XCTest

@testable import momr_audio

final class ConversionTests: XCTestCase {
    func testFloatPlanarToInt16Interleaved() throws {
        let sourceRate = 44_100.0
        let frames = 441
        let left = (0..<frames).map { i in
            Float(sin(2.0 * .pi * 440.0 * Double(i) / sourceRate))
        }
        // Right is silence plus one clipped sample: saturation, not wrap.
        var right = [Float](repeating: 0, count: frames)
        right[0] = 2.0
        guard
            let format = AVAudioFormat(
                commonFormat: .pcmFormatFloat32, sampleRate: sourceRate,
                channels: 2, interleaved: false),
            let buffer = AVAudioPCMBuffer(
                pcmFormat: format, frameCapacity: AVAudioFrameCount(frames))
        else {
            XCTFail("could not make the input buffer")
            return
        }
        buffer.frameLength = AVAudioFrameCount(frames)
        left.withUnsafeBufferPointer { src in
            buffer.floatChannelData![0].update(
                from: src.baseAddress!, count: frames)
        }
        right.withUnsafeBufferPointer { src in
            buffer.floatChannelData![1].update(
                from: src.baseAddress!, count: frames)
        }

        var converter = PCMConverter(rate: 48_000, channels: 2)
        let out = try converter.convert(buffer).get()
        // 441 frames at 44.1 kHz resample to ~480 at 48 kHz, stereo
        // interleaved; the resampler's priming keeps the first few frames.
        XCTAssertTrue(out.count % 2 == 0)
        XCTAssertTrue((896...960).contains(out.count), "got \(out.count)")
        // Left carries the sine, right is (almost) silent: energy differs by
        // orders of magnitude even with the clipped impulse on the right.
        let leftEnergy = stride(from: 0, to: out.count, by: 2).map {
            abs(Int(out[$0]))
        }.reduce(0, +)
        let rightEnergy = stride(from: 1, to: out.count, by: 2).map {
            abs(Int(out[$0]))
        }.reduce(0, +)
        XCTAssertGreaterThan(leftEnergy, 100 * max(rightEnergy, 1))
        // The clipped sample saturates at full scale instead of wrapping.
        XCTAssertGreaterThan(out.max()!, 32_000)
    }

    func testEmptyBufferIsAnEmptySuccess() throws {
        // A zero-length buffer is not a failure: it must not count towards
        // exit 7 when a device hands one over during a switch.
        let format = try XCTUnwrap(AVAudioFormat(
            commonFormat: .pcmFormatFloat32, sampleRate: 48_000,
            channels: 1, interleaved: false))
        let buffer = try XCTUnwrap(
            AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 16))
        buffer.frameLength = 0
        var converter = PCMConverter(rate: 48_000, channels: 1)
        XCTAssertEqual(try converter.convert(buffer).get(), [])
    }

    func testErrorsReadAsSentences() {
        XCTAssertEqual(
            ConversionError.noBuffer(frames: 512).description,
            "could not allocate an output buffer of 512 frames")
        XCTAssertTrue(
            ConversionError.convertFailed("boom").description.contains("boom"))
    }

    func testFirstFailureIsReportedThenQuiet() {
        var counter = FailureCounter(limit: 50)
        XCTAssertEqual(counter.failed("no converter"), .report)
        for _ in 2..<50 {
            XCTAssertEqual(counter.failed("no converter"), .quiet)
        }
        XCTAssertEqual(counter.consecutive, 49)
    }

    func testFiftyInARowGivesUp() {
        var counter = FailureCounter(limit: AudioWriter.giveUpAfter)
        var verdicts: [FailureCounter.Verdict] = []
        for _ in 0..<AudioWriter.giveUpAfter {
            verdicts.append(counter.failed("bad buffer"))
        }
        XCTAssertEqual(AudioWriter.giveUpAfter, 50)
        XCTAssertEqual(verdicts.first, .report)
        XCTAssertEqual(verdicts.last, .giveUp)
        XCTAssertEqual(verdicts.filter { $0 == .giveUp }.count, 1)
    }

    func testSuccessResetsTheRun() {
        var counter = FailureCounter(limit: 3)
        XCTAssertEqual(counter.failed("a"), .report)
        XCTAssertEqual(counter.failed("a"), .quiet)
        counter.succeeded()
        XCTAssertEqual(counter.consecutive, 0)
        // Failures that alternate with successes never give up, and the same
        // reason is not written again.
        for _ in 0..<10 {
            XCTAssertEqual(counter.failed("a"), .quiet)
            counter.succeeded()
        }
        // A different reason is news and gets its own line.
        XCTAssertEqual(counter.failed("b"), .report)
        XCTAssertEqual(counter.failed("b"), .quiet)
        XCTAssertEqual(counter.failed("b"), .giveUp)
    }
}
