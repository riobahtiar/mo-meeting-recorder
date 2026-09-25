import XCTest

@testable import momr_audio

final class PermissionTests: XCTestCase {
    func testPreflightCodesMap() {
        XCTAssertEqual(tapPermission(fromPreflight: 0), .granted)
        XCTAssertEqual(tapPermission(fromPreflight: 1), .denied)
        XCTAssertEqual(tapPermission(fromPreflight: 2), .unknown)
        XCTAssertEqual(tapPermission(fromPreflight: nil), .unknown)
    }
}
