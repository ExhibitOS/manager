// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class NativeCameraPoseRuntimeAdmissionTests: XCTestCase {
    func testExactPoseSuccessorPreservesExistingReaderEligibility() {
        let manifest = "9e18823be762d34a2f272e0e26550ac6536fb9910b99c02c1a95db7ee3d5934c"
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
        }
    }
}
