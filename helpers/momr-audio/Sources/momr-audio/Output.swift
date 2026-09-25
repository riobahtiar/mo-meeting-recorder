// The s16le contract with the Rust app: every capture path writes raw
// interleaved Int16 at the requested rate and channel count to stdout, until
// killed. Conversion and resampling go through AVAudioConverter so the tap
// (Float32, device rate) and the microphone (device format) both honour it.
// A short write means the pipe closed, i.e. the app went away: exit 0.

import AVFoundation
import Darwin

/// Converts any PCM input to interleaved Int16 at the target format.
struct PCMConverter {
    let targetRate: Double
    let targetChannels: AVAudioChannelCount
    private var converter: AVAudioConverter?
    private var sourceFormat: AVAudioFormat?

    init(rate: Double, channels: AVAudioChannelCount) {
        targetRate = rate
        targetChannels = channels
    }

    func targetFormat() -> AVAudioFormat {
        AVAudioFormat(
            commonFormat: .pcmFormatInt16, sampleRate: targetRate,
            channels: targetChannels, interleaved: true)!
    }

    /// One buffer in, one Int16 array out. Nil when the buffer cannot convert.
    mutating func convert(_ buffer: AVAudioPCMBuffer) -> [Int16]? {
        let input = buffer.format
        if converter == nil || sourceFormat != input {
            sourceFormat = input
            converter = AVAudioConverter(from: input, to: targetFormat())
        }
        guard let converter else { return nil }
        let capacity = AVAudioFrameCount(
            targetRate * Double(buffer.frameLength) / input.sampleRate + 16)
        guard let out = AVAudioPCMBuffer(
            pcmFormat: targetFormat(), frameCapacity: capacity)
        else { return nil }
        var error: NSError?
        var done = false
        _ = converter.convert(to: out, error: &error) {
            _, outStatus in
            if done {
                outStatus.pointee = .noDataNow
                return nil
            }
            done = true
            outStatus.pointee = .haveData
            return buffer
        }
        guard error == nil, out.frameLength > 0 else {
            return nil
        }
        // A single buffer reads as `.inputRanDry` once the resampler has
        // emitted what it can; the frames are valid either way.
        let ptr = out.int16ChannelData![0]
        return Array(UnsafeBufferPointer(
            start: ptr, count: Int(out.frameLength) * Int(targetChannels)))
    }
}

/// Writes converted buffers to stdout with fwrite. Ignores SIGPIPE and treats
/// a short write as "the app went away".
final class AudioWriter {
    private var converter: PCMConverter

    init(rate: Double, channels: AVAudioChannelCount) {
        converter = PCMConverter(rate: rate, channels: channels)
        signal(SIGPIPE, SIG_IGN)
    }

    func write(_ buffer: AVAudioPCMBuffer) {
        guard let samples = converter.convert(buffer) else { return }
        let count = samples.count * MemoryLayout<Int16>.size
        let written = samples.withUnsafeBytes { ptr in
            fwrite(ptr.baseAddress, 1, count, stdout)
        }
        fflush(stdout)
        if written < count {
            exit(0)
        }
    }
}
