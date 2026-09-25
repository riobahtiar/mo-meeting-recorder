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
        // The watch protocol and level history, shared with the menu bar item.
        .target(
            name: "MomrWatch",
            path: "Sources/MomrWatch"
        ),
        // The recording status in the menu bar (plan 09).
        .executableTarget(
            name: "momr-menubar",
            dependencies: ["MomrWatch"],
            path: "Sources/momr-menubar"
        ),
        .testTarget(
            name: "momr-audioTests",
            dependencies: ["momr-audio"],
            path: "Tests/momr-audioTests"
        ),
        .testTarget(
            name: "MomrWatchTests",
            dependencies: ["MomrWatch"],
            path: "Tests/MomrWatchTests"
        ),
    ]
)
