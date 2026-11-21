// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class NativeSavedHeadStaticReadinessTests: XCTestCase {
    #if DEBUG && os(macOS)
    func testMeasuredFrameCatalog052AdmissionRequiresExactSealAndLiveFrameCapabilities() throws {
        let manifest = "4430a973077e933ba8486abd07ce0d1410546e08473b217807511db5d782bb25"
        let neighbor = "4430a973077e933ba8486abd07ce0d1410546e08473b217807511db5d782bb24"
        XCTAssertTrue(AuthoringAPI.supportsMeasuredFrameCatalog052(manifest: manifest))
        XCTAssertFalse(AuthoringAPI.supportsMeasuredFrameCatalog052(manifest: neighbor))
        XCTAssertFalse(AuthoringAPI.supportsMeasuredFrameCatalog052(manifest: nil))
        XCTAssertTrue(AuthoringAPI.supportsOrdinarySavedHeadStaticRuntime(manifest: manifest))
        XCTAssertFalse(AuthoringAPI.supportsOrdinarySavedHeadStaticRuntime(manifest: neighbor))
        XCTAssertTrue(NativeJPEGImportRuntime.supportsICC(manifestSHA256: manifest, ready: true))
        XCTAssertFalse(NativeJPEGImportRuntime.supportsICC(manifestSHA256: manifest, ready: false))
        XCTAssertFalse(NativeJPEGImportRuntime.supportsICC(manifestSHA256: neighbor, ready: true))

        let owner = "10000000-0000-4000-8000-000000000001"
        var session = AuthoringSession(userId: owner, tenantId: owner, role: "artist",
                                       csrfToken: "test", expiresAt: "2030-01-01T00:00:00Z")
        session.workspaceFurnishingProfile = "workspace-furnishing-intake-v1"
        session.workspaceFurnishingStoolProfile = "workspace-furnishing-stool-v1"
        session.workspaceFurnishingRoomVisualProfile = "workspace-furnishing-room-visual-v1"
        session.workspaceArtworkFrameProfile = "workspace-artwork-frame-v1"
        session.workspaceArtworkFrameModelsProfile = "workspace-artwork-frame-models-v1"
        session.workspaceFurnishingReusableFitProfile = "source-bound-fit-v1"
        session.nativeAuthoringProposalProfile = "saved-head-v1"
        session.nativeFrameImageFitProposalProfile = FrameImageFitReconciliationRequest.profile
        session.nativeMeasuredFrameAttachmentProposalProfile = MeasuredFrameAttachmentRequest.profile
        XCTAssertTrue(NativeAutomationSession.admitsMeasuredFrameCatalog052Session(session))
        session.nativeMeasuredFrameAttachmentProposalProfile = nil
        XCTAssertFalse(NativeAutomationSession.admitsMeasuredFrameCatalog052Session(session))
        XCTAssertTrue(NativeAutomationSession.admitsMeasuredFrameCatalog052Operation("workspace-frame-attach"))
        XCTAssertTrue(NativeAutomationSession.admitsMeasuredFrameCatalog052Operation("export-static-saved-head"))
        XCTAssertFalse(NativeAutomationSession.admitsMeasuredFrameCatalog052Operation("workspace-frame-detach"))
        XCTAssertFalse(NativeAutomationSession.admitsMeasuredFrameCatalog052Operation("export-static-native-original-publication"))
    }
    #endif

    private func policy(bounded: DocumentValue, required: DocumentValue, readOnly: Bool = false) throws -> NativeViewportPolicy {
        let value: DocumentValue = .object(["exhibitionId": .string("10000000-0000-4000-8000-000000000001"),
            "revision": .integer(117), "etag": .string("saved-etag"), "boundedReadOnly": .bool(readOnly),
            "boundedProfile": bounded, "boundedEditRequired": required])
        return try JSONDecoder().decode(NativeViewportPolicy.self, from: JSONEncoder().encode(value))
    }
    func testValidSizeBoundedProvenanceStillReportsOrdinaryExportRefusal() throws {
        let p = try policy(bounded: .bool(true), required: .bool(false))
        let value = NativeAutomationSession.savedHeadStaticReadinessProjection(runtimeSupported: true,
            publicationProfile: "bounded-local-v1", policy: p)
        XCTAssertEqual(value["publicationSessionAdmitted"], .bool(true))
        XCTAssertEqual(value["ordinaryPolicyConfirmed"], .bool(false)); XCTAssertEqual(value["available"], .bool(false))
        XCTAssertEqual(value["policy"]["boundedProfile"], .bool(true))
    }
    func testMissingCapabilityAndUnknownPolicyRemainVisibleRatherThanOrdinary() throws {
        let unknown = try policy(bounded: .null, required: .null)
        let value = NativeAutomationSession.savedHeadStaticReadinessProjection(runtimeSupported: true,
            publicationProfile: nil, policy: unknown)
        XCTAssertEqual(value["nativePublicationProfile"], .null)
        XCTAssertEqual(value["publicationSessionAdmitted"], .bool(false)); XCTAssertEqual(value["available"], .bool(false))
        XCTAssertEqual(value["policy"]["boundedProfile"], .null)
        XCTAssertEqual(value["policy"]["boundedEditRequired"], .null)
    }
    func testPositiveOrdinaryPolicyStillRequiresRuntimeAndSessionAdmission() throws {
        let p = try policy(bounded: .bool(false), required: .bool(false))
        let good = NativeAutomationSession.savedHeadStaticReadinessProjection(runtimeSupported: true,
            publicationProfile: "bounded-local-v1", policy: p)
        XCTAssertEqual(good["available"], .bool(true)); XCTAssertEqual(good["policy"]["revision"], .integer(117))
        XCTAssertEqual(NativeAutomationSession.savedHeadStaticReadinessProjection(runtimeSupported: false,
            publicationProfile: "bounded-local-v1", policy: p)["available"], .bool(false))
        XCTAssertEqual(NativeAutomationSession.savedHeadStaticReadinessProjection(runtimeSupported: true,
            publicationProfile: "unsupported", policy: p)["available"], .bool(false))
    }
}
