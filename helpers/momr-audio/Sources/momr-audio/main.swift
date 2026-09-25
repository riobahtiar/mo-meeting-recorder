// momr-audio: microphone and system-audio capture for MOM Recorder.
// One executable, four subcommands:
//
//   list [--rate …] [--channels …]   devices and tap support, as JSON
//   mic [--rate N] [--channels N]    default microphone as s16le on stdout
//   system [--rate N] [--channels N] process tap on all processes as s16le
//   run -- <program> <args…>         run a program, killing it when the parent exits
//
// Exit codes, shared with src/audio.rs, which maps them to the BlackHole
// fallback and the ready-page banner:
//   0 normal end (stdout closed)   2 bad arguments   3 tap unsupported
//   4 permission denied            5 no device

import AVFoundation
import Darwin
import Foundation

enum MomrCommand: Equatable {
    case list
    case mic(rate: Double, channels: AVAudioChannelCount)
    case system(rate: Double, channels: AVAudioChannelCount)
    case run(program: String, args: [String])
}

private func flag(_ name: String, in args: [String], default defaultValue: String)
    -> String?
{
    guard let i = args.firstIndex(of: name) else { return defaultValue }
    guard i + 1 < args.count else { return nil }
    return args[i + 1]
}

/// Parsed and validated arguments, or nil for usage.
func parseCommand(_ args: [String]) -> MomrCommand? {
    guard let sub = args.first else { return nil }
    switch sub {
    case "run":
        guard args.count >= 4, args[1] == "--" else { return nil }
        return .run(program: args[2], args: Array(args.dropFirst(3)))
    case "list", "mic", "system":
        guard
            let rateText = flag("--rate", in: args, default: "48000"),
            let rate = Double(rateText), rate > 0,
            let channelsText = flag("--channels", in: args, default: "2"),
            let channelsInt = Int(channelsText),
            (1...2).contains(channelsInt)
        else { return nil }
        let channels = AVAudioChannelCount(channelsInt)
        if sub == "list" {
            return .list
        }
        return sub == "mic"
            ? .mic(rate: rate, channels: channels)
            : .system(rate: rate, channels: channels)
    default:
        return nil
    }
}

private func usage() -> Never {
    fputs(
        "usage: momr-audio (list | mic [--rate N] [--channels N] | system [--rate N] [--channels N] | run -- <program> <args…>)\n",
        stderr)
    exit(2)
}

@main
struct MomrAudio {
    static func main() {
        let args = Array(CommandLine.arguments.dropFirst())
        guard let command = parseCommand(args) else { usage() }
        let code: Int32
        switch command {
        case .list:
            code = runList()
        case let .mic(rate, channels):
            code = runMic(rate: rate, channels: channels)
        case let .system(rate, channels):
            code = runSystem(rate: rate, channels: channels)
        case let .run(program, programArgs):
            code = runRun(program: program, args: programArgs)
        }
        exit(code)
    }
}
