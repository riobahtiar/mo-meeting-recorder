// The socket path rule must agree with src/ipc.rs byte for byte, or the menu
// bar item connects to a socket nobody listens on. These cases mirror the
// Rust rule: `MOMR_SOCKET` wins, only an absolute XDG_CACHE_HOME counts, and
// a path over 100 bytes moves to the temp dir.
import XCTest

@testable import MomrWatch

final class SocketPathTests: XCTestCase {
    let home = "/Users/someone"

    func testDefaultIsUnderLibraryCaches() {
        XCTAssertEqual(
            socketPath(environment: [:], home: home),
            "/Users/someone/Library/Caches/momr/momr.sock")
    }

    func testAbsoluteXDGCacheHomeIsUsed() {
        XCTAssertEqual(
            socketPath(environment: ["XDG_CACHE_HOME": "/tmp/cache"], home: home),
            "/tmp/cache/momr/momr.sock")
        // A trailing slash gets no second separator, as PathBuf::push.
        XCTAssertEqual(
            socketPath(environment: ["XDG_CACHE_HOME": "/tmp/cache/"], home: home),
            "/tmp/cache/momr/momr.sock")
    }

    func testRelativeOrEmptyXDGCacheHomeIsIgnored() {
        for xdg in ["cache", "./cache", "~/cache", ""] {
            XCTAssertEqual(
                socketPath(environment: ["XDG_CACHE_HOME": xdg], home: home),
                "/Users/someone/Library/Caches/momr/momr.sock", xdg)
        }
    }

    func testLongPathFallsBackToTMPDIR() {
        let long = "/" + String(repeating: "x", count: 100)
        XCTAssertEqual(
            socketPath(
                environment: ["XDG_CACHE_HOME": long, "TMPDIR": "/var/folders/ab/T/"],
                home: home),
            "/var/folders/ab/T/momr.sock")
        // A long home directory triggers it too, not only XDG.
        XCTAssertEqual(
            socketPath(environment: ["TMPDIR": "/private/tmp"], home: long),
            "/private/tmp/momr.sock")
    }

    func testLongPathWithoutTMPDIRUsesSlashTmp() {
        let long = "/" + String(repeating: "x", count: 100)
        XCTAssertEqual(
            socketPath(environment: ["XDG_CACHE_HOME": long], home: home),
            "/tmp/momr.sock")
        XCTAssertEqual(
            socketPath(environment: ["XDG_CACHE_HOME": long, "TMPDIR": ""], home: home),
            "/tmp/momr.sock")
    }

    func testExactlyOneHundredBytesStaysPut() {
        // "/momr/momr.sock" is 15 bytes, so an 85-byte base makes 100.
        let base = "/" + String(repeating: "y", count: 84)
        let path = socketPath(environment: ["XDG_CACHE_HOME": base], home: home)
        XCTAssertEqual(path.utf8.count, 100)
        XCTAssertEqual(path, base + "/momr/momr.sock")
    }

    func testMOMRSocketWins() {
        let long = "/" + String(repeating: "x", count: 100)
        XCTAssertEqual(
            socketPath(
                environment: [
                    "MOMR_SOCKET": "/run/elsewhere/app.sock",
                    "XDG_CACHE_HOME": long,
                    "TMPDIR": "/private/tmp",
                ],
                home: home),
            "/run/elsewhere/app.sock")
        // Empty means unset, so the rule applies.
        XCTAssertEqual(
            socketPath(environment: ["MOMR_SOCKET": ""], home: home),
            "/Users/someone/Library/Caches/momr/momr.sock")
    }
}
