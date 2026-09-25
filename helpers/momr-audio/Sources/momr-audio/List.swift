// `list`: input devices (with the UID `mic --device` takes), output
// devices, the running audio processes (with the bundle id `system --bundle`
// takes) and whether this macOS supports process taps, as one JSON object.
// The Rust app reads it for the BlackHole fallback (a loopback device by
// name), the ready-page banner and the source pickers in Settings.
//
// `tap` only says the OS is 14.2 or newer; the key keeps its name because the
// Rust side reads it. `tap_permission` is TCC's answer for System Audio
// Recording without creating a tap (which would prompt from a listing):
// "granted", "denied" or "unknown" (see Permission.swift). Only "denied" is
// certain; "unknown" is common for apps whose tap works.

import CoreAudio
import Foundation

private func deviceIDs() -> [AudioObjectID] {
    objectIDs(of: AudioObjectID(kAudioObjectSystemObject), kAudioHardwarePropertyDevices)
}

private func deviceName(_ id: AudioObjectID) -> String {
    stringProperty(of: id, kAudioObjectPropertyName) ?? "Unknown device"
}

private func channelCount(_ id: AudioObjectID, scope: AudioObjectPropertyScope)
    -> Int
{
    var addr = AudioObjectPropertyAddress(
        mSelector: kAudioDevicePropertyStreamConfiguration,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain)
    var size: UInt32 = 0
    guard AudioObjectGetPropertyDataSize(id, &addr, 0, nil, &size) == noErr
    else { return 0 }
    let list = UnsafeMutablePointer<AudioBufferList>.allocate(
        capacity: Int(size))
    defer { list.deallocate() }
    guard AudioObjectGetPropertyData(id, &addr, 0, nil, &size, list) == noErr
    else { return 0 }
    let buffers = Int(list.pointee.mNumberBuffers)
    var channels = 0
    // AudioBufferList carries its first AudioBuffer inline; the rest follow
    // it in memory.
    withUnsafePointer(to: list.pointee.mBuffers) { ptr in
        ptr.withMemoryRebound(to: AudioBuffer.self, capacity: buffers) { bufs in
            for i in 0..<buffers {
                channels += Int(bufs[i].mNumberChannels)
            }
        }
    }
    return channels
}

func runList() -> Int32 {
    var inputs: [[String: Any]] = []
    var outputs: [[String: Any]] = []
    var blackhole: String?
    for id in deviceIDs() {
        let name = deviceName(id)
        let ins = channelCount(
            id, scope: kAudioDevicePropertyScopeInput)
        let outs = channelCount(
            id, scope: kAudioDevicePropertyScopeOutput)
        if ins > 0 {
            inputs.append([
                "name": name, "channels": ins,
                "uid": stringProperty(of: id, kAudioDevicePropertyDeviceUID) ?? "",
            ])
        }
        if outs > 0 {
            outputs.append(["name": name, "channels": outs])
        }
        if name.localizedCaseInsensitiveContains("blackhole") {
            // Prefer a stereo device; the name is what ffmpeg matches on.
            if blackhole == nil || name.contains("2ch") {
                blackhole = name
            }
        }
    }
    let tap: Bool = {
        if #available(macOS 14.2, *) { return true }
        return false
    }()
    let processes: [[String: Any]] = audioProcesses().map { p in
        [
            "pid": Int(p.pid), "bundle": p.bundle, "name": p.name,
            "playing": p.playing,
        ]
    }
    var info: [String: Any] = [
        "inputs": inputs, "outputs": outputs, "tap": tap,
        "tap_permission": tapPermission().rawValue,
        "processes": processes,
    ]
    if let blackhole {
        info["blackhole"] = blackhole
    }
    let data = try! JSONSerialization.data(
        withJSONObject: info, options: [.sortedKeys])
    print(String(data: data, encoding: .utf8)!)
    return 0
}
