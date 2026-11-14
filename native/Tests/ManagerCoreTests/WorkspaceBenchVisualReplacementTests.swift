// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class WorkspaceBenchVisualReplacementTests: XCTestCase {
    private let bench = "11111111-1111-4111-8111-111111111111"
    private let placement = "22222222-2222-4222-8222-222222222222"
    private let oldId = "33333333-3333-4333-8333-333333333333"
    private let newId = "44444444-4444-4444-8444-444444444444"
    private let oldHash = String(repeating: "a", count: 64), newHash = String(repeating: "b", count: 64)
    private func descriptor(_ id: String, hash: String, kind: String = "bench") -> DocumentValue {
        .object(["id": .string(id), "version": .integer(1), "kind": .string(kind), "name": .string("Synthetic bench"),
            "provenance": .string("Synthetic test fixture"), "model": .object(["mime": .string("model/gltf-binary"), "bytes": .integer(44), "sha256": .string(hash)]),
            "rights": .object(["holder": .string("Fixture"), "ownership": .string("owner"), "licenseId": .string("CC0-1.0"),
                "creditLine": .string("Fixture"), "permissions": .object(["display": .bool(true), "export": .bool(true), "download": .bool(false), "commercial": .bool(false)])])])
    }
    private func replacement(expected: String? = nil) throws -> WorkspaceBenchVisualReplacement {
        try .init(benchId: bench, placementId: placement, expectedModelSHA256: expected ?? oldHash,
            furnishingId: newId, modelSHA256: newHash)
    }
    private func fixture(shared: Bool = false) throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let seat = try BenchSettings(id: bench, name: "Backed seat", roomId: room, kind: .backed,
            position: [0,0,2], yaw: 0, width: 1.5, depth: 0.7, height: 0.45, material: .defaults(.wood),
            viewpointId: "55555555-5555-4555-8555-555555555555",
            workspaceVisual: .init(placementId: placement, modelSha256: oldHash, modelForward: .positiveZ))
        var rows: [DocumentValue] = [.object(["id": .string(placement), "roomId": .string(room),
            "furnishingId": .string(oldId), "furnishingVersion": .integer(1), "position": .vector([0,0,2]), "yaw": .number(0), "scale": .number(1)])]
        if shared { var other = rows[0]; other["id"] = .string("66666666-6666-4666-8666-666666666666"); rows.append(other) }
        candidate["extensions"] = .object([
            BenchSettings.namespace: .object(["version": .integer(1), "benches": .array([seat.value])]),
            WorkspaceFurnishingPlacement.namespace: .object(["version": .integer(1), "furnishings": .array([descriptor(oldId, hash: oldHash)]), "placements": .array(rows)]),
            "fixture/collision-and-viewpoint": .object(["unchanged": .bool(true)])])
        try draft.replaceCandidate(candidate); return draft
    }
    private func newFurnishing(kind: String = "bench") throws -> WorkspaceFurnishing {
        try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(descriptor(newId, hash: newHash, kind: kind)))
    }
    func testReplacementPreservesHostAndPoseWhileChangingOnlyLinkedVisual() throws {
        let before = try fixture(), next = try replacement().applying(newFurnishing(), to: before, supportedExtensionVersion: 11)
        var expected = before.candidate
        let workspace = WorkspaceFurnishingPlacement.namespace
        expected["extensions"][workspace]["furnishings"] = .array([descriptor(newId, hash: newHash)])
        var rows = try XCTUnwrap(expected["extensions"][workspace]["placements"].items)
        rows[0]["furnishingId"] = .string(newId); expected["extensions"][workspace]["placements"] = .array(rows)
        var seats = try XCTUnwrap(expected["extensions"][BenchSettings.namespace]["benches"].items)
        seats[0]["workspaceVisual"]["modelSha256"] = .string(newHash)
        expected["extensions"][BenchSettings.namespace]["benches"] = .array(seats)
        XCTAssertEqual(next.candidate, expected)
        XCTAssertEqual(try BenchSettings.list(in: next)[0].workspaceVisual?.modelForward, .positiveZ)
    }
    func testSharedOldDescriptorIsRetainedAndOtherPlacementExact() throws {
        let before = try fixture(shared: true), next = try replacement().applying(newFurnishing(), to: before, supportedExtensionVersion: 11)
        let workspace = WorkspaceFurnishingPlacement.namespace
        XCTAssertEqual(next.candidate["extensions"][workspace]["furnishings"].items, [descriptor(oldId, hash: oldHash), descriptor(newId, hash: newHash)])
        XCTAssertEqual(next.candidate["extensions"][workspace]["placements"].items?[1], before.candidate["extensions"][workspace]["placements"].items?[1])
    }
    func testWrongSourceKindDuplicateLinkAndPoseMismatchRefuse() throws {
        let before = try fixture()
        XCTAssertThrowsError(try replacement(expected: String(repeating: "c", count: 64)).applying(newFurnishing(), to: before, supportedExtensionVersion: 11))
        XCTAssertThrowsError(try replacement().applying(newFurnishing(kind: "display-case"), to: before, supportedExtensionVersion: 11))
        var candidate = before.candidate, rows = try XCTUnwrap(candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["placements"].items)
        rows[0]["scale"] = .number(1.1); candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["placements"] = .array(rows)
        var changed = before; try changed.replaceCandidate(candidate)
        XCTAssertThrowsError(try replacement().applying(newFurnishing(), to: changed, supportedExtensionVersion: 11))
        rows.append(rows[0]); candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["placements"] = .array(rows)
        try changed.replaceCandidate(candidate)
        XCTAssertThrowsError(try replacement().applying(newFurnishing(), to: changed, supportedExtensionVersion: 11))
    }
}
