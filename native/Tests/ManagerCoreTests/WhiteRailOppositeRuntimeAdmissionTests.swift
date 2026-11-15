// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class WhiteRailOppositeRuntimeAdmissionTests: XCTestCase {
    func testExactContactSuccessorPreservesExistingReaderEligibility() throws {
        for manifest in ["41e741a15bedf72750de1033c3afd462503fc5901100a1660caf14ce25a980a1"] {
            #if DEBUG
            XCTAssertTrue(AuthoringAPI.supportsCombinedThumbnailRuntime(manifest: manifest))
            XCTAssertTrue(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: manifest))
            XCTAssertTrue(AuthoringAPI.supportsPinnedStartupDrain(manifest: manifest))
            #else
            XCTAssertFalse(AuthoringAPI.supportsCombinedThumbnailRuntime(manifest: manifest))
            XCTAssertFalse(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: manifest))
            XCTAssertFalse(AuthoringAPI.supportsPinnedStartupDrain(manifest: manifest))
            #endif
            let api = try AuthoringAPI(port: 18091, tenantId: "10000000-0000-4000-8000-000000000001", nativeRuntimeManifestSHA256: manifest)
            #if DEBUG
            XCTAssertTrue(api.supportsPlacedExitDoorDepthCorrection)
            #else
            XCTAssertFalse(api.supportsPlacedExitDoorDepthCorrection)
            #endif
            XCTAssertFalse(AuthoringAPI.supportsSavedHeadAuthoringRuntime(manifest: manifest))
            for unknown in [manifest.uppercased(), String(repeating: "f", count: 64)] {
                XCTAssertFalse(AuthoringAPI.supportsCombinedThumbnailRuntime(manifest: unknown))
                XCTAssertFalse(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: unknown))
                XCTAssertFalse(AuthoringAPI.supportsPinnedStartupDrain(manifest: unknown))
            }
        }
    }
}
