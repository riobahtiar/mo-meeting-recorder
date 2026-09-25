// momr-audio: microphone and system-audio capture for MOM Recorder.
// One executable, four subcommands (run lands in plan 04):
//
//   list [--rate …] [--channels …]   devices and tap support, as JSON
//   mic [--rate N] [--channels N]    default microphone as s16le on stdout
//   system [--rate N] [--channels N] process tap on all processes as s16le
//
// Exit codes, shared with src/audio.rs, which maps them to the BlackHole
// fallback and the ready-page banner:
//   0 normal end (stdout closed)   2 bad arguments   3 tap unsupported
//   4 permission denied            5 no device

import AVFoundation
import Darwin
import Foundation

private func flag(_ name: String, in args: [String], default defaultValue: String)
    -> String?
{
    guard let i = args.firstIndex(of: name) else { return defaultValue }
    guard i + 1 < args.count else { return nil }
    return args[i + 1]
}

private func usage() -> Never {
    fputs(
        "usage: momr-audio (list | mic [--rate N] [--channels N] | system [--rate N] [--channels N])\n",
        stderr)
    exit(2)
}

@main
struct MomrAudio {
    static func main() {
        let args = Array(CommandLine.arguments.dropFirst())
        guard let sub = args.first, ["list", "mic", "system"].contains(sub)
        else { usage() }
        guard
            let rateText = flag("--rate", in: args, default: "48000"),
            let rate = Double(rateText), rate > 0,
            let channelsText = flag("--channels", in: args, default: "2"),
            let channelsInt = Int(channelsText),
            (1...2).contains(channelsInt)
        else { usage() }
        let channels = AVAudioChannelCount(channelsInt)
        let code: Int32
        switch sub {
        case "list":
            code = runList()
        case "mic":
            code = runMic(rate: rate, channels: channels)
        default:
            code = runSystem(rate: rate, channels: channels)
        }
        exit(code)
    }
}
