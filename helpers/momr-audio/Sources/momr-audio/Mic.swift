// Default-microphone capture through AVAudioEngine. Restarts on
// AVAudioEngineConfigurationChange so a headset plugged in mid-call is
// followed without restarting the process.

import AVFoundation
import Darwin

func runMic(rate: Double, channels: AVAudioChannelCount) -> Int32 {
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
        return 4
    }

    let engine = AVAudioEngine()
    let input = engine.inputNode
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
