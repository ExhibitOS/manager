// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class ViewerBudgetRuntimeAdmissionTests: XCTestCase {
    func testExactQualifiedSuccessorsPreserveExistingReaderEligibility() throws {
        for manifest in ["290fdd125f157e6a7c4578490f33aa205b355772256f1e535e574ffbcb589354",
                         "7af55e9ecddb2da8a76e5985fbb184e59150ca5b7c799a058cbc7d334bd48a29",
                         "09eacf46cabcc7564d0ad9dee50bf5c40f398a69667d8886e9558d6db67fdbd1",
                         "9e74b420d6c2d232294443d6db1e96a5e73828dfe164c7a74cec4e638999eb02"] {
            #if DEBUG
            XCTAssertTrue(AuthoringAPI.supportsCombinedThumbnailRuntime(manifest: manifest))
            XCTAssertTrue(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: manifest))
            XCTAssertTrue(AuthoringAPI.supportsPinnedStartupDrain(manifest: manifest))
            XCTAssertTrue(AuthoringAPI.supportsOrdinarySavedHeadStaticRuntime(manifest: manifest))
            #else
            XCTAssertFalse(AuthoringAPI.supportsCombinedThumbnailRuntime(manifest: manifest))
            XCTAssertFalse(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: manifest))
            XCTAssertFalse(AuthoringAPI.supportsPinnedStartupDrain(manifest: manifest))
            XCTAssertFalse(AuthoringAPI.supportsOrdinarySavedHeadStaticRuntime(manifest: manifest))
            #endif
            let api = try AuthoringAPI(port: 18091, tenantId: "10000000-0000-4000-8000-000000000001", nativeRuntimeManifestSHA256: manifest)
            #if DEBUG
            XCTAssertTrue(api.supportsPlacedExitDoorDepthCorrection)
            #else
            XCTAssertFalse(api.supportsPlacedExitDoorDepthCorrection)
            #endif
            XCTAssertFalse(AuthoringAPI.supportsSavedHeadAuthoringRuntime(manifest: manifest))
            for unknown in [manifest.uppercased(), String(repeating: "f", count: 64),
                            "7cfac368536f4863e36b01625c29244ece5d3a2f26238eece1475002cec8310a"] {
                XCTAssertFalse(AuthoringAPI.supportsCombinedThumbnailRuntime(manifest: unknown))
                XCTAssertFalse(AuthoringAPI.supportsPassiveCeilingRailRuntime(manifest: unknown))
                XCTAssertFalse(AuthoringAPI.supportsPinnedStartupDrain(manifest: unknown))
                XCTAssertFalse(AuthoringAPI.supportsOrdinarySavedHeadStaticRuntime(manifest: unknown))
            }
        }
    }
}
