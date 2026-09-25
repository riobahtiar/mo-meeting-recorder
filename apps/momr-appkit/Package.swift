// swift-tools-version: 5.9
// MomrApp: the native macOS shell for MOM Recorder (plan 12, D24). AppKit
// windows with SwiftUI where it is cheaper; the engine stays Rust.
// So far: live meters from momr-audio, recording with pause, and Stop
// through `momr finish`, so meetings are written by the core and open in
// both shells. The done page, Settings and recovery come in later slices.
import PackageDescription

let package = Package(
    name: "momr-appkit",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(
            name: "MomrApp",
            path: "Sources/MomrApp"
        ),
    ]
)
