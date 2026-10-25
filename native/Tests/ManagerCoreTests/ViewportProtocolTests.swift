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
    func testPlanPresentationAddsOnlyItsBoundedOptionalField() throws {
        let draft = try NativeExhibitionDraft.new()
        let selection = ViewportSelection(kind: "room", id: draft.rooms[0]["id"].string!)
        let epoch = UUID().uuidString, tenant = UUID().uuidString, principal = UUID().uuidString
        func encoded(_ presentation: ViewportPresentation) throws -> [String: Any] {
            let frame = ViewportFrame(epoch: epoch, sequence: 7, tenantId: tenant, principalId: principal,
                draft: draft, selection: selection, mode: "dimensions", view: "front", disabled: true,
                cameraSequence: 9, transparent: false, presentation: presentation)
            return try JSONSerialization.jsonObject(with: JSONEncoder().encode(frame)) as! [String: Any]
        }
        let scene = try encoded(.threeD)
        var plan = try encoded(.plan)
        XCTAssertEqual(scene.count, 12)
        XCTAssertNil(scene["presentation"])
        XCTAssertEqual(plan.removeValue(forKey: "presentation") as? String, "plan")
        XCTAssertEqual(try JSONSerialization.data(withJSONObject: scene, options: .sortedKeys),
                       try JSONSerialization.data(withJSONObject: plan, options: .sortedKeys))
    }
    func testPlanCapabilityRequiresExplicitAcceptedSupportAndKeepsLegacyCompatible() throws {
        func response(_ text: String) throws -> ViewportAcceptance {
            try JSONDecoder().decode(ViewportAcceptance.self, from: Data(text.utf8))
        }
        XCTAssertFalse(try response(#"{"accepted":true}"#).supportsPlan)
        XCTAssertFalse(try response(#"{"accepted":false,"presentations":["3d","plan"]}"#).supportsPlan)
        XCTAssertFalse(try response(#"{"accepted":true,"presentations":["3d"]}"#).supportsPlan)
        XCTAssertTrue(try response(#"{"accepted":true,"presentations":["3d","plan"]}"#).supportsPlan)
        XCTAssertThrowsError(try response(#"{"accepted":true,"presentations":["arbitrary"]}"#))
    }

}
