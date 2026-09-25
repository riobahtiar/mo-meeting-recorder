// momr-audio: microphone and system-audio capture for MOM Recorder.
// One executable, four subcommands:
//
//   list [--rate …] [--channels …]   devices, audio processes and tap support, as JSON
//   mic [--rate N] [--channels N] [--device UID]
//                                    a microphone as s16le on stdout: the
//                                    default input, or the device with that UID
//   system [--rate N] [--channels N] [--bundle ID]…
//                                    process tap as s16le: every process, or
//                                    only the apps with those bundle identifiers
//   run -- <program> <args…>         run a program, killing it when the parent exits
//
// Exit codes, shared with src/audio.rs, which maps them to the BlackHole
// fallback and the ready-page banner. They are an interface: change one only
// together with the Rust side.
//
//   0  normal end: stdout closed (the app went away) or SIGTERM/SIGINT.
//   2  bad arguments, or `run` could not find or start its program.
//   3  process taps are unsupported here (macOS older than 14.2).
//   4  permission denied. For the microphone this is the Microphone
//      privacy setting; for `system` it means TCC reports System Audio
//      Recording as refused, or the tap could not be created, which is
//      almost always that same permission.
//   5  no input device, or the microphone could not start or restart.
//   6  the tap failed for a Core Audio reason other than permission: reading
//      the tap format, creating the aggregate device, attaching the IOProc or
//      starting it. The OSStatus is on stderr.
//   7  audio conversion keeps failing: 50 buffers in a row could not become
//      s16le, so the recording would be silent. The reason is on stderr.
//
// `run` exits with its child's status instead, except for 2 above.

import AVFoundation
import Darwin
import Foundation

enum MomrCommand: Equatable {
    case list
    case mic(rate: Double, channels: AVAudioChannelCount, device: String?)
    case system(rate: Double, channels: AVAudioChannelCount, bundles: [String])
    case run(program: String, args: [String])
}

private func flag(_ name: String, in args: [String], default defaultValue: String)
    -> String?
{
    guard let i = args.firstIndex(of: name) else { return defaultValue }
    guard i + 1 < args.count else { return nil }
    return args[i + 1]
}

/// Every value of a repeatable flag, in order; nil when one has no value.
private func values(_ name: String, in args: [String]) -> [String]? {
    var out: [String] = []
    var i = 0
    while i < args.count {
        if args[i] == name {
            guard i + 1 < args.count else { return nil }
            out.append(args[i + 1])
            i += 2
        } else {
            i += 1
        }
    }
    return out
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
        guard let devices = values("--device", in: args), devices.count <= 1,
            let bundles = values("--bundle", in: args),
            bundles.allSatisfy({ !$0.isEmpty })
        else { return nil }
        return sub == "mic"
            ? .mic(rate: rate, channels: channels, device: devices.first)
            : .system(rate: rate, channels: channels, bundles: bundles)
    default:
        return nil
    }
}

private func usage() -> Never {
    fputs(
        "usage: momr-audio (list | mic [--rate N] [--channels N] [--device UID] | system [--rate N] [--channels N] [--bundle ID]… | run -- <program> <args…>)\n",
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
        case let .mic(rate, channels, device):
            code = runMic(rate: rate, channels: channels, device: device)
        case let .system(rate, channels, bundles):
            code = runSystem(rate: rate, channels: channels, bundles: bundles)
        case let .run(program, programArgs):
            code = runRun(program: program, args: programArgs)
        }
        exit(code)
    }
}
