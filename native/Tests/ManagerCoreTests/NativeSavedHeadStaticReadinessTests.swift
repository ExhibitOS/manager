// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class NativeSavedHeadStaticReadinessTests: XCTestCase {
    #if DEBUG && os(macOS)
    func testPairedWallCatalog052AdmissionRequiresExactSealAndBothLiveProposals() {
        let manifest = "4b4bcae8a1a471012db922048be51f6693cf8867c334c2e8927a1c9577176c05"
        let neighbor = "4b4bcae8a1a471012db922048be51f6693cf8867c334c2e8927a1c9577176c04"
        XCTAssertTrue(AuthoringAPI.supportsPairedWallCatalog052(manifest: manifest))
        XCTAssertFalse(AuthoringAPI.supportsPairedWallCatalog052(manifest: neighbor))
        XCTAssertFalse(AuthoringAPI.supportsPairedWallCatalog052(manifest: nil))
        XCTAssertTrue(AuthoringAPI.supportsOrdinarySavedHeadStaticRuntime(manifest: manifest))
        XCTAssertFalse(AuthoringAPI.supportsOrdinarySavedHeadStaticRuntime(manifest: neighbor))

        let owner = "10000000-0000-4000-8000-000000000001"
        var session = AuthoringSession(userId: owner, tenantId: owner, role: "artist",
                                       csrfToken: "test", expiresAt: "2030-01-01T00:00:00Z")
        session.nativeAuthoringProposalProfile = "saved-head-v1"
        session.nativeSceneAuthoringProposalProfile = "saved-head-scene-v1"
        session.nativeDoorPairAuthoringProfile = NativeDoorPairProposal.profile
        XCTAssertFalse(NativeAutomationSession.admitsPairedWallCatalog052Session(session))
        session.nativePairedWallStructureProfile = NativePairedWallStructure.profile
        XCTAssertTrue(NativeAutomationSession.admitsPairedWallCatalog052Session(session))
        session.nativeDoorPairAuthoringProfile = nil
        XCTAssertFalse(NativeAutomationSession.admitsPairedWallCatalog052Session(session))
        XCTAssertTrue(NativeAutomationSession.admitsPairedWallCatalog052Operation("workspace-door-pair-create"))
        XCTAssertTrue(NativeAutomationSession.admitsPairedWallCatalog052Operation("export-static-saved-head"))
        XCTAssertFalse(NativeAutomationSession.admitsPairedWallCatalog052Operation("workspace-frame-attach"))
        XCTAssertFalse(NativeAutomationSession.admitsPairedWallCatalog052Operation("export-static-native-original-publication"))
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
