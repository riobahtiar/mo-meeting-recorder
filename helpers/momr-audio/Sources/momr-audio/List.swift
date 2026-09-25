// `list`: input devices, output devices, and whether this macOS supports
// process taps, as one JSON object. The Rust app reads it for the BlackHole
// fallback (a loopback device by name) and the ready-page banner.
//
// `tap` only says the OS is 14.2 or newer. Whether System Audio Recording is
// allowed cannot be asked without creating a tap, which would prompt the user
// from a listing, so permission is only known when `system` runs and exits 4.
// The key keeps its name because the Rust side reads it.

import CoreAudio
import Foundation

private func deviceIDs() -> [AudioObjectID] {
    var addr = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyDevices,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain)
    var size: UInt32 = 0
    guard AudioObjectGetPropertyDataSize(
        AudioObjectID(kAudioObjectSystemObject), &addr, 0, nil, &size)
        == noErr
    else { return [] }
    let count = Int(size) / MemoryLayout<AudioObjectID>.size
    var ids = [AudioObjectID](repeating: kAudioObjectUnknown, count: count)
    let status = ids.withUnsafeMutableBufferPointer { buf in
        AudioObjectGetPropertyData(
            AudioObjectID(kAudioObjectSystemObject), &addr, 0, nil, &size,
            UnsafeMutableRawPointer(buf.baseAddress!))
    }
    guard status == noErr else { return [] }
    return ids
}

private func deviceName(_ id: AudioObjectID) -> String {
    var name: CFString = "" as CFString
    var size = UInt32(MemoryLayout<CFString>.size)
    var addr = AudioObjectPropertyAddress(
        mSelector: kAudioObjectPropertyName,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain)
    guard AudioObjectGetPropertyData(id, &addr, 0, nil, &size, &name) == noErr
    else { return "Unknown device" }
    return name as String
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
            inputs.append(["name": name, "channels": ins])
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
    var info: [String: Any] = [
        "inputs": inputs, "outputs": outputs, "tap": tap,
    ]
    if let blackhole {
        info["blackhole"] = blackhole
    }
    let data = try! JSONSerialization.data(
        withJSONObject: info, options: [.sortedKeys])
    print(String(data: data, encoding: .utf8)!)
    return 0
}
