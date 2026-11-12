// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class WorkspaceBenchVisualReplacementTests: XCTestCase {
    let oldId = "10000000-0000-4000-8000-000000000001"
    let newId = "10000000-0000-4000-8000-000000000002"
    let placementId = "10000000-0000-4000-8000-000000000003"
    let benchId = "10000000-0000-4000-8000-000000000004"
    let cameraId = "10000000-0000-4000-8000-000000000005"
    let ns = WorkspaceFurnishingPlacement.namespace
    func value<T: Encodable>(_ item: T) throws -> DocumentValue {
        try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(item))
    }
    func descriptor(_ id: String, kind: String = "bench") throws -> WorkspaceFurnishing {
        let wire: DocumentValue = .object(["id": .string(id), "version": .integer(1), "kind": .string(kind),
            "name": .string("Synthetic bench"), "provenance": .string("Synthetic fixture"),
            "rights": .object(["holder": .string("Fixture"), "ownership": .string("owner"), "licenseId": .string("CC0-1.0"),
                "creditLine": .string(""), "permissions": .object(["display": .bool(true), "download": .bool(false), "export": .bool(false), "commercial": .bool(false)])]),
            "model": .object(["mime": .string("model/gltf-binary"), "bytes": .integer(28),
                "sha256": .string(String(repeating: id == oldId ? "a" : "b", count: 64))])])
        return try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(wire))
    }
    func command() -> DocumentValue {
        .object(["benchId": .string(benchId), "placementId": .string(placementId),
            "expectedFurnishingId": .string(oldId), "expectedFurnishingVersion": .integer(1),
            "expectedModelSha256": .string(String(repeating: "a", count: 64)),
            "furnishingId": .string(newId), "furnishingVersion": .integer(1),
            "modelSha256": .string(String(repeating: "b", count: 64))])
    }
    func fixture(shared: Bool = false) throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let host = try BenchSettings(id: benchId, name: "Keep seat", roomId: room, kind: .backed,
            position: [-1, 0, 0], yaw: 0.25, width: 1.5, depth: 0.7, height: 0.45,
            material: .defaults(.wood), viewpointId: cameraId)
        let camera = try BenchViewpoint(id: cameraId, name: "Keep camera", roomId: room,
            position: [-1, 1.2, 0], target: [-1, 1.4, -2], fov: 50)
        try draft.replaceCandidate(BenchCommand.add(host, viewpoint: camera).proposedCandidate(in: draft, selectedBenchId: benchId))
        let placement = WorkspaceFurnishingPlacement(id: placementId, roomId: room, furnishingId: oldId,
            furnishingVersion: 1, position: host.position, yaw: host.yaw, scale: 1)
        draft = try XCTUnwrap(placement.applying(descriptor(oldId), to: draft, supportedExtensionVersion: 7))
        let attachment = try WorkspaceBenchSeatAttachment(benchId: benchId, placementId: placementId,
            furnishingId: oldId, furnishingVersion: 1, modelSha256: String(repeating: "a", count: 64),
            width: host.width, depth: host.depth, height: host.height, viewpointId: cameraId, viewpoint: nil)
        draft = try XCTUnwrap(attachment.applying(descriptor(oldId), to: draft))
        if shared {
            let other = WorkspaceFurnishingPlacement(id: "10000000-0000-4000-8000-000000000006", roomId: room,
                furnishingId: oldId, furnishingVersion: 1, position: [1, 0, 0], yaw: 0, scale: 1)
            draft = try XCTUnwrap(other.applying(descriptor(oldId), to: draft, supportedExtensionVersion: 7))
        }
        var candidate = draft.candidate
        candidate["extensions"]["example/opaque"] = .object(["retain": .string("exact")])
        candidate["extensions"]["org.exhibitos.studio/furnishing-walking-collision"] = .object([
            "version": .integer(1), "overrides": .array([.object([
                "kind": .string("workspace"), "id": .string(placementId), "enabled": .bool(false)])])])
        try draft.replaceCandidate(candidate)
        return draft
    }
    func testAtomicReplacementPreservesEntireSeatCameraAndUnrelatedCandidate() throws {
        let before = try fixture(), target = try descriptor(newId)
        XCTAssertEqual(before.candidate["extensions"][ns]["version"], .integer(1))
        let next = try WorkspaceBenchVisualReplacement(value: command()).applying(target, to: before, supportedExtensionVersion: 7)
        var expected = before.candidate
        var placements = try XCTUnwrap(expected["extensions"][ns]["placements"].items)
        placements[0]["furnishingId"] = .string(newId)
        expected["extensions"][ns]["placements"] = .array(placements)
        expected["extensions"][ns]["furnishings"] = .array([try value(target)])
        var hosts = try XCTUnwrap(expected["extensions"][BenchSettings.namespace]["benches"].items)
        hosts[0]["workspaceVisual"]["modelSha256"] = .string(String(repeating: "b", count: 64))
        expected["extensions"][BenchSettings.namespace]["benches"] = .array(hosts)
        XCTAssertEqual(next.candidate, expected)
        XCTAssertEqual(next.document["editVersion"].integer, try XCTUnwrap(before.document["editVersion"].integer) + 1)
        XCTAssertEqual(try BenchViewpoint.list(in: next), try BenchViewpoint.list(in: before))
    }
    func testSupportedLegacyWorkspaceVersionsArePreserved() throws {
        let input = try WorkspaceBenchVisualReplacement(value: command()), target = try descriptor(newId)
        for version in [Int64(1), 2, 3] {
            var before = try fixture(), candidate = before.candidate
            candidate["extensions"][ns]["version"] = .integer(version)
            try before.replaceCandidate(candidate)
            let next = try input.applying(target, to: before, supportedExtensionVersion: 7)
            XCTAssertEqual(next.candidate["extensions"][ns]["version"], .integer(version))
        }
        for version in [Int64(0), 8] {
            var before = try fixture(), candidate = before.candidate
            candidate["extensions"][ns]["version"] = .integer(version)
            try before.replaceCandidate(candidate)
            XCTAssertThrowsError(try input.applying(target, to: before, supportedExtensionVersion: 7))
        }
        var before = try fixture(), candidate = before.candidate
        candidate["extensions"][ns]["version"] = .integer(2)
        try before.replaceCandidate(candidate)
        XCTAssertThrowsError(try input.applying(target, to: before, supportedExtensionVersion: 1))
    }
    func testSharedOldDescriptorRetainsOrderAndOnlySelectedPlacementChanges() throws {
        let before = try fixture(shared: true), target = try descriptor(newId)
        let next = try WorkspaceBenchVisualReplacement(value: command()).applying(target, to: before, supportedExtensionVersion: 7)
        XCTAssertEqual(next.candidate["extensions"][ns]["furnishings"], .array([try value(descriptor(oldId)), try value(target)]))
        XCTAssertEqual(next.candidate["extensions"][ns]["placements"].items?[1], before.candidate["extensions"][ns]["placements"].items?[1])
    }
    func testExistingTargetDescriptorIsReusedWithoutDuplication() throws {
        let initial = try fixture(), target = try descriptor(newId)
        let other = WorkspaceFurnishingPlacement(id: "10000000-0000-4000-8000-000000000006",
            roomId: try XCTUnwrap(initial.rooms.first?["id"].string), furnishingId: newId,
            furnishingVersion: 1, position: [1, 0, 0], yaw: 0, scale: 1)
        let before = try XCTUnwrap(other.applying(target, to: initial, supportedExtensionVersion: 7))
        let next = try WorkspaceBenchVisualReplacement(value: command()).applying(target, to: before, supportedExtensionVersion: 7)
        XCTAssertEqual(next.candidate["extensions"][ns]["furnishings"], .array([try value(target)]))
        XCTAssertEqual(next.candidate["extensions"][ns]["placements"].items?[1], before.candidate["extensions"][ns]["placements"].items?[1])
    }
    func testStaleBindingAndCallerMutationFieldsAreRejected() throws {
        let before = try fixture(), target = try descriptor(newId)
        for key in ["expectedFurnishingId", "expectedModelSha256", "placementId", "benchId", "modelSha256"] {
            var bad = command(); bad[key] = .string(key.contains("Sha256") ? String(repeating: "c", count: 64) : UUID().uuidString.lowercased())
            XCTAssertThrowsError(try WorkspaceBenchVisualReplacement(value: bad).applying(target, to: before, supportedExtensionVersion: 7))
        }
        for key in ["position", "viewpoint", "collision", "draft", "scale"] {
            var bad = command(); bad[key] = .null
            XCTAssertThrowsError(try WorkspaceBenchVisualReplacement(value: bad))
        }
        for key in ["expectedFurnishingVersion", "furnishingVersion"] {
            var bad = command(); bad[key] = .integer(2)
            XCTAssertThrowsError(try WorkspaceBenchVisualReplacement(value: bad))
            bad[key] = .null
            XCTAssertThrowsError(try WorkspaceBenchVisualReplacement(value: bad))
        }
        XCTAssertThrowsError(try WorkspaceBenchVisualReplacement(value: command()).applying(descriptor(newId, kind: "info-stand"), to: before, supportedExtensionVersion: 7))
    }
    func testChangedPlacementPoseScaleAndMissingHostVisualAreRejected() throws {
        let before = try fixture(), target = try descriptor(newId), input = try WorkspaceBenchVisualReplacement(value: command())
        for key in ["position", "scale", "roomId"] {
            var candidate = before.candidate
            var rows = try XCTUnwrap(candidate["extensions"][ns]["placements"].items)
            rows[0][key] = key == "position" ? .vector([0, 0, 0]) : key == "scale" ? .number(0.9) : .string(UUID().uuidString.lowercased())
            candidate["extensions"][ns]["placements"] = .array(rows)
            var changed = before; try changed.replaceCandidate(candidate)
            XCTAssertThrowsError(try input.applying(target, to: changed, supportedExtensionVersion: 7))
        }
        var candidate = before.candidate
        var rows = try XCTUnwrap(candidate["extensions"][BenchSettings.namespace]["benches"].items)
        if case .object(var host) = rows[0] { host.removeValue(forKey: "workspaceVisual"); rows[0] = .object(host) }
        candidate["extensions"][BenchSettings.namespace]["benches"] = .array(rows)
        var changed = before; try changed.replaceCandidate(candidate)
        XCTAssertThrowsError(try input.applying(target, to: changed, supportedExtensionVersion: 7))
    }
    func testCheckNeverReplaysCachedAuthority() throws {
        let request = Data("request".utf8)
        XCTAssertNil(try NativeAutomationSession.replayResponse(for: "workspace-bench-visual-check", prior: (request, .null), matching: request))
    }
    func testReplacementRequiresItsOwnLiveMarkerAndRoomVisualAdmission() throws {
        var fields = ["userId": oldId, "tenantId": newId, "role": "admin", "csrfToken": "synthetic",
            "expiresAt": "2099-01-01T00:00:00Z", "workspaceFurnishingProfile": "workspace-furnishing-intake-v1",
            "workspaceFurnishingStoolProfile": "workspace-furnishing-stool-v1",
            "workspaceFurnishingRoomVisualProfile": "workspace-furnishing-room-visual-v1"]
        func session() throws -> AuthoringSession {
            try JSONDecoder().decode(AuthoringSession.self, from: JSONSerialization.data(withJSONObject: fields))
        }
        XCTAssertFalse(try session().advertisesWorkspaceBenchVisualReplacement)
        fields["workspaceBenchVisualReplacementProfile"] = WorkspaceBenchVisualReplacement.profile
        XCTAssertTrue(try session().advertisesWorkspaceBenchVisualReplacement)
        fields["workspaceBenchVisualReplacementProfile"] = "wrong-profile"
        XCTAssertFalse(try session().advertisesWorkspaceBenchVisualReplacement)
        fields["workspaceBenchVisualReplacementProfile"] = WorkspaceBenchVisualReplacement.profile
        fields["workspaceFurnishingRoomVisualProfile"] = "unknown"
        XCTAssertFalse(try session().advertisesWorkspaceBenchVisualReplacement)
    }
}
