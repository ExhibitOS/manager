// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class NativeSavedHeadStaticReadinessTests: XCTestCase {
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
    func testFixtureFirstSealUsesExactOrdinaryAdmission() {
        let fixtureFirst = "fbbb3a8736fbfce90845e167074b1ce0752e9d886093b21ed01f7ce2994603ac"
        let ordinary = "7875816a0d59f73db6cc1cac1f046a147a677cdee1527e18b120d9f1ff9119a6"
        #if DEBUG && os(macOS)
        XCTAssertTrue(AuthoringAPI.supportsOrdinaryFixtureFirst121(manifest: fixtureFirst))
        XCTAssertTrue(AuthoringAPI.supportsOrdinarySavedHeadStaticRuntime(manifest: fixtureFirst))
        XCTAssertTrue(AuthoringAPI.supportsOrdinaryDefaultFurnishings121(manifest: ordinary))
        #else
        XCTAssertFalse(AuthoringAPI.supportsOrdinaryFixtureFirst121(manifest: fixtureFirst))
        #endif
        XCTAssertFalse(AuthoringAPI.supportsOrdinaryFixtureFirst121(manifest: ordinary))
        XCTAssertFalse(AuthoringAPI.supportsOrdinaryFixtureFirst121(manifest: fixtureFirst.uppercased()))
        XCTAssertFalse(AuthoringAPI.supportsOrdinaryFixtureFirst121(manifest: "0" + fixtureFirst.dropFirst()))
        XCTAssertFalse(AuthoringAPI.supportsOrdinaryFixtureFirst121(manifest: nil))
        XCTAssertFalse(AuthoringAPI.supportsOrdinaryDefaultFurnishings121(manifest: fixtureFirst))
        XCTAssertFalse(AuthoringAPI.supportsOwnerLocalOriginalRuntime(manifest: fixtureFirst))
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
