// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class ExhibitionDocumentTests: XCTestCase {
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
}
