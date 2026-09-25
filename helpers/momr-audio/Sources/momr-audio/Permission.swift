// System Audio Recording permission, read without creating a tap.
//
// A process tap that the user refused is still created, and then delivers
// digital silence, which looks exactly like a Mac playing nothing. So the
// helper asks TCC first, the way insidegui/AudioCap does: TCC.framework's
// `TCCAccessPreflight` for `kTCCServiceAudioCapture`. It is private SPI,
// looked up at run time with dlopen, so a macOS without it only loses the
// check. Only a definite "denied" is acted on: the preflight has been seen
// answering "unknown" for a terminal whose tap worked, so anything but
// denied lets the tap run and decide.

import Darwin
import Foundation

enum TapPermission: String {
    case granted, denied, unknown
}

/// TCC's preflight answer: 0 granted, 1 denied, anything else (2 is "not
/// determined") or no answer at all is unknown.
func tapPermission(fromPreflight code: Int?) -> TapPermission {
    switch code {
    case 0: return .granted
    case 1: return .denied
    default: return .unknown
    }
}

/// The current System Audio Recording permission for this process's
/// responsible app.
func tapPermission() -> TapPermission {
    tapPermission(fromPreflight: audioCapturePreflight())
}

private typealias Preflight = @convention(c) (CFString, CFDictionary?) -> Int

private func audioCapturePreflight() -> Int? {
    guard
        let handle = dlopen(
            "/System/Library/PrivateFrameworks/TCC.framework/Versions/A/TCC", RTLD_NOW),
        let symbol = dlsym(handle, "TCCAccessPreflight")
    else { return nil }
    let preflight = unsafeBitCast(symbol, to: Preflight.self)
    return preflight("kTCCServiceAudioCapture" as CFString, nil)
}
