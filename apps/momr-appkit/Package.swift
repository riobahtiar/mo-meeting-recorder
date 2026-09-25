// swift-tools-version: 5.9
// MomrApp: the native macOS shell for MOM Recorder (plan 12, D24). AppKit
// windows with SwiftUI where it is cheaper; the engine stays Rust.
// Slice 7 is the ready page: live meters from momr-audio, app menu, About.
// Recording drives the same helpers and file formats the GTK shell uses,
// so meetings open in both shells.
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
