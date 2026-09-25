// swift-tools-version: 5.9
// momr-audio: capture helper for MOM Recorder (plan 03). Swift, because the
// process tap and AVAudioEngine are Objective-C/Swift APIs; binding them from
// Rust would cost a large dependency tree for a few hundred lines.
import PackageDescription

let package = Package(
    name: "momr-audio",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(
            name: "momr-audio",
            path: "Sources/momr-audio"
        ),
        .testTarget(
            name: "momr-audioTests",
            dependencies: ["momr-audio"],
            path: "Tests/momr-audioTests"
        ),
    ]
)
