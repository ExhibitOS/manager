// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class ArtworkCatalogTests: XCTestCase {
    private func artwork(kind: String, title: String) -> DocumentValue {
        let id = UUID().uuidString.lowercased(), asset = UUID().uuidString.lowercased()
        return .object(["id": .string(id), "kind": .string("artwork"), "artworkType": .string(kind), "revisionId": .string(UUID().uuidString.lowercased()), "primaryAssetId": .string(asset),
            "assets": .array([.object(["id": .string(asset), "mime": .string(kind == "image" || kind == "painting" ? "image/png" : "model/gltf-binary")])]),
            "metadata": .object(["title": .string(title), "artist": .string("합성 작가")]), "dimensions": .object(["width": .number(0.934), "height": .number(0.732)]),
            "extensions": .object(["org.exhibitos.studio/cms": .object(["artworkId": .string(id), "revision": .integer(1)])])])
    }
    private func frame(_ draft: NativeExhibitionDraft, disabled: Bool = false, selection: ViewportSelection? = nil) -> ViewportFrame {
        ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 7, tenantId: UUID().uuidString.lowercased(), principalId: UUID().uuidString.lowercased(), draft: draft, selection: selection, mode: "translate", view: "front", disabled: disabled, cameraSequence: 3)
    }
    func testSourceFiltersAndMultipleInstanceSelectionDoNotMutateDraftOrTrueDimensions() throws {
        var draft = try NativeExhibitionDraft.new()
        let painting = artwork(kind: "image", title: "밀밭"), sculpture = artwork(kind: "sculpture", title: "조각")
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        try draft.addApprovedArtwork(sculpture, roomId: room, position: [0,1,0])
        try draft.addApprovedArtwork(sculpture, roomId: room, position: [2,1,0])
        let before = draft, id = try XCTUnwrap(sculpture["id"].string)
        let ids = draft.catalogPlacementIDs(for: id)
        XCTAssertEqual(ids.count, 2)
        XCTAssertEqual(draft.catalogPlacement(for: id, preferring: ids.last), ids.last)
        XCTAssertEqual(draft.catalogSourceID(for: ids[0]), id)
        XCTAssertNil(draft.catalogSourceID(for: id), "A source ID must never masquerade as a placement instance")
        XCTAssertEqual(draft.catalogPlacementIDs(for: painting["id"].string!), [])
        let image = ArtworkCatalogEntry(id: painting["id"].string!, revision: 1, artwork: painting)
        XCTAssertEqual(image.typeLabel, "회화", "Approved CMS PNG snapshots use artworkType image")
        XCTAssertEqual(image.primaryMime, "image/png")
        let legacyPainting = ArtworkCatalogEntry(id: UUID().uuidString, revision: 1, artwork: artwork(kind: "painting", title: "기존 회화"))
        XCTAssertTrue(legacyPainting.matches(query: "", filter: .painting, in: draft))
        let object = ArtworkCatalogEntry(id: id, revision: 1, artwork: sculpture)
        XCTAssertTrue(image.matches(query: "합성", filter: .unplaced, in: draft))
        XCTAssertTrue(image.matches(query: "밀밭", filter: .painting, in: draft))
        XCTAssertFalse(image.matches(query: "", filter: .sculpture, in: draft))
        XCTAssertFalse(object.matches(query: "", filter: .unplaced, in: draft))
        XCTAssertTrue(object.matches(query: "조각", filter: .sculpture, in: draft))
        XCTAssertEqual(image.dimensions, [0.934,0.732], "Absent depth must stay absent")
        XCTAssertEqual(draft, before)
    }
    func testLegacyCapabilitiesAndDisabledOrForeignAuxiliaryRequestsStayClosed() throws {
        let legacy = try JSONDecoder().decode(ViewportAcceptance.self, from: Data(#"{"accepted":true,"presentations":["3d","plan"]}"#.utf8))
        XCTAssertTrue(legacy.supportsPlan); XCTAssertFalse(legacy.supportsArtworkThumbnail); XCTAssertFalse(legacy.supportsFindSelection)
        let modern = try JSONDecoder().decode(ViewportAcceptance.self, from: Data(#"{"accepted":true,"capabilities":["artwork-thumbnail-v1","find-selection-v1"]}"#.utf8))
        XCTAssertTrue(modern.supportsArtworkThumbnail && modern.supportsFindSelection)
        let draft = try NativeExhibitionDraft.new(), current = frame(draft)
        XCTAssertThrowsError(try ViewportThumbnailRequest(frame: current, artworkId: "../foreign", expectedRevision: 1))
        XCTAssertThrowsError(try ViewportThumbnailRequest(frame: current, artworkId: UUID().uuidString, expectedRevision: 0))
        XCTAssertThrowsError(try ViewportThumbnailRequest(frame: frame(draft, disabled: true), artworkId: UUID().uuidString, expectedRevision: 1))
        XCTAssertThrowsError(try ViewportFindSelectionRequest(frame: current))
        XCTAssertThrowsError(try ViewportFindSelectionRequest(frame: frame(draft, selection: ViewportSelection(kind: "placement", id: UUID().uuidString))))
        let fields = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(current)) as? [String: Any])
        XCTAssertNil(fields["capabilities"]); XCTAssertNil(fields["presentation"])
    }
    func testThumbnailRejectsStaleAccountFrameRevisionAndUnsafePNGBounds() throws {
        let request = try ViewportThumbnailRequest(frame: frame(NativeExhibitionDraft.new()), artworkId: UUID().uuidString, expectedRevision: 4)
        let data = try JSONEncoder().encode(request)
        var fields = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
        fields.removeValue(forKey: "expectedRevision"); fields["revision"] = 4; fields["width"] = 1; fields["height"] = 1
        fields["pngBase64"] = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII="
        func reply(_ value: [String: Any]) throws -> ViewportThumbnailReply { try JSONDecoder().decode(ViewportThumbnailReply.self, from: JSONSerialization.data(withJSONObject: value)) }
        XCTAssertFalse(try reply(fields).pngData(matching: request).isEmpty)
        for key in ["epoch","tenantId","principalId","requestId","artworkId"] {
            var stale = fields; stale[key] = UUID().uuidString.lowercased()
            XCTAssertThrowsError(try reply(stale).pngData(matching: request), key)
        }
        for (key,value) in [("sequence",8),("revision",5),("width",257),("width",2),("height",0)] {
            var invalid = fields; invalid[key] = value
            XCTAssertThrowsError(try reply(invalid).pngData(matching: request), key)
        }
        var oversized = fields; oversized["pngBase64"] = String(repeating: "A", count: 699_053)
        XCTAssertThrowsError(try reply(oversized).pngData(matching: request))
        var wrongFormat = fields; wrongFormat["pngBase64"] = Data("<html>not an image</html>".utf8).base64EncodedString()
        XCTAssertThrowsError(try reply(wrongFormat).pngData(matching: request))
    }
    func testFindReplyBindsExactCurrentInstanceAndDoesNotBecomeDocumentEdit() throws {
        var draft = try NativeExhibitionDraft.new(); let art = artwork(kind: "sculpture", title: "같은 원본")
        let room = draft.rooms[0]["id"].string!
        try draft.addApprovedArtwork(art, roomId: room, position: [0,1,0]); try draft.addApprovedArtwork(art, roomId: room, position: [2,1,0])
        let current = frame(draft, selection: ViewportSelection(kind: "placement", id: draft.placements[1]["id"].string!))
        let before = current.draft, request = try ViewportFindSelectionRequest(frame: current)
        var fields = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(request)) as? [String: Any]); fields["accepted"] = true
        let reply = try JSONDecoder().decode(ViewportFindSelectionReply.self, from: JSONSerialization.data(withJSONObject: fields)); XCTAssertTrue(reply.matches(request))
        fields["selection"] = ["kind":"placement","id":draft.placements[0]["id"].string!]
        XCTAssertFalse(try JSONDecoder().decode(ViewportFindSelectionReply.self, from: JSONSerialization.data(withJSONObject: fields)).matches(request))
        XCTAssertEqual(current.draft, before)
        XCTAssertEqual(current.cameraSequence, 3)
    }
}
