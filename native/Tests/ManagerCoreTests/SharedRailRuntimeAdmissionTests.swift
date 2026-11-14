// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class SharedRailRuntimeAdmissionTests: XCTestCase {
    func testExactSharedRailSuccessorRetainsExistingScopedConsumers() {
        let manifest = "9cc5b33143c6fc887e405f96952a8a96c4b17cde573ebf77bc77b8f72c55651b"
        #if DEBUG
        XCTAssertTrue(AuthoringAPI.supportsCombinedThumbnailRuntime(manifest: manifest))
        XCTAssertTrue(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: manifest))
        XCTAssertTrue(AuthoringAPI.supportsPinnedStartupDrain(manifest: manifest))
        #else
        XCTAssertFalse(AuthoringAPI.supportsCombinedThumbnailRuntime(manifest: manifest))
        XCTAssertFalse(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: manifest))
        XCTAssertFalse(AuthoringAPI.supportsPinnedStartupDrain(manifest: manifest))
        #endif
        XCTAssertFalse(AuthoringAPI.supportsSavedHeadAuthoringRuntime(manifest: manifest))
        for unknown in [manifest.uppercased(), String(repeating: "f", count: 64)] {
            XCTAssertFalse(AuthoringAPI.supportsCombinedThumbnailRuntime(manifest: unknown))
            XCTAssertFalse(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: unknown))
            XCTAssertFalse(AuthoringAPI.supportsPinnedStartupDrain(manifest: unknown))
            XCTAssertFalse(AuthoringAPI.supportsSavedHeadAuthoringRuntime(manifest: unknown))
        }
    }
}
