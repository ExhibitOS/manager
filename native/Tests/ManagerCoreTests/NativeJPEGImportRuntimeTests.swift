// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class NativeJPEGImportRuntimeTests: XCTestCase {
    func testReviewedCoarseStatusIsClosedAndRetryRequiresExactDrain() throws {
        let approval = "ce328328-97fa-56cf-8fc1-6bafd4573153"
        let intent = NativeCoarsePreparationIntent(approvalId: approval, artworkRevision: 1)
        var wire: [String: Any] = [
            "id": "a968c98b-62c4-4289-b09b-534ab9c2c65e", "state": "failed", "progress": 25, "attempts": 1,
            "retryAt": "2026-01-01T00:00:00Z"
        ]
        func job() throws -> NativeImportJob {
            try JSONDecoder().decode(NativeImportJob.self, from: JSONSerialization.data(withJSONObject: wire))
        }
        XCTAssertFalse(try job().canRetry(coarse: intent))
        wire["coarsePreparation"] = NSNull()
        XCTAssertThrowsError(try job())
        var status: [String: Any] = [
            "profile": "viewer-png-box-512-v1", "approvalId": approval, "artworkRevision": 1,
            "status": "failed", "workerDrained": false
        ]
        wire["coarsePreparation"] = status
        XCTAssertFalse(try job().canRetry(coarse: intent))
        status["workerDrained"] = true
        status["artworkRevision"] = 2
        wire["coarsePreparation"] = status
        XCTAssertFalse(try job().canRetry(coarse: intent))
        status["artworkRevision"] = 1
        wire["coarsePreparation"] = status
        XCTAssertTrue(try job().canRetry(coarse: intent))
        wire.removeValue(forKey: "retryAt")
        XCTAssertFalse(try job().canRetry(coarse: intent))
        status["unverified"] = true
        wire["coarsePreparation"] = status
        XCTAssertThrowsError(try job())

        let bytes = try JSONEncoder().encode(intent)
        XCTAssertEqual(try JSONDecoder().decode(NativeCoarsePreparationIntent.self, from: bytes), intent)
        var shape = try XCTUnwrap(JSONSerialization.jsonObject(with: bytes) as? [String: Any])
        shape["coarseSha256"] = String(repeating: "a", count: 64)
        let extra = try JSONSerialization.data(withJSONObject: shape)
        XCTAssertThrowsError(try JSONDecoder().decode(NativeCoarsePreparationIntent.self, from: extra))
    }

    func testCurrentPacketRequiresReadinessAndUnknownPacketsStayClosed() {
        let current = NativeJPEGImportRuntime.currentManifestSHA256
        XCTAssertTrue(NativeJPEGImportRuntime.supports(manifestSHA256: current, ready: true))
        XCTAssertFalse(NativeJPEGImportRuntime.supports(manifestSHA256: current, ready: false))
        for manifest in [nil, "", String(repeating: "0", count: 64), current.uppercased()] {
            XCTAssertFalse(NativeJPEGImportRuntime.supports(manifestSHA256: manifest, ready: true))
        }
    }

    func testPortalICCValidationIsExactReadyAndDebugOnly() {
        let manifest = "0bea16fb7d61aa3b0e60921a7f4d8d702025e4857f779fd763b9633f4884b330"
        #if DEBUG && os(macOS)
        XCTAssertTrue(NativeJPEGImportRuntime.supportsICC(manifestSHA256: manifest, ready: true))
        #else
        XCTAssertFalse(NativeJPEGImportRuntime.supportsICC(manifestSHA256: manifest, ready: true))
        #endif
        XCTAssertFalse(NativeJPEGImportRuntime.supportsICC(manifestSHA256: manifest, ready: false))
        XCTAssertFalse(NativeJPEGImportRuntime.supportsICC(manifestSHA256: manifest.uppercased(), ready: true))
        XCTAssertFalse(NativeJPEGImportRuntime.supportsPortalICCValidation(
            manifestSHA256: "253de8963ec9f0e0a2884a27d8e756b453cf61899e38c31571d9993a7d6ecff4", ready: true))
    }

    func testExistingJPEGPacketsRetainedWithoutEnablingOtherFeatures() throws {
        let watermarkPacket = try XCTUnwrap(PublicationWatermarkRuntime.qualifiedManifestSHA256)
        for manifest in ["74b08c7899d74ad989bbb61e47fc86fe9fa25c8853ab27ce57e9135c4932e96a",
                         watermarkPacket,
                         "b2ce49e9703cadf7d22b73ec5dfd93622489a07c34721f9bbe2b9f6df17ee28e",
                         "859525370a4599a465a502fff15d1cb26db3961ee11ac5bf0a6f64af070c355d"] {
            XCTAssertTrue(NativeJPEGImportRuntime.supports(manifestSHA256: manifest, ready: true))
            XCTAssertFalse(NativeJPEGImportRuntime.supports(manifestSHA256: manifest, ready: false))
        }
        let current = NativeJPEGImportRuntime.currentManifestSHA256
        XCTAssertFalse(PublicationWatermarkRuntime.supports(manifestSHA256: current, ready: true))
        XCTAssertFalse(NativeModelDerivationProfile.supports(manifestSHA256: current, ready: true))
    }
}
