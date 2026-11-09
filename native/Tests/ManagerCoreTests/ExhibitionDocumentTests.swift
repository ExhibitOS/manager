// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class ExhibitionDocumentTests: XCTestCase {
    func testComparedSnapshotMustStillMatchBeforeAdoption() throws {
        let draft = try NativeExhibitionDraft.new()
        let reviewed = NativeStudioResponse(revision: 3, etag: "etag-3", draft: draft)
        XCTAssertNoThrow(try reviewed.requireSameComparison(reviewed))
        var changed = draft
        try changed.rename("서버에서 변경된 제목")
        let mismatches = [
            NativeStudioResponse(revision: 4, etag: "etag-3", draft: draft),
            NativeStudioResponse(revision: 3, etag: "etag-4", draft: draft),
            NativeStudioResponse(revision: 3, etag: "etag-3", draft: changed),
            NativeStudioResponse(revision: 3, etag: "etag-3", draft: try NativeExhibitionDraft.new()),
        ]
        for current in mismatches {
            XCTAssertThrowsError(try reviewed.requireSameComparison(current)) { error in
                XCTAssertEqual((error as? AuthoringFailure)?.code, "EXHIBITION_COMPARISON_STALE")
            }
        }
        XCTAssertEqual(reviewed.draft, draft)
    }
    func testNativeEditsPreserveOpaqueExtensionsApprovedArtworkAndPose() throws {
        var draft = try NativeExhibitionDraft.new()
        var document = draft.document
        let opaque: DocumentValue = .object(["version": .integer(1), "nested": .array([.string("작가 원문"), .null, .bool(false), .integer(9_007_199_254_740_991)])])
        document["candidate"]["extensions"] = .object(["org.example/annotation": opaque])
        draft = try NativeExhibitionDraft(document: document)
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let artwork: DocumentValue = .object(["kind": .string("artwork"), "revisionId": .string(UUID().uuidString.lowercased()), "primaryAssetId": .string(UUID().uuidString.lowercased()), "rights": opaque, "transform": .object(["position": .vector([1, 2, 3]), "rotation": .vector([0, 0, 0, 1]), "scale": .vector([2, 1, 1])]), "extensions": opaque])
        try draft.rename("네이티브 전시")
        try draft.updateRoom(room, name: "첫 전시실", width: 10, height: 3, depth: 8)
        try draft.addApprovedArtwork(artwork, roomId: room, position: [0, 1, 0])
        let placement = try XCTUnwrap(draft.placements.first?["id"].string)
        try draft.movePlacement(placement, position: [2, 1, -1])
        let roundTrip = try JSONDecoder().decode(NativeExhibitionDraft.self, from: JSONEncoder().encode(draft))
        XCTAssertEqual(roundTrip, draft)
        XCTAssertEqual(roundTrip.candidate["extensions"]["org.example/annotation"], opaque)
        XCTAssertEqual(roundTrip.artworks, [artwork])
        XCTAssertEqual(roundTrip.placements.first?["transform"]["position"], .vector([2, 1, -1]))
        XCTAssertEqual(roundTrip.candidate["accessibility"]["artworkDescriptions"].items?.first?["placementId"].string, placement)
        XCTAssertFalse(roundTrip.candidate["accessibility"]["artworkDescriptions"].items?.first?["text"].string?.isEmpty ?? true)
        XCTAssertEqual(roundTrip.document["editVersion"].integer, 5)
        XCTAssertEqual(DocumentValue.number(1), .integer(1))
        XCTAssertNotEqual(DocumentValue.integer(9_007_199_254_740_991), .number(9_007_199_254_740_992))
    }
    func testInvalidIdentityGeometryAndRevisionReplacementPreserveDraft() throws {
        var draft = try NativeExhibitionDraft.new()
        let original = draft
        XCTAssertThrowsError(try draft.rename(" "))
        XCTAssertThrowsError(try draft.updateRoom(draft.rooms[0]["id"].string!, name: "Room", width: .infinity, height: 3, depth: 4))
        XCTAssertThrowsError(try draft.movePlacement(UUID().uuidString, position: [0, 0, .nan]))
        XCTAssertEqual(draft, original)
        var document = draft.document; document["candidate"]["id"] = .string(UUID().uuidString)
        XCTAssertThrowsError(try NativeExhibitionDraft(document: document))
        var future = draft.document; future["schemaVersion"] = .string("2.0.0")
        XCTAssertThrowsError(try NativeExhibitionDraft(document: future))
        let room = draft.rooms[0]["id"].string!, revision = UUID().uuidString
        var artwork: DocumentValue = .object(["kind": .string("artwork"), "revisionId": .string(revision), "primaryAssetId": .string(UUID().uuidString)])
        try draft.addApprovedArtwork(artwork, roomId: room, position: [0, 0, 0])
        let saved = draft
        artwork["rights"] = .string("Changed immutable snapshot")
        XCTAssertThrowsError(try draft.addApprovedArtwork(artwork, roomId: room, position: [1, 0, 0]))
        XCTAssertEqual(draft, saved)
    }

    func testPlacementRotationChangesOnlyTheSelectedQuaternion() throws {
        var draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let artwork: DocumentValue = .object([
            "kind": .string("artwork"), "revisionId": .string(UUID().uuidString),
            "primaryAssetId": .string(UUID().uuidString)
        ])
        try draft.addApprovedArtwork(artwork, roomId: room, position: [1, 2, 3])
        try draft.addApprovedArtwork(artwork, roomId: room, position: [2, 3, 4])
        let id = try XCTUnwrap(draft.placements.first?["id"].string)
        let before = draft
        XCTAssertThrowsError(try draft.rotatePlacement(id, rotation: [0, 2, 0, 0]))
        XCTAssertThrowsError(try draft.rotatePlacement(id, rotation: [0, .nan, 0, 0]))
        XCTAssertThrowsError(try draft.rotatePlacement(UUID().uuidString, rotation: [0, 1, 0, 0]))
        XCTAssertEqual(draft, before)
        try draft.rotatePlacement(id, rotation: [0, 1, 0, 0])
        var expected = before.candidate
        var placements = before.placements
        placements[0]["transform"]["rotation"] = .vector([0, 1, 0, 0])
        expected["placements"] = .array(placements)
        XCTAssertEqual(draft.candidate, expected)
        XCTAssertEqual(draft.document["editVersion"].integer, before.document["editVersion"].integer.map { $0 + 1 })
    }

    func testInterpretationRequiresExactUTF8ForExpectedText() throws {
        var draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let artwork: DocumentValue = .object(["kind": .string("artwork"), "revisionId": .string(UUID().uuidString),
                                                "primaryAssetId": .string(UUID().uuidString), "metadata": .object(["description": .string("é")])])
        try draft.addApprovedArtwork(artwork, roomId: room, position: [0, 1, 0])
        let id = try XCTUnwrap(draft.placements.first?["id"].string)
        let before = draft
        XCTAssertThrowsError(try draft.setArtworkInterpretation(id, expectedText: "e\u{301}", text: "새 해설"))
        XCTAssertEqual(draft, before)
        XCTAssertNoThrow(try draft.setArtworkInterpretation(id, expectedText: "é", text: "새 해설"))
    }

    func testInterpretationRequiresReviewedTextAndPreservesOtherContent() throws {
        var draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let artwork: DocumentValue = .object(["kind": .string("artwork"), "revisionId": .string(UUID().uuidString),
                                                "primaryAssetId": .string(UUID().uuidString), "metadata": .object(["description": .string("원래 설명")])])
        try draft.addApprovedArtwork(artwork, roomId: room, position: [0, 1, 0])
        try draft.addApprovedArtwork(artwork, roomId: room, position: [2, 1, 0])
        let id = try XCTUnwrap(draft.placements.first?["id"].string)
        let before = draft
        XCTAssertThrowsError(try draft.setArtworkInterpretation(id, expectedText: "이전 값이 다름", text: "새 해설"))
        XCTAssertThrowsError(try draft.setArtworkInterpretation(id, expectedText: "원래 설명", text: "  \n"))
        XCTAssertEqual(draft, before)
        try draft.setArtworkInterpretation(id, expectedText: "원래 설명", text: "새 해설")
        var expected = before.candidate
        var descriptions = try XCTUnwrap(expected["accessibility"]["artworkDescriptions"].items)
        descriptions[0]["text"] = .string("새 해설")
        expected["accessibility"]["artworkDescriptions"] = .array(descriptions)
        XCTAssertEqual(draft.candidate, expected)
        XCTAssertEqual(draft.document["editVersion"].integer, before.document["editVersion"].integer.map { $0 + 1 })
    }

    func testPositionMovePreservesPlacementRelationshipsAndOtherPoseFields() throws {
        var draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let artwork: DocumentValue = .object(["kind": .string("artwork"), "revisionId": .string(UUID().uuidString),
                                                "primaryAssetId": .string(UUID().uuidString)])
        try draft.addApprovedArtwork(artwork, roomId: room, position: [0, 1, 0])
        let id = try XCTUnwrap(draft.placements.first?["id"].string)
        let before = draft
        try draft.movePlacement(id, position: [2, 1, -1])
        var expected = before.candidate
        var placements = before.placements
        placements[0]["transform"]["position"] = .vector([2, 1, -1])
        expected["placements"] = .array(placements)
        XCTAssertEqual(draft.candidate, expected)
        XCTAssertEqual(draft.document["editVersion"].integer, before.document["editVersion"].integer.map { $0 + 1 })
    }

    func testApprovedCompactRefreshKeepsPlacementAndFullModel() throws {
        var draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let full = UUID().uuidString.lowercased(), coarse = UUID().uuidString.lowercased()
        let lodKey = "org.exhibitos.viewer/lod"
        let original: DocumentValue = .object([
            "id": .string(UUID().uuidString.lowercased()), "kind": .string("artwork"),
            "artworkType": .string("sculpture"), "revision": .integer(1),
            "revisionId": .string(UUID().uuidString.lowercased()), "primaryAssetId": .string(full),
            "assets": .array([.object(["id": .string(full), "sha256": .string(String(repeating: "a", count: 64))])]),
            "transform": NativeExhibitionDraft.identity,
            "extensions": .object(["org.exhibitos.studio/cms": .object(["revision": .integer(1)])])
        ])
        try draft.addApprovedArtwork(original, roomId: room, position: [1, 2, 3])
        let before = draft
        let id = try XCTUnwrap(draft.placements.first?["id"].string)
        var approved = original
        approved["revision"] = .integer(2)
        approved["revisionId"] = .string(UUID().uuidString.lowercased())
        approved["extensions"]["org.exhibitos.studio/cms"]["revision"] = .integer(2)
        approved["assets"] = .array(try XCTUnwrap(original["assets"].items) + [
            .object(["id": .string(coarse), "mime": .string("model/gltf-binary"), "role": .string("model")])
        ])
        approved["extensions"][lodKey] = .object([
            "version": .integer(1), "profile": .string(NativeCompactModelVariantRequest.profile),
            "variants": .array([
                .object(["detail": .string("full"), "assetId": .string(full)]),
                .object(["detail": .string("coarse"), "assetId": .string(coarse)])
            ])
        ])
        var invalid = approved
        invalid["primaryAssetId"] = .string(coarse)
        XCTAssertThrowsError(try draft.refreshApprovedCompactArtwork(invalid, placementId: id))
        invalid = approved
        invalid["transform"]["scale"] = .vector([2, 2, 2])
        XCTAssertThrowsError(try draft.refreshApprovedCompactArtwork(invalid, placementId: id))
        XCTAssertEqual(draft, before)

        try draft.refreshApprovedCompactArtwork(approved, placementId: id)
        var expected = before.candidate
        var placement = before.placements[0]
        placement["artworkRevisionId"] = approved["revisionId"]
        expected["placements"] = .array([placement])
        expected["artworks"] = .array([approved])
        XCTAssertEqual(draft.candidate, expected)
    }

    func testHayesTextRefreshPreservesAssetPoseAndUnrelatedPlacements() throws {
        var draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let asset = UUID().uuidString
        var prior: DocumentValue = .object([
            "id": .string(UUID().uuidString), "kind": .string("artwork"),
            "schemaVersion": .string("1.0.0-draft.2"), "units": .string("unknown"),
            "artworkType": .string("sculpture"), "revision": .integer(1),
            "revisionId": .string(UUID().uuidString), "primaryAssetId": .string(asset),
            "createdAt": .string("2026-01-01T00:00:00Z"),
            "metadata": .object(["description": .string("Original description")]),
            "rights": .string("retained rights"), "provenance": .object(["authorship": .string("human-authored")]),
            "extensions": .object([
                "org.exhibitos.studio/cms": .object(["revision": .integer(1)]),
                "org.exhibitos.studio/display-reference": .object(["referenceHeightMeters": .number(0.273)])
            ])
        ])
        prior["transform"] = NativeExhibitionDraft.identity
        try draft.addApprovedArtwork(prior, roomId: room, position: [1, 2, 3])
        let placement = try XCTUnwrap(draft.placements.first?["id"].string)
        var other = prior
        other["id"] = .string(UUID().uuidString)
        other["revisionId"] = .string(UUID().uuidString)
        other["primaryAssetId"] = .string(UUID().uuidString)
        try draft.addApprovedArtwork(other, roomId: room, position: [2, 3, 4])
        let before = draft
        var approved = prior
        approved["revision"] = .integer(2)
        approved["revisionId"] = .string(UUID().uuidString)
        approved["createdAt"] = .string("2026-01-02T00:00:00Z")
        approved["extensions"]["org.exhibitos.studio/cms"]["revision"] = .integer(2)
        approved["metadata"]["description"] = .string("Attributed catalogue dimensions")
        var changed = approved
        changed["rights"] = .string("different rights")
        XCTAssertThrowsError(try draft.refreshHayesArtwork(changed, placementId: placement))
        changed = approved
        changed["extensions"]["org.exhibitos.studio/display-reference"]["referenceHeightMeters"] = .number(1)
        XCTAssertThrowsError(try draft.refreshHayesArtwork(changed, placementId: placement))
        XCTAssertThrowsError(try draft.refreshHayesArtwork(approved, placementId: UUID().uuidString))
        XCTAssertEqual(draft, before)

        var withAuthoredDescription = draft.document
        var authoredDescriptions = try XCTUnwrap(draft.candidate["accessibility"]["artworkDescriptions"].items)
        authoredDescriptions[0]["text"] = .string("A separate description written for this placement")
        withAuthoredDescription["candidate"]["accessibility"]["artworkDescriptions"] = .array(authoredDescriptions)
        var authored = try NativeExhibitionDraft(document: withAuthoredDescription)
        XCTAssertThrowsError(try authored.refreshHayesArtwork(approved, placementId: placement))
        XCTAssertEqual(authored.document, withAuthoredDescription)

        try draft.refreshHayesArtwork(approved, placementId: placement)
        XCTAssertEqual(draft.artworks, [approved, other])
        var updatedPlacement = before.placements[0]
        updatedPlacement["artworkRevisionId"] = approved["revisionId"]
        XCTAssertEqual(draft.placements, [updatedPlacement, before.placements[1]])
        let descriptions = try XCTUnwrap(draft.candidate["accessibility"]["artworkDescriptions"].items)
        XCTAssertEqual(descriptions[0]["text"], approved["metadata"]["description"])
        XCTAssertEqual(descriptions[1], before.candidate["accessibility"]["artworkDescriptions"].items?[1])
        let refreshed = draft
        XCTAssertThrowsError(try draft.refreshHayesArtwork(approved, placementId: placement))
        XCTAssertEqual(draft, refreshed)
    }
}
