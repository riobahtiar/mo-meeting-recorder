// `enhance <in.raw> <out.raw>`: voice isolation for a saved track (plan 17).
//
// Reads a raw track in the D03 contract (s16le, 48 kHz, stereo), runs each
// channel through Apple's AUSoundIsolation audio unit offline, and writes
// the same format back, the same length. It runs after a recording, not
// during it: the tap path has no engine graph to insert a unit into, and a
// post-process leaves the raw track untouched for the transcript (D26) and
// for a fallback when enhancement fails. Research and measurements are in
// plans/research/voice-enhancement.md.
//
// The unit reports no latency but delays its output (measured 56 ms with
// the Voice model, 93 ms with High Quality Voice), which would pull the two
// tracks of a meeting apart. So the delay is measured on each render by
// correlating input and output envelopes, and trimmed.
//
// Exit codes, part of the interface with crates/momr-core/src/enhance.rs:
//   0  written.
//   2  bad arguments, or the input could not be read or the output written.
//   8  AUSoundIsolation is not available on this Mac.
//   9  the unit failed to render; the reason is on stderr.

import Accelerate
import AVFoundation
import Darwin

let exitEnhanceUnavailable: Int32 = 8
let exitEnhanceFailed: Int32 = 9

/// The D03 contract this subcommand reads and writes.
let enhanceRate = 48_000.0
let enhanceChannels = 2

/// How much of the isolated signal is heard. Below 100 a little of the
/// original stays under the voice, which masks the gating and warble
/// neural suppressors leave on breaths and word onsets; 10 % dry is still
/// 20 dB less noise.
let enhanceWetPercent: Float32 = 90

enum EnhanceError: Error, CustomStringConvertible {
    case unavailable
    case render(String)

    var description: String {
        switch self {
        case .unavailable: "AUSoundIsolation is not available on this Mac"
        case let .render(reason): reason
        }
    }
}

func runEnhance(input: String, output: String) -> Int32 {
    guard let data = FileManager.default.contents(atPath: input) else {
        fputs("momr-audio: cannot read \(input)\n", stderr)
        return 2
    }
    let channels = deinterleave(data, channels: enhanceChannels)
    do {
        var enhanced: [[Float]] = []
        for (index, channel) in channels.enumerated() {
            // A mono microphone arrives duplicated on both channels: one
            // render serves both, at half the cost.
            if index > 0, channel == channels[0] {
                enhanced.append(enhanced[0])
            } else {
                enhanced.append(try isolateVoice(channel, rate: enhanceRate))
            }
        }
        let bytes = interleave(enhanced)
        guard FileManager.default.createFile(atPath: output, contents: bytes) else {
            fputs("momr-audio: cannot write \(output)\n", stderr)
            return 2
        }
        return 0
    } catch EnhanceError.unavailable {
        fputs("momr-audio: \(EnhanceError.unavailable)\n", stderr)
        return exitEnhanceUnavailable
    } catch {
        fputs("momr-audio: enhance: \(error)\n", stderr)
        return exitEnhanceFailed
    }
}

/// Interleaved s16le to one Float array per channel, -1..1. A trailing
/// partial frame is dropped, as the Rust side's frame reads drop it.
func deinterleave(_ data: Data, channels: Int) -> [[Float]] {
    let frames = data.count / (2 * channels)
    var out = Array(repeating: [Float](repeating: 0, count: frames), count: channels)
    data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
        for frame in 0 ..< frames {
            for channel in 0 ..< channels {
                let at = (frame * channels + channel) * 2
                let sample = Int16(bitPattern: UInt16(raw[at]) | UInt16(raw[at + 1]) << 8)
                out[channel][frame] = Float(sample) / 32768
            }
        }
    }
    return out
}

/// One Float array per channel back to interleaved s16le, saturating.
func interleave(_ channels: [[Float]]) -> Data {
    let frames = channels.map(\.count).min() ?? 0
    var data = Data(count: frames * channels.count * 2)
    data.withUnsafeMutableBytes { (raw: UnsafeMutableRawBufferPointer) in
        for frame in 0 ..< frames {
            for (index, channel) in channels.enumerated() {
                // The same scale as `deinterleave`, so a track that goes
                // through untouched comes back bit for bit.
                let scaled = (channel[frame] * 32768).rounded()
                let sample = Int16(max(-32768, min(32767, scaled)))
                let at = (frame * channels.count + index) * 2
                let bits = UInt16(bitPattern: sample)
                raw[at] = UInt8(bits & 0xFF)
                raw[at + 1] = UInt8(bits >> 8)
            }
        }
    }
    return data
}

private var soundIsolation = AudioComponentDescription(
    componentType: kAudioUnitType_Effect,
    componentSubType: kAudioUnitSubType_AUSoundIsolation,
    componentManufacturer: kAudioUnitManufacturer_Apple,
    componentFlags: 0,
    componentFlagsMask: 0
)

/// `samples` with the voice isolated: same length, delay removed.
func isolateVoice(_ samples: [Float], rate: Double) throws -> [Float] {
    guard !samples.isEmpty else { return samples }
    guard AudioComponentFindNext(nil, &soundIsolation) != nil else {
        throw EnhanceError.unavailable
    }
    guard let format = AVAudioFormat(standardFormatWithSampleRate: rate, channels: 1) else {
        throw EnhanceError.render("no mono format at \(rate) Hz")
    }
    let engine = AVAudioEngine()
    let player = AVAudioPlayerNode()
    let unit = AVAudioUnitEffect(audioComponentDescription: soundIsolation)
    engine.attach(player)
    engine.attach(unit)
    engine.connect(player, to: unit, format: format)
    engine.connect(unit, to: engine.mainMixerNode, format: format)
    // The better model where the OS has it; the standard one otherwise.
    if #available(macOS 15.0, *) {
        AudioUnitSetParameter(
            unit.audioUnit, AudioUnitParameterID(kAUSoundIsolationParam_SoundToIsolate),
            kAudioUnitScope_Global, 0,
            AudioUnitParameterValue(kAUSoundIsolationSoundType_HighQualityVoice), 0)
    }
    AudioUnitSetParameter(
        unit.audioUnit, AudioUnitParameterID(kAUSoundIsolationParam_WetDryMixPercent),
        kAudioUnitScope_Global, 0, enhanceWetPercent, 0)

    let block: AVAudioFrameCount = 4096
    do {
        try engine.enableManualRenderingMode(.offline, format: format, maximumFrameCount: block)
        try engine.start()
    } catch {
        throw EnhanceError.render("engine: \(error.localizedDescription)")
    }
    defer { engine.stop() }

    // The input plus enough silence to flush the unit's delay out.
    let tail = Int(rate * 0.25)
    let total = samples.count + tail
    guard let input = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(total)),
          let chunk = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: block)
    else { throw EnhanceError.render("could not allocate \(total) frames") }
    input.frameLength = AVAudioFrameCount(total)
    let target = input.floatChannelData![0]
    samples.withUnsafeBufferPointer { target.update(from: $0.baseAddress!, count: samples.count) }
    (target + samples.count).update(repeating: 0, count: tail)
    player.scheduleBuffer(input, completionHandler: nil)
    player.play()

    var rendered = [Float]()
    rendered.reserveCapacity(total)
    while rendered.count < total {
        let frames = min(block, AVAudioFrameCount(total - rendered.count))
        let status: AVAudioEngineManualRenderingStatus
        do {
            status = try engine.renderOffline(frames, to: chunk)
        } catch {
            throw EnhanceError.render("render: \(error.localizedDescription)")
        }
        switch status {
        case .success:
            let out = chunk.floatChannelData![0]
            rendered.append(contentsOf: UnsafeBufferPointer(start: out, count: Int(chunk.frameLength)))
        case .insufficientDataFromInputNode, .cannotDoInCurrentContext:
            // Offline rendering pulls from the player; neither happens with a
            // scheduled buffer, but looping on them would never end.
            throw EnhanceError.render("render stalled (\(status.rawValue))")
        case .error:
            throw EnhanceError.render("render failed")
        @unknown default:
            throw EnhanceError.render("render status \(status.rawValue)")
        }
    }
    let lag = measuredDelay(of: rendered, behind: samples, rate: rate)
    return Array(rendered[lag ..< lag + samples.count])
}

/// How many samples `output` lags `input`: first coarsely, from the
/// correlation of their envelopes at 4 kHz over the first minute (up to
/// 200 ms), because isolation changes the waveform but keeps the syllables
/// where they were; then to the sample, from the waveforms themselves
/// within two envelope steps of that, since a flat envelope leaves the
/// coarse peak a millisecond or so off. A silent input has no delay worth
/// finding: 0.
func measuredDelay(of output: [Float], behind input: [Float], rate: Double) -> Int {
    let step = max(1, Int(rate / 4000))
    let maxLag = Int(rate * 0.2) / step
    let span = min(input.count, output.count - maxLag * step, Int(rate * 60)) / step
    guard span > maxLag * 2 else { return 0 }
    // Each envelope point is the mean magnitude over 5 ms, taken every
    // `step` samples: short enough to follow syllables, long enough that a
    // steady tone's own ripple does not pull the peak off.
    let width = max(step, Int(rate * 0.005))
    func envelope(_ x: [Float], count: Int) -> [Float] {
        var env = [Float](repeating: 0, count: count)
        x.withUnsafeBufferPointer { p in
            for i in 0 ..< count {
                let length = min(width, x.count - i * step)
                guard length > 0 else { break }
                var mean: Float = 0
                vDSP_meamgv(p.baseAddress! + i * step, 1, &mean, vDSP_Length(length))
                env[i] = mean
            }
        }
        // Without its mean the product rewards overlap of speech, not of level.
        var mean: Float = 0
        vDSP_meanv(env, 1, &mean, vDSP_Length(count))
        var shift = -mean
        vDSP_vsadd(env, 1, &shift, &env, 1, vDSP_Length(count))
        return env
    }
    let a = envelope(input, count: span)
    let b = envelope(output, count: span + maxLag)
    var energy: Float = 0
    vDSP_svesq(a, 1, &energy, vDSP_Length(span))
    guard energy > 1e-9 else { return 0 }
    var best = (lag: 0, score: -Float.infinity)
    b.withUnsafeBufferPointer { pb in
        for lag in 0 ... maxLag {
            var score: Float = 0
            vDSP_dotpr(a, 1, pb.baseAddress! + lag, 1, &score, vDSP_Length(span))
            if score > best.score { best = (lag, score) }
        }
    }
    let coarse = best.lag * step
    let window = span * step
    var fine = (lag: coarse, score: -Float.infinity)
    input.withUnsafeBufferPointer { pi in
        output.withUnsafeBufferPointer { po in
            for lag in max(0, coarse - 2 * step) ... coarse + 2 * step
            where lag + window <= output.count {
                var score: Float = 0
                vDSP_dotpr(pi.baseAddress!, 1, po.baseAddress! + lag, 1, &score, vDSP_Length(window))
                if score > fine.score { fine = (lag, score) }
            }
        }
    }
    return fine.lag
}
