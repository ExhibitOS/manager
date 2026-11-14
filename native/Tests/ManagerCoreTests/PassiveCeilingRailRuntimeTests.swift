// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class PassiveCeilingRailRuntimeTests: XCTestCase {
    func testApprovedModelVariantPreviewSuccessorRetainsExistingFeatures() {
        let qualified = "ac4b4c214179eb74351d1786291d8cab53f5f7b290fca33a4d1090db43b373fd"
        XCTAssertTrue(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: qualified))
        XCTAssertTrue(AuthoringAPI.supportsCombinedThumbnailRuntime(manifest: qualified))
        XCTAssertTrue(AuthoringAPI.supportsPinnedStartupDrain(manifest: qualified))
        XCTAssertFalse(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: qualified.uppercased()))
        XCTAssertFalse(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: String(repeating: "f", count: 64)))
    }
    func testWorkspaceFailureReadoutSuccessorRetainsExistingFeatures() {
        let qualified = "abe21aec0226f969d9d22bb297b17f402d7e6d0688f5a600c57088973fb33d56"
        XCTAssertTrue(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: qualified))
        XCTAssertTrue(AuthoringAPI.supportsCombinedThumbnailRuntime(manifest: qualified))
        XCTAssertTrue(AuthoringAPI.supportsPinnedStartupDrain(manifest: qualified))
        XCTAssertFalse(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: qualified.uppercased()))
        XCTAssertFalse(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: String(repeating: "f", count: 64)))
    }
    func testAuthenticatedFurnishingReadinessSuccessorRetainsExistingFeatures() {
        let qualified = "4931d9d9819541a93eec26f8b0e6b49b91138fee6f324eb3577d862d23042156"
        XCTAssertTrue(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: qualified))
        XCTAssertTrue(AuthoringAPI.supportsCombinedThumbnailRuntime(manifest: qualified))
        XCTAssertTrue(AuthoringAPI.supportsPinnedStartupDrain(manifest: qualified))
        XCTAssertFalse(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: qualified.uppercased()))
        XCTAssertFalse(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: String(repeating: "f", count: 64)))
    }
    func testExactQualifiedRuntimeAndOldRuntimeExclusion() {
        let qualified = "f27857e0acce017f8e9594b9b5cb6cba3a1fd9d12168cc000af8efa762f25f26"
        XCTAssertTrue(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: qualified))
        XCTAssertTrue(AuthoringAPI.supportsCombinedThumbnailRuntime(manifest: qualified))
        XCTAssertTrue(AuthoringAPI.supportsPinnedStartupDrain(manifest: qualified))
        for denied in ["", "979489bc659e068e5cd4a24df36f500ca02e65c96c041cd2e9d88af127cb8c4c",
                       "76200d7f27776a9e0060c1f071cc66deeffe5de271d92a02ef4048c56493bdbd", String(repeating: "0", count: 64)] {
            XCTAssertFalse(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: denied))
        }
    }
}
