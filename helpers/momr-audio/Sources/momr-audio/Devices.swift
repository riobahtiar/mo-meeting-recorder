// Core Audio lookups shared by `list`, `mic` and `system`: device UIDs, the
// device behind a UID, and the process objects (macOS 14.2+) with their pid,
// bundle identifier and whether they are playing. Everything here is a read
// of a property; nothing creates a tap or opens a device.

import AppKit
import CoreAudio
import Foundation

private let systemObject = AudioObjectID(kAudioObjectSystemObject)

private func address(_ selector: AudioObjectPropertySelector)
    -> AudioObjectPropertyAddress
{
    AudioObjectPropertyAddress(
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain)
}

/// An array-valued property of `object`, empty when it cannot be read.
func objectIDs(of object: AudioObjectID, _ selector: AudioObjectPropertySelector)
    -> [AudioObjectID]
{
    var addr = address(selector)
    var size: UInt32 = 0
    guard AudioObjectGetPropertyDataSize(object, &addr, 0, nil, &size) == noErr
    else { return [] }
    let count = Int(size) / MemoryLayout<AudioObjectID>.size
    var ids = [AudioObjectID](repeating: kAudioObjectUnknown, count: count)
    let status = ids.withUnsafeMutableBufferPointer { buf in
        AudioObjectGetPropertyData(
            object, &addr, 0, nil, &size, UnsafeMutableRawPointer(buf.baseAddress!))
    }
    guard status == noErr else { return [] }
    return ids
}

/// A string-valued property of `object`, nil when it cannot be read.
func stringProperty(of object: AudioObjectID, _ selector: AudioObjectPropertySelector)
    -> String?
{
    var value: CFString = "" as CFString
    var size = UInt32(MemoryLayout<CFString>.size)
    var addr = address(selector)
    guard AudioObjectGetPropertyData(object, &addr, 0, nil, &size, &value) == noErr
    else { return nil }
    return value as String
}

/// A 32-bit property of `object`, nil when it cannot be read.
func uint32Property(of object: AudioObjectID, _ selector: AudioObjectPropertySelector)
    -> UInt32?
{
    var value: UInt32 = 0
    var size = UInt32(MemoryLayout<UInt32>.size)
    var addr = address(selector)
    guard AudioObjectGetPropertyData(object, &addr, 0, nil, &size, &value) == noErr
    else { return nil }
    return value
}

/// The device with this UID, if it is connected.
func deviceID(forUID uid: String) -> AudioObjectID? {
    var uidRef = uid as CFString
    var device = AudioObjectID(kAudioObjectUnknown)
    var size = UInt32(MemoryLayout<AudioObjectID>.size)
    var addr = address(kAudioHardwarePropertyTranslateUIDToDevice)
    let status = withUnsafeMutablePointer(to: &uidRef) { qualifier in
        AudioObjectGetPropertyData(
            systemObject, &addr, UInt32(MemoryLayout<CFString>.size), qualifier,
            &size, &device)
    }
    guard status == noErr, device != kAudioObjectUnknown else { return nil }
    return device
}

/// One process Core Audio knows as an audio client.
struct AudioProcess: Equatable {
    let object: AudioObjectID
    let pid: pid_t
    let bundle: String
    let name: String
    let playing: Bool
}

/// Every audio process, with its bundle identifier when it has one. The
/// name comes from the running application, else from the bundle id, so
/// Settings can list "zoom.us" rather than "us.zoom.xos".
func audioProcesses() -> [AudioProcess] {
    guard #available(macOS 14.2, *) else { return [] }
    return objectIDs(of: systemObject, kAudioHardwarePropertyProcessObjectList).map {
        object in
        let pid = pid_t(
            bitPattern: uint32Property(of: object, kAudioProcessPropertyPID) ?? 0)
        let bundle = stringProperty(of: object, kAudioProcessPropertyBundleID) ?? ""
        let name =
            NSRunningApplication(processIdentifier: pid)?.localizedName
            ?? bundle.split(separator: ".").last.map(String.init) ?? ""
        let playing =
            (uint32Property(of: object, kAudioProcessPropertyIsRunningOutput) ?? 0) != 0
        return AudioProcess(
            object: object, pid: pid, bundle: bundle, name: name, playing: playing)
    }
}

/// The process objects of the apps with these bundle identifiers, for a tap
/// on only them. An app that is not running contributes nothing until the
/// process list changes and the caller asks again.
func processObjects(forBundles bundles: [String]) -> [AudioObjectID] {
    let wanted = Set(bundles)
    return audioProcesses()
        .filter { wanted.contains($0.bundle) }
        .map { $0.object }
}
