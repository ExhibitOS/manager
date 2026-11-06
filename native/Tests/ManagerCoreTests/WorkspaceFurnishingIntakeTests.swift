// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class WorkspaceFurnishingIntakeTests: XCTestCase {
    private let intakeId = "10000000-0000-4000-8000-000000000001"
    private let requestId = "10000000-0000-4000-8000-000000000002"
    private let furnishingId = "10000000-0000-4000-8000-000000000003"
    private let placementId = "10000000-0000-4000-8000-000000000004"
    private let modelSHA256 = String(repeating: "a", count: 64)

    private func fixture() throws -> DocumentValue {
        try JSONDecoder().decode(DocumentValue.self, from: Data("""
            {
              "intakeId":"\(intakeId)","requestId":"\(requestId)","phase":"registered","retryable":false,"error":null,
              "received":{"bytes":23,"sha256":"\(modelSHA256)"},
              "declaration":{
                "kind":"pedestal","name":"Fixture","provenance":"Synthetic",
                "rights":{"holder":"Fixture","ownership":"owner","licenseId":"CC0-1.0","creditLine":"","permissions":{"display":true,"download":false,"export":false,"commercial":false}},
                "model":{"mime":"model/gltf-binary","bytes":23,"sha256":"\(modelSHA256)"}
              }
            }
            """.utf8))
    }

    private func registered() throws -> DocumentValue {
        var fixture = try fixture()
        var furnishing = fixture["declaration"]
        furnishing["id"] = .string(furnishingId)
        furnishing["version"] = .integer(1)
        fixture["furnishing"] = furnishing
        return fixture
    }

    private func intake(_ value: DocumentValue) throws -> WorkspaceFurnishingIntake {
        try JSONDecoder().decode(WorkspaceFurnishingIntake.self, from: JSONEncoder().encode(value))
    }

    func testCreateAndRegisteredDescriptorUseFlatWireFields() throws {
        let wire = try registered()
        let value = try intake(wire)
        try value.check(id: intakeId, requestId: requestId)
        let request = WorkspaceFurnishingRequest(requestId: requestId, details: value.declaration)
        var expected = wire["declaration"]
        expected["requestId"] = .string(requestId)
        XCTAssertEqual(try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(request)), expected)
        XCTAssertEqual(try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(value.furnishing)), wire["furnishing"])
    }

    func testResumeAndTerminalReceiptsBindOriginalBytesAndRequest() throws {
        var wire = try registered()
        let issued = try intake(wire)
        XCTAssertThrowsError(try issued.check(requestId: intakeId))
        wire["received"]["sha256"] = .string(String(repeating: "b", count: 64))
        XCTAssertThrowsError(try intake(wire).check())
        wire = try registered()
        wire["furnishing"]["model"]["bytes"] = .integer(24)
        XCTAssertThrowsError(try intake(wire).check())
        wire = try fixture()
        wire["phase"] = .string("failed")
        wire["retryable"] = .bool(true)
        try intake(wire).check()
        wire["received"] = .null
        XCTAssertThrowsError(try intake(wire).check())
        wire["phase"] = .string("uploading")
        wire["retryable"] = .bool(false)
        try intake(wire).check()
    }

    func testVisualPlacementIsRoomLocalAndReplaysExactly() throws {
        let furnishing = try XCTUnwrap(intake(registered()).furnishing)
        var draft = try NativeExhibitionDraft.new()
        var candidate = draft.candidate
        candidate["extensions"] = .object(["example": .string("kept")])
        try draft.replaceCandidate(candidate)
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let placement = WorkspaceFurnishingPlacement(id: placementId, roomId: room, furnishingId: furnishing.id,
            furnishingVersion: 1, position: [-1, 0, 0], yaw: 0, scale: 1)
        let changed = try XCTUnwrap(placement.applying(furnishing, to: draft))
        XCTAssertEqual(changed.candidate["extensions"]["example"], .string("kept"))
        XCTAssertEqual(changed.candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["furnishings"].items?.count, 1)
        XCTAssertEqual(changed.candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["placements"].items?.count, 1)
        XCTAssertNil(try placement.applying(furnishing, to: changed))
        let conflict = WorkspaceFurnishingPlacement(id: placementId, roomId: room, furnishingId: furnishing.id,
            furnishingVersion: 1, position: [1, 0, 0], yaw: 0, scale: 1)
        XCTAssertThrowsError(try conflict.applying(furnishing, to: changed))
        let outside = WorkspaceFurnishingPlacement(id: placementId, roomId: room, furnishingId: furnishing.id,
            furnishingVersion: 1, position: [10, 0, 0], yaw: 0, scale: 1)
        XCTAssertThrowsError(try outside.applying(furnishing, to: draft))
    }

    func testUnqualifiedRuntimeRejectsFurnishingReadBeforeNetwork() async throws {
        let api = try AuthoringAPI(port: 49151, tenantId: intakeId, nativeRuntimeManifestSHA256: modelSHA256)
        XCTAssertFalse(api.workspaceFurnishingRuntimeQualified)
        do {
            _ = try await api.workspaceFurnishingIntakes()
            XCTFail("Unqualified runtime admitted")
        } catch let failure as AuthoringFailure {
            XCTAssertEqual(failure.code, "WORKSPACE_FURNISHING_UNAVAILABLE")
        }
    }
}
