// The s16le contract with the Rust app: every capture path writes raw
// interleaved Int16 at the requested rate and channel count to stdout, until
// killed. Conversion and resampling go through AVAudioConverter so the tap
// (Float32, device rate) and the microphone (device format) both honour it.
// A short write means the pipe closed, i.e. the app went away: exit 0.
//
// A buffer that cannot be converted is dropped, but never silently. Dropping
// one now and then costs a few milliseconds of audio, while dropping all of
// them would leave the app recording a flat track with nothing on its meters
// to explain it. So the first failure, and each new kind of failure after it,
// is written to stderr with its reason, and after `AudioWriter.giveUpAfter` failures in a row the helper
// exits 7, which the app reports instead of carrying on with silence.

import AVFoundation
import Darwin

/// Why one buffer could not become Int16, in words for stderr.
enum ConversionError: Error, Equatable, CustomStringConvertible {
    /// AVAudioConverter refused this pair of formats.
    case noConverter(from: String, to: String)
    /// The output buffer could not be allocated.
    case noBuffer(frames: AVAudioFrameCount)
    /// AVAudioConverter.convert reported an error.
    case convertFailed(String)

    var description: String {
        switch self {
        case let .noConverter(from, to):
            return "no converter from \(from) to \(to)"
        case let .noBuffer(frames):
            return "could not allocate an output buffer of \(frames) frames"
        case let .convertFailed(reason):
            return "conversion failed: \(reason)"
        }
    }
}

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

    /// One buffer in, its Int16 samples out, or the reason it failed. An
    /// empty array is a success: the resampler may hold back every frame of
    /// a very short buffer until the next one arrives.
    mutating func convert(_ buffer: AVAudioPCMBuffer)
        -> Result<[Int16], ConversionError>
    {
        let input = buffer.format
        let target = targetFormat()
        if converter == nil || sourceFormat != input {
            sourceFormat = input
            converter = AVAudioConverter(from: input, to: target)
        }
        guard let converter else {
            return .failure(
                .noConverter(from: "\(input)", to: "\(target)"))
        }
        let capacity = AVAudioFrameCount(
            targetRate * Double(buffer.frameLength) / input.sampleRate + 16)
        guard let out = AVAudioPCMBuffer(
            pcmFormat: target, frameCapacity: capacity)
        else { return .failure(.noBuffer(frames: capacity)) }
        var error: NSError?
        var done = false
        let status = converter.convert(to: out, error: &error) {
            _, outStatus in
            if done {
                outStatus.pointee = .noDataNow
                return nil
            }
            done = true
            outStatus.pointee = .haveData
            return buffer
        }
        if let error {
            return .failure(.convertFailed(error.localizedDescription))
        }
        if status == .error {
            return .failure(
                .convertFailed("the converter reported an error without a reason"))
        }
        // A single buffer reads as `.inputRanDry` once the resampler has
        // emitted what it can; the frames are valid either way.
        let ptr = out.int16ChannelData![0]
        return .success(Array(UnsafeBufferPointer(
            start: ptr, count: Int(out.frameLength) * Int(targetChannels))))
    }
}

/// Counts dropped buffers in a row and decides what each failure deserves:
/// a stderr line, nothing, or giving up. Kept apart from `AudioWriter` so the
/// rule can be tested without the process exiting.
struct FailureCounter {
    enum Verdict: Equatable {
        /// Worth a stderr line: the first failure, or a new kind of failure.
        case report
        /// The same failure again; the line already written covers it.
        case quiet
        /// `limit` failures in a row: the capture is not producing audio.
        case giveUp
    }

    let limit: Int
    private(set) var consecutive = 0
    private var lastReported: String?

    init(limit: Int) {
        self.limit = limit
    }

    /// One more failure. A reason already reported stays quiet even across
    /// a success, so a converter that fails every other buffer writes one
    /// line and not a hundred a second.
    mutating func failed(_ reason: String) -> Verdict {
        consecutive += 1
        if consecutive >= limit {
            return .giveUp
        }
        if reason != lastReported {
            lastReported = reason
            return .report
        }
        return .quiet
    }

    /// A buffer went through, so the run of failures is over.
    mutating func succeeded() {
        consecutive = 0
    }
}

/// Writes converted buffers to stdout with fwrite. Ignores SIGPIPE and treats
/// a short write as "the app went away". Each capture path calls it from one
/// audio thread at a time, which is what lets the converter and the failure
/// count go without a lock.
final class AudioWriter {
    /// About a second of audio at the tap's and the engine's buffer sizes:
    /// long enough to ride out a device switch, short enough that the app
    /// hears about a dead capture before the user does.
    static let giveUpAfter = 50

    private var converter: PCMConverter
    private var failures = FailureCounter(limit: AudioWriter.giveUpAfter)

    init(rate: Double, channels: AVAudioChannelCount) {
        converter = PCMConverter(rate: rate, channels: channels)
        signal(SIGPIPE, SIG_IGN)
    }

    func write(_ buffer: AVAudioPCMBuffer) {
        let samples: [Int16]
        switch converter.convert(buffer) {
        case let .success(converted):
            samples = converted
        case let .failure(error):
            dropped(error.description)
            return
        }
        failures.succeeded()
        guard !samples.isEmpty else { return }
        let count = samples.count * MemoryLayout<Int16>.size
        let written = samples.withUnsafeBytes { ptr in
            fwrite(ptr.baseAddress, 1, count, stdout)
        }
        fflush(stdout)
        if written < count {
            exit(0)
        }
    }

    /// A buffer that never reached the converter, or failed in it. Both
    /// count towards the same run of failures, so exit 7 means "no audio is
    /// getting through" whichever step is dropping it.
    func dropped(_ reason: String) {
        switch failures.failed(reason) {
        case .quiet:
            break
        case .report:
            fputs("momr-audio: dropped a buffer: \(reason)\n", stderr)
        case .giveUp:
            fputs(
                "momr-audio: \(failures.consecutive) buffers in a row could not be converted (last: \(reason)); stopping instead of recording silence\n",
                stderr)
            exit(7)
        }
    }
}
