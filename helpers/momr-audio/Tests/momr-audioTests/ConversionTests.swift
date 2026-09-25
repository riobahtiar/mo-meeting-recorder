// The Float32 to Int16 conversion and interleaving behind every capture
// path: a planar Float32 sine at 44.1 kHz must come out an interleaved Int16
// sine at 48 kHz, and out-of-range samples must saturate, not wrap.
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
        guard let out = converter.convert(buffer) else {
            XCTFail("conversion returned nil")
            return
        }
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
}
