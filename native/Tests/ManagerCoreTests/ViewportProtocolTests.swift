// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore
final class ViewportProtocolTests: XCTestCase {
    func testGeometryProposalPreservesIdentityRightsAndOpaqueFields() throws {
        var draft = try NativeExhibitionDraft.new()
        let original = draft
        var geometry = draft.candidate
        var rooms = geometry["rooms"].items!; rooms[0]["name"] = .string("바꾼 방")
        geometry["rooms"] = .array(rooms)
        try draft.acceptGeometry(geometry)
        XCTAssertEqual(draft.rooms[0]["name"].string, "바꾼 방")
        XCTAssertEqual(draft.id, original.id)
        XCTAssertEqual(draft.document["editVersion"].integer, 2)
        let accepted = draft
        for key in ["id","artworks","extensions","accessibility","title","mediaAssets","scripts"] {
            var wrong = geometry; wrong[key] = .string("untrusted replacement")
            XCTAssertThrowsError(try draft.acceptGeometry(wrong)); XCTAssertEqual(draft, accepted)
        }
        try draft.replaceCandidate(original.candidate)
        XCTAssertEqual(draft.candidate, original.candidate)
        XCTAssertEqual(draft.document["editVersion"].integer, 3)
    }
    func testFrameRetainsNullSelectionAndRequiresRealSelectedItem() throws {
        let draft = try NativeExhibitionDraft.new(), epoch = UUID().uuidString
        let frame = ViewportFrame(epoch: epoch, sequence: 1, tenantId: UUID().uuidString, principalId: UUID().uuidString, draft: draft, selection: nil, mode: "translate", view: "top", disabled: false)
        let object = try JSONSerialization.jsonObject(with: JSONEncoder().encode(frame)) as! [String: Any]
        XCTAssertTrue(object["selection"] is NSNull)
        XCTAssertEqual(object.count, 12)
        XCTAssertEqual(object["cameraSequence"] as? Int, 0)
        XCTAssertEqual(object["transparent"] as? Bool, true)
        XCTAssertTrue(ViewportSelection(kind: "room", id: draft.rooms[0]["id"].string!).belongs(to: draft))
        XCTAssertFalse(ViewportSelection(kind: "placement", id: draft.rooms[0]["id"].string!).belongs(to: draft))
        XCTAssertFalse(ViewportSelection(kind: "arbitrary", id: UUID().uuidString).belongs(to: draft))
    }
}
