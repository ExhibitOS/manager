// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class WorkspaceInfoStandReplacementTests: XCTestCase {
    let oldId = "10000000-0000-4000-8000-000000000001"
    let newId = "10000000-0000-4000-8000-000000000002"
    let standId = "10000000-0000-4000-8000-000000000003"
    let plaqueId = "10000000-0000-4000-8000-000000000004"
    let artId = "10000000-0000-4000-8000-000000000005"
    let ns = WorkspaceFurnishingPlacement.namespace

    func descriptor(_ id: String, kind: String = "info-stand") throws -> WorkspaceFurnishing {
        let wire: DocumentValue = .object(["id": .string(id), "version": .integer(1), "kind": .string(kind),
            "name": .string("Synthetic stand"), "provenance": .string("Synthetic test fixture"),
            "rights": .object(["holder": .string("Fixture"), "ownership": .string("owner"), "licenseId": .string("CC0-1.0"), "creditLine": .string(""),
                "permissions": .object(["display": .bool(true), "download": .bool(false), "export": .bool(false), "commercial": .bool(false)])]),
            "model": .object(["mime": .string("model/gltf-binary"), "bytes": .integer(28), "sha256": .string(String(repeating: id == oldId ? "a" : "b", count: 64))])])
        return try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(wire))
    }

    func value<T: Encodable>(_ item: T) throws -> DocumentValue {
        try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(item))
    }

    func fixture(shared: Bool = false) throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let artwork: DocumentValue = .object(["kind": .string("artwork"), "revisionId": .string("10000000-0000-4000-8000-000000000006"),
            "primaryAssetId": .string("10000000-0000-4000-8000-000000000007")])
        try draft.addApprovedArtwork(artwork, roomId: room, placementId: artId, transform: NativeExhibitionDraft.identity)
        let stand = WorkspaceFurnishingPlacement(id: standId, roomId: room, furnishingId: oldId, furnishingVersion: 1,
            position: [-1, 0, 0], yaw: 0, scale: 1)
        draft = try XCTUnwrap(stand.applying(descriptor(oldId), to: draft, supportedExtensionVersion: 3))
        if shared {
            let other = WorkspaceFurnishingPlacement(id: "10000000-0000-4000-8000-000000000008", roomId: room,
                furnishingId: oldId, furnishingVersion: 1, position: [1, 0, 0], yaw: 0, scale: 1)
            draft = try XCTUnwrap(other.applying(descriptor(oldId), to: draft, supportedExtensionVersion: 3))
        }
        let plaque = try PlaqueSettings(id: plaqueId, roomId: room, source: .artwork(placementId: artId),
            position: [-1, 1, 0], rotation: [0, 0, 0, 1], width: 0.15, height: 0.1, visible: true,
            captionTranslations: [.init(locale: "en", artist: "Fixture author", date: "2026", medium: "Synthetic", dimensions: "1 × 1 cm")])
        var candidate = draft.candidate
        candidate["extensions"][PlaqueSettings.namespace] = .object(["version": .integer(3), "plaques": .array([plaque.value])])
        candidate["extensions"]["example/preserved"] = .object(["value": .string("unchanged")])
        try draft.replaceCandidate(candidate)
        return draft
    }

    func command(_ draft: NativeExhibitionDraft) -> DocumentValue {
        .object(["placement": .object(["id": .string(standId), "roomId": draft.rooms[0]["id"],
            "furnishingId": .string(newId), "furnishingVersion": .integer(1), "position": .vector([-1, 0, 0]), "yaw": .number(0), "scale": .number(1)]),
            "plaque": .object(["id": .string(plaqueId), "position": .vector([-1, 0.9, 0]), "rotation": .vector([0, 0, 0, 1]),
                "dimensions": .object(["width": .number(0.15), "height": .number(0.1)])])])
    }

    func testSoleUseReplacementPrunesOnlyUnusedSnapshotAndPreservesWholeScene() throws {
        let before = try fixture(), input = command(before), model = try descriptor(newId)
        let next = try XCTUnwrap(WorkspaceInfoStandReplacement(value: input).applying(model, to: before, supportedExtensionVersion: 3))
        var expected = before.candidate
        expected["extensions"][ns]["placements"] = .array([input["placement"]])
        expected["extensions"][ns]["furnishings"] = .array([try value(model)])
        var plaques = try XCTUnwrap(expected["extensions"][PlaqueSettings.namespace]["plaques"].items)
        plaques[0]["position"] = input["plaque"]["position"]
        expected["extensions"][PlaqueSettings.namespace]["plaques"] = .array(plaques)
        XCTAssertEqual(next.candidate, expected)
        XCTAssertEqual(next.document["editVersion"].integer, try XCTUnwrap(before.document["editVersion"].integer) + 1)
        XCTAssertEqual(next.candidate["extensions"][ns]["version"], .integer(3))
        XCTAssertNil(try WorkspaceInfoStandReplacement(value: input).applying(model, to: next, supportedExtensionVersion: 3))
    }

    func testSharedOldDescriptorStaysInOrderAndNewDescriptorAppends() throws {
        let before = try fixture(shared: true), input = command(before), model = try descriptor(newId)
        let next = try XCTUnwrap(WorkspaceInfoStandReplacement(value: input).applying(model, to: before, supportedExtensionVersion: 3))
        XCTAssertEqual(next.candidate["extensions"][ns]["furnishings"], .array([try value(descriptor(oldId)), try value(model)]))
        XCTAssertEqual(next.candidate["extensions"][ns]["placements"].items?[1], before.candidate["extensions"][ns]["placements"].items?[1])
        XCTAssertEqual(next.candidate["extensions"][PlaqueSettings.namespace]["plaques"].items?[0]["source"], before.candidate["extensions"][PlaqueSettings.namespace]["plaques"].items?[0]["source"])
    }

    func testRejectsExtraInputsAndWrongExistingPairOrDescriptor() throws {
        let before = try fixture(), model = try descriptor(newId)
        var bad = command(before); bad["plaque"]["source"] = .object(["kind": .string("room")])
        XCTAssertThrowsError(try WorkspaceInfoStandReplacement(value: bad))
        bad = command(before); bad["placement"]["id"] = .string("10000000-0000-4000-8000-000000000009")
        XCTAssertThrowsError(try WorkspaceInfoStandReplacement(value: bad).applying(model, to: before, supportedExtensionVersion: 3))
        bad = command(before); bad["plaque"]["id"] = .string(artId)
        XCTAssertThrowsError(try WorkspaceInfoStandReplacement(value: bad).applying(model, to: before, supportedExtensionVersion: 3))
        XCTAssertThrowsError(try WorkspaceInfoStandReplacement(value: command(before)).applying(descriptor(newId, kind: "pedestal"), to: before, supportedExtensionVersion: 3))
        XCTAssertThrowsError(try WorkspaceInfoStandReplacement(value: command(before)).applying(model, to: before, supportedExtensionVersion: 2))
    }

    func testInvalidGeometryIsRejectedWithoutNormalizingOrChangingSource() throws {
        let before = try fixture(), original = before.candidate, model = try descriptor(newId)
        var bad = command(before); bad["plaque"]["rotation"] = .vector([0, 0, 0, 2])
        XCTAssertThrowsError(try WorkspaceInfoStandReplacement(value: bad).applying(model, to: before, supportedExtensionVersion: 3))
        bad = command(before); bad["placement"]["position"] = .vector([100, 0, 0])
        XCTAssertThrowsError(try WorkspaceInfoStandReplacement(value: bad).applying(model, to: before, supportedExtensionVersion: 3))
        XCTAssertEqual(before.candidate, original)
    }

    func testLiveProfileRequiresDistinctMarkerAndExistingRoomVisualAdmission() throws {
        let raw: [String: String] = ["userId": oldId, "tenantId": newId, "role": "admin", "csrfToken": "synthetic", "expiresAt": "2099-01-01T00:00:00Z",
            "workspaceFurnishingProfile": "workspace-furnishing-intake-v1", "workspaceFurnishingStoolProfile": "workspace-furnishing-stool-v1",
            "workspaceFurnishingRoomVisualProfile": "workspace-furnishing-room-visual-v1"]
        func session(_ fields: [String: String]) throws -> AuthoringSession { try JSONDecoder().decode(AuthoringSession.self, from: JSONSerialization.data(withJSONObject: fields)) }
        XCTAssertFalse(try session(raw).advertisesWorkspaceInfoStandReplacement)
        var active = raw; active["workspaceInfoStandReplacementProfile"] = WorkspaceInfoStandReplacement.profile
        XCTAssertTrue(try session(active).advertisesWorkspaceInfoStandReplacement)
        active["workspaceFurnishingRoomVisualProfile"] = "unknown"
        XCTAssertFalse(try session(active).advertisesWorkspaceInfoStandReplacement)
    }
}
