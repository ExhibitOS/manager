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
        XCTAssertEqual(changed.candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["version"], .integer(1))
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

    func testFrameAttachmentSurvivesRoomPlacementAndDetachesOnlyItsDescriptor() throws {
        let pedestal = try XCTUnwrap(intake(registered()).furnishing)
        var frameWire = try registered()["furnishing"]
        frameWire["id"] = .string(intakeId)
        frameWire["kind"] = .string("artwork-frame")
        frameWire["model"]["bytes"] = .integer(Int64(WorkspaceArtworkFrameAttachment.thinBrassBytes))
        frameWire["model"]["sha256"] = .string(WorkspaceArtworkFrameAttachment.thinBrassSHA256)
        let frame = try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(frameWire))
        var draft = try NativeExhibitionDraft.new()
        let roomId = try XCTUnwrap(draft.rooms.first?["id"].string)
        let assetId = "10000000-0000-4000-8000-000000000005"
        let artwork: DocumentValue = .object([
            "kind": .string("artwork"), "revisionId": .string(requestId), "primaryAssetId": .string(assetId)
        ])
        try draft.addApprovedArtwork(artwork, roomId: roomId, placementId: placementId, transform: NativeExhibitionDraft.identity)
        var candidate = draft.candidate
        candidate["extensions"] = .object(["example": .string("kept")])
        try draft.replaceCandidate(candidate)

        let attach = WorkspaceArtworkFrameCommand.attach(.init(placementId: placementId,
            artworkRevisionId: requestId, assetId: assetId, furnishingId: frame.id,
            furnishingVersion: 1, modelSha256: WorkspaceArtworkFrameAttachment.thinBrassSHA256))
        let detach = WorkspaceArtworkFrameCommand.detach(placementId: placementId)
        let namespace = WorkspaceFurnishingPlacement.namespace
        let attached = try XCTUnwrap(attach.applying(to: draft, furnishing: frame))
        XCTAssertEqual(attached.placements, draft.placements)
        XCTAssertEqual(attached.candidate["extensions"][namespace]["placements"], .array([]))
        XCTAssertNil(try attach.applying(to: attached, furnishing: frame))
        let empty = try XCTUnwrap(detach.applying(to: attached, furnishing: nil))
        XCTAssertEqual(empty.candidate, draft.candidate)
        XCTAssertThrowsError(try detach.applying(to: empty, furnishing: nil)) {
            XCTAssertEqual(($0 as? AuthoringFailure)?.code, "FRAME_TARGET_STALE")
        }

        let roomPlacement = WorkspaceFurnishingPlacement(id: "10000000-0000-4000-8000-000000000006",
            roomId: roomId, furnishingId: pedestal.id, furnishingVersion: 1, position: [0, 0, 0], yaw: 0, scale: 1)
        let mixed = try XCTUnwrap(roomPlacement.applying(pedestal, to: attached, supportedExtensionVersion: 4))
        let mixedExtension = mixed.candidate["extensions"][namespace]
        XCTAssertEqual(mixedExtension["frameAttachments"], attached.candidate["extensions"][namespace]["frameAttachments"])
        let detached = try XCTUnwrap(detach.applying(to: mixed, furnishing: nil))
        let remaining = detached.candidate["extensions"][namespace]
        XCTAssertEqual(remaining["version"], .integer(4))
        XCTAssertEqual(remaining["frameAttachments"], .array([]))
        XCTAssertEqual(remaining["placements"], mixedExtension["placements"])
        XCTAssertEqual(remaining["furnishings"].items?.map { $0["id"].string }, [pedestal.id])
    }

    func testStoolUsesExistingIntakeWireAndRequiresExactSessionAdvertisement() throws {
        var session: DocumentValue = .object([
            "userId": .string(intakeId), "tenantId": .string(requestId), "role": .string("artist"),
            "csrfToken": .string("fixture"), "expiresAt": .string("2026-12-01T00:00:00Z"),
            "workspaceFurnishingProfile": .string("workspace-furnishing-intake-v1")
        ])
        func advertised() throws -> Bool {
            try JSONDecoder().decode(AuthoringSession.self, from: JSONEncoder().encode(session))
                .advertisesWorkspaceFurnishingStool
        }
        XCTAssertFalse(try advertised())
        session["workspaceFurnishingStoolProfile"] = .string("workspace-furnishing-stool-v2")
        XCTAssertFalse(try advertised())
        session["workspaceFurnishingStoolProfile"] = .string("workspace-furnishing-stool-v1")
        XCTAssertTrue(try advertised())
        session["workspaceFurnishingProfile"] = .null
        XCTAssertFalse(try advertised())

        var wire = try registered()
        wire["declaration"]["kind"] = .string("stool")
        wire["furnishing"]["kind"] = .string("stool")
        let value = try intake(wire)
        XCTAssertThrowsError(try value.check())
        try value.check(supportedExtensionVersion: 2)
        let request = WorkspaceFurnishingRequest(requestId: requestId, details: value.declaration)
        var expected = wire["declaration"]
        expected["requestId"] = .string(requestId)
        XCTAssertEqual(try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(request)), expected)
        XCTAssertEqual(value.furnishing?.version, 1)
    }

    func testStoolUpgradesPlacementExtensionAndPreservesLegacyRows() throws {
        let pedestal = try XCTUnwrap(intake(registered()).furnishing)
        var wire = try registered()
        wire["furnishing"]["id"] = .string("10000000-0000-4000-8000-000000000005")
        wire["furnishing"]["kind"] = .string("stool")
        let stool = try XCTUnwrap(intake(wire).furnishing)
        let draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let first = WorkspaceFurnishingPlacement(id: placementId, roomId: room, furnishingId: pedestal.id,
            furnishingVersion: 1, position: [-1, 0, 0], yaw: 0, scale: 1)
        let addStool = WorkspaceFurnishingPlacement(id: "10000000-0000-4000-8000-000000000006",
            roomId: room, furnishingId: stool.id, furnishingVersion: 1, position: [1, 0, 0], yaw: 0, scale: 1)
        let legacy = try XCTUnwrap(first.applying(pedestal, to: draft, supportedExtensionVersion: 2))
        let namespace = WorkspaceFurnishingPlacement.namespace
        let v1 = legacy.candidate["extensions"][namespace]
        XCTAssertEqual(v1["version"], .integer(1))
        XCTAssertThrowsError(try addStool.applying(stool, to: legacy)) {
            XCTAssertEqual(($0 as? AuthoringFailure)?.code, "WORKSPACE_FURNISHING_UNAVAILABLE")
        }
        let upgraded = try XCTUnwrap(addStool.applying(stool, to: legacy, supportedExtensionVersion: 2))
        let v2 = upgraded.candidate["extensions"][namespace]
        XCTAssertEqual(v2["version"], .integer(2))
        XCTAssertEqual(v2["furnishings"].items?.first, v1["furnishings"].items?.first)
        XCTAssertEqual(v2["placements"].items?.first, v1["placements"].items?.first)
        XCTAssertEqual(v2["furnishings"].items?.last, wire["furnishing"])
        XCTAssertNil(try addStool.applying(stool, to: upgraded, supportedExtensionVersion: 2))
        XCTAssertThrowsError(try first.applying(pedestal, to: upgraded))
        let another = WorkspaceFurnishingPlacement(id: "10000000-0000-4000-8000-000000000007",
            roomId: room, furnishingId: pedestal.id, furnishingVersion: 1, position: [0, 0, 1], yaw: 0, scale: 1)
        let kept = try XCTUnwrap(another.applying(pedestal, to: upgraded, supportedExtensionVersion: 2))
        XCTAssertEqual(kept.candidate["extensions"][namespace]["version"], .integer(2))
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

    func testRoomVisualAdvertisementRequiresBothParentProfiles() throws {
        var fields: DocumentValue = .object([
            "userId": .string(intakeId), "tenantId": .string(requestId), "role": .string("artist"),
            "csrfToken": .string("fixture"), "expiresAt": .string("2026-12-01T00:00:00Z"),
            "workspaceFurnishingRoomVisualProfile": .string("workspace-furnishing-room-visual-v1")
        ])
        func advertised() throws -> Bool {
            try JSONDecoder().decode(AuthoringSession.self, from: JSONEncoder().encode(fields))
                .advertisesWorkspaceFurnishingRoomVisual
        }
        XCTAssertFalse(try advertised())
        fields["workspaceFurnishingProfile"] = .string("workspace-furnishing-intake-v1")
        XCTAssertFalse(try advertised())
        fields["workspaceFurnishingStoolProfile"] = .string("workspace-furnishing-stool-v1")
        XCTAssertTrue(try advertised())
        fields["workspaceFurnishingRoomVisualProfile"] = .string("workspace-furnishing-room-visual-v2")
        XCTAssertFalse(try advertised())
    }

    func testRoomVisualKindsKeepTheIntakeWireAndUpgradeOnlyWhenPlaced() throws {
        let draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let namespace = WorkspaceFurnishingPlacement.namespace
        var stoolWire = try registered()
        stoolWire["furnishing"]["kind"] = .string("stool")
        let stool = try XCTUnwrap(intake(stoolWire).furnishing)
        let oldPose = WorkspaceFurnishingPlacement(id: placementId, roomId: room, furnishingId: stool.id,
            furnishingVersion: 1, position: [-1, 0, 0], yaw: 0, scale: 1)
        let oldDraft = try XCTUnwrap(oldPose.applying(stool, to: draft, supportedExtensionVersion: 3))
        let v2 = oldDraft.candidate["extensions"][namespace]
        XCTAssertEqual(v2["version"], .integer(2))
        for name in ["partition", "info-stand", "barrier", "display-case"] {
            var wire = try registered()
            wire["declaration"]["kind"] = .string(name)
            wire["furnishing"]["kind"] = .string(name)
            wire["furnishing"]["id"] = .string("10000000-0000-4000-8000-000000000005")
            let response = try intake(wire)
            XCTAssertThrowsError(try response.check(supportedExtensionVersion: 2))
            try response.check(supportedExtensionVersion: 3)
            let request = WorkspaceFurnishingRequest(requestId: requestId, details: response.declaration)
            var expected = wire["declaration"]
            expected["requestId"] = .string(requestId)
            XCTAssertEqual(try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(request)), expected)
            let furnishing = try XCTUnwrap(response.furnishing)
            let pose = WorkspaceFurnishingPlacement(id: "10000000-0000-4000-8000-000000000006",
                roomId: room, furnishingId: furnishing.id, furnishingVersion: 1, position: [1, 0, 0], yaw: 0, scale: 1)
            XCTAssertThrowsError(try pose.applying(furnishing, to: oldDraft, supportedExtensionVersion: 2))
            let upgraded = try XCTUnwrap(pose.applying(furnishing, to: oldDraft, supportedExtensionVersion: 3))
            let v3 = upgraded.candidate["extensions"][namespace]
            XCTAssertEqual(v3["version"], .integer(3))
            XCTAssertEqual(v3["furnishings"].items, [stoolWire["furnishing"], wire["furnishing"]])
            XCTAssertEqual(v3["placements"].items?.first, v2["placements"].items?.first)
            XCTAssertNil(try pose.applying(furnishing, to: upgraded, supportedExtensionVersion: 3))
            XCTAssertThrowsError(try oldPose.applying(stool, to: upgraded, supportedExtensionVersion: 2)) {
                XCTAssertEqual(($0 as? AuthoringFailure)?.code, "WORKSPACE_FURNISHING_UNAVAILABLE")
            }
            let another = WorkspaceFurnishingPlacement(id: "10000000-0000-4000-8000-000000000007",
                roomId: room, furnishingId: stool.id, furnishingVersion: 1, position: [0, 0, 1], yaw: 0, scale: 1)
            let kept = try XCTUnwrap(another.applying(stool, to: upgraded, supportedExtensionVersion: 3))
            XCTAssertEqual(kept.candidate["extensions"][namespace]["version"], .integer(3))
        }
    }
}
