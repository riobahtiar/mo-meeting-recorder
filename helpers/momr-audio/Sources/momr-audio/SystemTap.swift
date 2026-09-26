// Computer-audio capture through a Core Audio process tap, read through a
// private aggregate device (macOS 14.2+). The shape follows
// insidegui/AudioCap (MIT). The tap runs at the output device's rate in
// Float32; AudioWriter resamples to the requested rate.
//
// Without `--bundle` the tap is global: every process. With bundle
// identifiers it is a mixdown of only those apps' process objects. Those
// come and go with the apps, so the process list is watched and the tap is
// torn down and made again whenever the resolved set changes; while none of
// the chosen apps runs there is no tap and nothing is written, which the app
// shows as a flat meter rather than an error.
//
// Creating the tap is the step macOS gates on System Audio Recording, so its
// failure exits 4 and the app points the user at that setting. Every later
// step fails for Core Audio reasons the user cannot fix in System Settings,
// so those exit 6 with the OSStatus on stderr, and the app can tell the two
// apart instead of sending someone to a permission that is already granted.
// Once running, buffers that keep failing to convert end in AudioWriter's 7.

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

/// The tap: every process, or a mixdown of the given process objects.
@available(macOS 14.2, *)
private func tapDescription(processes: [AudioObjectID]?) -> CATapDescription {
    let desc: CATapDescription
    if let processes {
        desc = CATapDescription(stereoMixdownOfProcesses: processes)
    } else {
        desc = CATapDescription(stereoGlobalTapButExcludeProcesses: [])
    }
    desc.name = "MOM Recorder"
    desc.isPrivate = true
    return desc
}

func runSystem(rate: Double, channels: AVAudioChannelCount, bundles: [String]) -> Int32 {
    guard #available(macOS 14.2, *) else {
        fputs("momr-audio: process taps need macOS 14.2 or newer\n", stderr)
        return 3
    }

    // A refused tap would still be created and record silence, so a
    // definite refusal is caught here (see Permission.swift).
    if tapPermission() == .denied {
        fputs(
            "momr-audio: System Audio Recording permission was refused; allow it in System Settings > Privacy & Security\n",
            stderr)
        return 4
    }

    let writer = AudioWriter(rate: rate, channels: channels)
    if bundles.isEmpty {
        let code = startTap(processes: nil, writer: writer)
        if code != 0 {
            return code
        }
    } else {
        var tapped = processObjects(forBundles: bundles)
        if !tapped.isEmpty {
            let code = startTap(processes: tapped, writer: writer)
            if code != 0 {
                return code
            }
        }
        // Apps start and quit: follow the process list and rebuild the tap
        // when the chosen set changes.
        var addr = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyProcessObjectList,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain)
        let status = AudioObjectAddPropertyListenerBlock(
            AudioObjectID(kAudioObjectSystemObject), &addr, DispatchQueue.main
        ) { _, _ in
            let now = processObjects(forBundles: bundles)
            guard Set(now) != Set(tapped) else { return }
            tapped = now
            cleanupTap()
            if !tapped.isEmpty {
                let code = startTap(processes: tapped, writer: writer)
                if code != 0 {
                    exit(code)
                }
            }
        }
        if status != noErr {
            fputs(
                "momr-audio: could not watch the process list (OSStatus \(status)); apps started later will not be heard\n",
                stderr)
        }
    }

    signal(SIGTERM, tapCleanupHandler)
    signal(SIGINT, tapCleanupHandler)
    RunLoop.main.run()
    return 0
}

/// Creates the tap, its aggregate device and the IOProc that feeds `writer`,
/// and starts it. Returns 0, or the exit code the failure deserves.
@available(macOS 14.2, *)
private func startTap(processes: [AudioObjectID]?, writer: AudioWriter) -> Int32 {
    let desc = tapDescription(processes: processes)
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
    guard status == noErr else {
        fputs(
            "momr-audio: could not read the tap format (OSStatus \(status))\n",
            stderr)
        cleanupTap()
        return 6
    }
    guard let tapFormat = AVAudioFormat(streamDescription: &tapASBD) else {
        fputs(
            "momr-audio: the tap reported a format AVAudioFormat cannot describe (\(tapASBD.mSampleRate) Hz, \(tapASBD.mChannelsPerFrame) channels, format ID \(tapASBD.mFormatID))\n",
            stderr)
        cleanupTap()
        return 6
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
        return 6
    }

    status = AudioDeviceCreateIOProcIDWithBlock(
        &procID, aggregateID, nil
    ) { _, inputData, _, _, _ in
        // A buffer list that does not match the tap format would otherwise
        // vanish here without a trace; routing it through the writer's
        // failure count makes a tap that only ever delivers such buffers end
        // with exit 7 instead of a silent track.
        guard
            let pcm = AVAudioPCMBuffer(
                pcmFormat: tapFormat, bufferListNoCopy: inputData)
        else {
            writer.dropped(
                "the tap delivered a buffer list that does not match its format (\(tapFormat))")
            return
        }
        writer.write(pcm)
    }
    guard status == noErr else {
        fputs(
            "momr-audio: could not attach to the tap (OSStatus \(status))\n",
            stderr)
        cleanupTap()
        return 6
    }

    status = AudioDeviceStart(aggregateID, procID)
    guard status == noErr else {
        fputs(
            "momr-audio: could not start the tap (OSStatus \(status))\n",
            stderr)
        cleanupTap()
        return 6
    }
    return 0
}
