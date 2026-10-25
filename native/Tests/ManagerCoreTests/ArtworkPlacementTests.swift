// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class ArtworkPlacementTests: XCTestCase {
    private let tenant = "10000000-0000-4000-8000-000000000001"
    private func fixture(kind: String = "image", existing: Bool = true) throws -> (ArtworkPlacementIntent, String) {
        var draft = try NativeExhibitionDraft.new()
        let room = draft.rooms[0]["id"].string!, art = UUID().uuidString.lowercased(), asset = UUID().uuidString.lowercased(), surface = UUID().uuidString.lowercased()
        let artwork: DocumentValue = .object(["id": .string(art), "revisionId": .string(UUID().uuidString.lowercased()), "kind": .string("artwork"), "artworkType": .string(kind), "primaryAssetId": .string(asset),
            "metadata": .object(["title": .string("합성 원본"), "description": .string("보존할 대체 설명")]), "dimensions": .object(["width": .number(0.934), "height": .number(0.732)]),
            "rights": .object(["permissions": .object(["display": .bool(true)])]),
            "extensions": .object(["org.exhibitos.studio/cms": .object(["tenantId": .string(tenant), "artworkId": .string(art), "revision": .integer(7)]), "example.org/original": .object(["preserve": .bool(true)])])])
        var candidate = draft.candidate
        candidate["surfaces"] = .array([.object(["id": .string(surface), "roomId": .string(room), "type": .string(kind == "sculpture" ? "floor" : "wall"), "dimensions": .object(["width": .number(12), "height": .number(4)]), "transform": NativeExhibitionDraft.identity])])
        candidate["extensions"] = .object(["example.org/exhibition": .string("preserved")])
        try draft.replaceCandidate(candidate)
        if existing { try draft.addApprovedArtwork(artwork, roomId: room, position: [2,1,0]) }
        return (try ArtworkPlacementIntent(artworkId: art, expectedRevision: 7, tenantId: tenant, artwork: artwork, roomId: room, base: draft), surface)
    }
    private func placement(_ intent: ArtworkPlacementIntent) -> DocumentValue {
        var pose = NativeExhibitionDraft.identity; pose["position"] = .vector([0,1,0])
        return .object(["id": .string(intent.placementId), "roomId": .string(intent.roomId), "artworkRevisionId": intent.artwork["revisionId"], "assetId": intent.artwork["primaryAssetId"], "transform": pose])
    }
    func testOneApprovedAdditionPreservesSourcePriorRowsAndUndoRedoIdentity() throws {
        let (intent, surface) = try fixture(), before = intent.base
        let after = try intent.applying(placement(intent), surfaceId: surface, offset: [0,1], to: before)
        XCTAssertEqual(after.placements.count, before.placements.count + 1)
        XCTAssertEqual(after.placements[0], before.placements[0])
        XCTAssertEqual(after.placements.last?["id"].string, intent.placementId)
        XCTAssertEqual(after.artworks, before.artworks)
        XCTAssertEqual(after.artworks[0]["dimensions"], intent.artwork["dimensions"])
        XCTAssertEqual(after.artworks[0]["dimensions"]["depth"], .null)
        XCTAssertEqual(after.candidate["accessibility"]["artworkDescriptions"].items?.last, .object(["placementId": .string(intent.placementId), "text": .string("보존할 대체 설명")]))
        for key in ["rooms", "surfaces", "openings", "lights", "extensions"] { XCTAssertEqual(after.candidate[key], before.candidate[key], key) }
        XCTAssertEqual(after.document["editVersion"].integer, before.document["editVersion"].integer! + 1)
        var history = after
        try history.replaceCandidate(before.candidate); XCTAssertEqual(history.candidate, before.candidate)
        try history.replaceCandidate(after.candidate); XCTAssertEqual(history.candidate, after.candidate)
        XCTAssertEqual(intent.base, before, "Preview/cancel has no document mutation")
    }
    func testStaleScopeForeignTargetsAndForgedPlacementAreRefusedWithoutMutation() throws {
        let (intent, surface) = try fixture(), before = intent.base, valid = placement(intent)
        var stale = before; try stale.rename("다른 현재 입력")
        XCTAssertThrowsError(try intent.applying(valid, surfaceId: surface, offset: [0,1], to: stale))
        XCTAssertThrowsError(try intent.applying(valid, surfaceId: UUID().uuidString, offset: [0,1], to: before))
        XCTAssertThrowsError(try intent.applying(valid, surfaceId: surface, offset: [7,1], to: before))
        XCTAssertThrowsError(try intent.applying(valid, surfaceId: surface, offset: [.nan,1], to: before))
        for key in ["id", "roomId", "artworkRevisionId", "assetId"] {
            var forged = valid; forged[key] = .string(UUID().uuidString.lowercased())
            XCTAssertThrowsError(try intent.applying(forged, surfaceId: surface, offset: [0,1], to: before), key)
        }
        for (key, value) in [("scale", DocumentValue.vector([2,1,1])), ("rotation", .vector([0,0,0,0])), ("position", .vector([100,1,0]))] {
            var forged = valid; forged["transform"][key] = value
            XCTAssertThrowsError(try intent.applying(forged, surfaceId: surface, offset: [0,1], to: before), key)
        }
        var extra = valid; extra["extensions"] = .object(["arbitrary": .bool(true)])
        XCTAssertThrowsError(try intent.applying(extra, surfaceId: surface, offset: [0,1], to: before))
        XCTAssertEqual(intent.base, before)
        var duplicate = before
        try duplicate.addApprovedArtwork(intent.artwork, roomId: intent.roomId, placementId: intent.placementId, transform: valid["transform"])
        let committed = duplicate
        XCTAssertThrowsError(try duplicate.addApprovedArtwork(intent.artwork, roomId: intent.roomId, placementId: intent.placementId, transform: valid["transform"]))
        XCTAssertEqual(duplicate, committed)
    }
    func testIntentBindsTenantApprovedRevisionRightsAndExplicitSurfaceKind() throws {
        let (intent, surface) = try fixture()
        XCTAssertThrowsError(try ArtworkPlacementIntent(artworkId: intent.artworkId, expectedRevision: 8, tenantId: tenant, artwork: intent.artwork, roomId: intent.roomId, base: intent.base))
        XCTAssertThrowsError(try ArtworkPlacementIntent(artworkId: intent.artworkId, expectedRevision: 7, tenantId: UUID().uuidString, artwork: intent.artwork, roomId: intent.roomId, base: intent.base))
        var denied = intent.artwork; denied["rights"]["permissions"]["display"] = .bool(false)
        XCTAssertThrowsError(try ArtworkPlacementIntent(artworkId: intent.artworkId, expectedRevision: 7, tenantId: tenant, artwork: denied, roomId: intent.roomId, base: intent.base))
        var changed = intent.base.candidate, rows = changed["surfaces"].items!
        rows[0]["type"] = .string("floor"); changed["surfaces"] = .array(rows)
        var floor = intent.base; try floor.replaceCandidate(changed)
        let wrongSurface = try ArtworkPlacementIntent(artworkId: intent.artworkId, expectedRevision: 7, tenantId: tenant, artwork: intent.artwork, roomId: intent.roomId, base: floor)
        XCTAssertThrowsError(try wrongSurface.applying(placement(wrongSurface), surfaceId: surface, offset: [0,1], to: floor))
    }
    func testClosedRequestsBindCurrentFrameAndTargetAndAddNewApprovedSourceOnly() throws {
        let (intent, surface) = try fixture(existing: false)
        let frame = ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 7, tenantId: tenant, principalId: UUID().uuidString.lowercased(), draft: intent.base, selection: nil, mode: "translate", view: "front", disabled: false)
        let begin = try ViewportPlacementRequest(frame: frame, intent: intent, phase: .begin)
        XCTAssertEqual(begin.value["expectedRevision"], .integer(7))
        XCTAssertEqual(begin.value["requestId"].string, intent.requestId)
        XCTAssertEqual(begin.value["artworkRevisionId"], .null)
        let target = try ViewportPlacementTarget(value: .object(["placementRequestId": .string(intent.requestId), "targetToken": .string(UUID().uuidString.lowercased()), "surfaceId": .string(surface), "offset": .vector([0,1]), "valid": .bool(true), "reason": .string("")]), intent: intent)
        let request = try ViewportPlacementRequest(frame: frame, intent: intent, phase: .commit, target: target)
        XCTAssertNotEqual(request.value["requestId"], begin.value["requestId"])
        var reply = request.value; reply["accepted"] = .bool(true); reply["validatedPlacement"] = placement(intent)
        XCTAssertEqual(try request.checkedReply(reply), placement(intent))
        let added = try intent.applying(placement(intent), surfaceId: surface, offset: [0,1], to: intent.base)
        XCTAssertEqual(added.artworks, [intent.artwork]); XCTAssertEqual(added.placements.count, 1)
        for key in ["epoch", "tenantId", "principalId", "requestId", "placementRequestId", "targetToken"] {
            var stale = reply; stale[key] = .string(UUID().uuidString.lowercased())
            XCTAssertThrowsError(try request.checkedReply(stale), key)
        }
        var stale = reply; stale["sequence"] = .integer(6)
        XCTAssertThrowsError(try request.checkedReply(stale))
        var arbitrary = reply; arbitrary["candidate"] = intent.base.candidate
        XCTAssertThrowsError(try request.checkedReply(arbitrary))
        let disabled = ViewportFrame(epoch: frame.epoch, sequence: 7, tenantId: tenant, principalId: frame.principalId, draft: intent.base, selection: nil, mode: "translate", view: "front", disabled: true)
        XCTAssertThrowsError(try ViewportPlacementRequest(frame: disabled, intent: intent, phase: .begin))
    }
}
