// Computer-audio capture through a Core Audio process tap on all processes,
// read through a private aggregate device (macOS 14.2+). The shape follows
// insidegui/AudioCap (MIT). The tap runs at the output device's rate in
// Float32; AudioWriter resamples to the requested rate.

import AVFoundation
import CoreAudio
import Darwin

private var tapID = AudioObjectID(kAudioObjectUnknown)
private var aggregateID = AudioObjectID(kAudioObjectUnknown)
private var procID: AudioDeviceIOProcID?

private func cleanupTap() {
    if let proc = procID, aggregateID != kAudioObjectUnknown {
        AudioDeviceStop(aggregateID, proc)
        AudioDeviceDestroyIOProcID(aggregateID, proc)
        procID = nil
    }
    if aggregateID != kAudioObjectUnknown {
        AudioHardwareDestroyAggregateDevice(aggregateID)
        aggregateID = kAudioObjectUnknown
    }
    if tapID != kAudioObjectUnknown {
        if #available(macOS 14.2, *) {
            AudioHardwareDestroyProcessTap(tapID)
        }
        tapID = kAudioObjectUnknown
    }
}

private func tapCleanupHandler(_ sig: Int32) {
    cleanupTap()
    exit(0)
}

func runSystem(rate: Double, channels: AVAudioChannelCount) -> Int32 {
    guard #available(macOS 14.2, *) else {
        fputs("momr-audio: process taps need macOS 14.2 or newer\n", stderr)
        return 3
    }

    let desc = CATapDescription(stereoGlobalTapButExcludeProcesses: [])
    desc.name = "MOM Recorder"
    desc.isPrivate = true

    var status = AudioHardwareCreateProcessTap(desc, &tapID)
    guard status == noErr else {
        fputs(
            "momr-audio: could not create the system-audio tap (OSStatus \(status)); allow System Audio Recording in System Settings > Privacy & Security\n",
            stderr)
        return 4
    }

    var tapASBD = AudioStreamBasicDescription()
    var asbdSize = UInt32(MemoryLayout<AudioStreamBasicDescription>.size)
    var formatAddr = AudioObjectPropertyAddress(
        mSelector: kAudioTapPropertyFormat,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain)
    status = AudioObjectGetPropertyData(
        tapID, &formatAddr, 0, nil, &asbdSize, &tapASBD)
    guard status == noErr, let tapFormat = AVAudioFormat(streamDescription: &tapASBD)
    else {
        fputs("momr-audio: could not read the tap format\n", stderr)
        cleanupTap()
        return 4
    }

    let aggregate: [String: Any] = [
        kAudioAggregateDeviceNameKey as String: "MOM Recorder Tap",
        kAudioAggregateDeviceUIDKey as String: UUID().uuidString,
        kAudioAggregateDeviceIsPrivateKey as String: true,
        kAudioAggregateDeviceTapAutoStartKey as String: true,
        kAudioAggregateDeviceTapListKey as String: [
            [kAudioSubTapUIDKey as String: desc.uuid.uuidString]
        ],
    ]
    status = AudioHardwareCreateAggregateDevice(
        aggregate as CFDictionary, &aggregateID)
    guard status == noErr else {
        fputs(
            "momr-audio: could not create the aggregate device (OSStatus \(status))\n",
            stderr)
        cleanupTap()
        return 4
    }

    let writer = AudioWriter(rate: rate, channels: channels)
    status = AudioDeviceCreateIOProcIDWithBlock(
        &procID, aggregateID, nil
    ) { _, inputData, _, _, _ in
        guard
            let pcm = AVAudioPCMBuffer(
                pcmFormat: tapFormat, bufferListNoCopy: inputData)
        else { return }
        writer.write(pcm)
    }
    guard status == noErr else {
        fputs(
            "momr-audio: could not attach to the tap (OSStatus \(status))\n",
            stderr)
        cleanupTap()
        return 4
    }

    status = AudioDeviceStart(aggregateID, procID)
    guard status == noErr else {
        fputs(
            "momr-audio: could not start the tap (OSStatus \(status))\n",
            stderr)
        cleanupTap()
        return 4
    }

    signal(SIGTERM, tapCleanupHandler)
    signal(SIGINT, tapCleanupHandler)
    RunLoop.main.run()
    return 0
}
