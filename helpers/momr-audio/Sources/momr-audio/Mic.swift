// Microphone capture through AVAudioEngine: the default input, or with
// `--device` the device with that UID, set on the input unit before the
// engine starts. Restarts on AVAudioEngineConfigurationChange so a headset
// plugged in mid-call is followed without restarting the process. A chosen
// device that is not connected falls back to the default with a line on
// stderr, so a recording still happens.
//
// Exit codes (the table is in main.swift): 4 when the Microphone privacy
// setting refuses us, 5 when there is no input device or the engine will not
// start or restart, 7 from AudioWriter when its buffers keep failing to
// convert.

import AVFoundation
import AudioToolbox
import Darwin

/// Points the engine's input unit at the device with `uid`. False when the
/// device is not connected or the unit refuses it; the caller then records
/// the default input instead.
private func select(device uid: String, on input: AVAudioInputNode) -> Bool {
    guard let id = deviceID(forUID: uid), let unit = input.audioUnit else {
        return false
    }
    var device = id
    let status = AudioUnitSetProperty(
        unit, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0,
        &device, UInt32(MemoryLayout<AudioDeviceID>.size))
    return status == noErr
}

func runMic(rate: Double, channels: AVAudioChannelCount, device: String?) -> Int32 {
    switch AVCaptureDevice.authorizationStatus(for: .audio) {
    case .authorized:
        break
    case .notDetermined:
        var granted = false
        let sem = DispatchSemaphore(value: 0)
        AVCaptureDevice.requestAccess(for: .audio) { ok in
            granted = ok
            sem.signal()
        }
        _ = sem.wait(timeout: .now() + 30)
        if !granted {
            fputs("momr-audio: microphone permission was not granted\n", stderr)
            return 4
        }
    case .denied, .restricted:
        fputs(
            "momr-audio: microphone permission was refused; allow it in System Settings > Privacy & Security > Microphone\n",
            stderr)
        return 4
    @unknown default:
        // A status this build does not know is treated as a refusal, since
        // recording without knowing we may is worse than asking the user.
        fputs(
            "momr-audio: unknown microphone authorization status; check System Settings > Privacy & Security > Microphone\n",
            stderr)
        return 4
    }

    let engine = AVAudioEngine()
    let input = engine.inputNode
    if let device, !select(device: device, on: input) {
        fputs(
            "momr-audio: the microphone with UID \(device) is not available; recording the default input\n",
            stderr)
    }
    guard input.inputFormat(forBus: 0).channelCount > 0 else {
        fputs("momr-audio: no input device\n", stderr)
        return 5
    }
    let writer = AudioWriter(rate: rate, channels: channels)

    func start() throws {
        engine.stop()
        input.removeTap(onBus: 0)
        input.installTap(onBus: 0, bufferSize: 4096, format: nil) {
            buffer, _ in
            writer.write(buffer)
        }
        try engine.start()
    }

    do {
        try start()
    } catch {
        fputs("momr-audio: could not start the microphone: \(error)\n", stderr)
        return 5
    }
    NotificationCenter.default.addObserver(
        forName: .AVAudioEngineConfigurationChange, object: engine,
        queue: .main
    ) { _ in
        do {
            try start()
        } catch {
            fputs("momr-audio: microphone reconfigure failed: \(error)\n", stderr)
            exit(5)
        }
    }
    signal(SIGTERM) { _ in exit(0) }
    signal(SIGINT) { _ in exit(0) }
    RunLoop.main.run()
    return 0
}
