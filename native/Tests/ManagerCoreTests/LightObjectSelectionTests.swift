// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class LightObjectSelectionTests: XCTestCase {
    func testLightSelectionBelongsOnlyToItsCurrentDraft() throws {
        let base = try NativeExhibitionDraft.new()
        let lightId = UUID().uuidString.lowercased()
        var document = base.document
        document["candidate"]["lights"] = .array([.object([
            "id": .string(lightId), "roomId": base.rooms[0]["id"], "type": .string("point"),
            "transform": NativeExhibitionDraft.identity, "color": .vector([1, 1, 1]),
            "intensity": .integer(16), "unit": .string("candela"), "castsShadow": .bool(false)
        ])])
        let draft = try NativeExhibitionDraft(document: document)
        let selection = ViewportSelection(kind: "light", id: lightId)
        XCTAssertTrue(selection.belongs(to: draft))
        XCTAssertFalse(selection.belongs(to: base))
        XCTAssertFalse(ViewportSelection(kind: "light", id: base.rooms[0]["id"].string!).belongs(to: draft))
        XCTAssertFalse(ViewportSelection(kind: "lamp", id: lightId).belongs(to: draft))
    }

    func testLightObjectNeedsItsExplicitEditableViewportAcknowledgement() throws {
        func accepted(_ fields: [String: Any]) throws -> Bool {
            try JSONDecoder().decode(ViewportAcceptance.self,
                from: JSONSerialization.data(withJSONObject: fields)).supportsLightObject
        }
        let supported: [String: Any] = ["accepted": true, "boundedReadOnly": false,
                                      "capabilities": [LightObjectAuthoring.capability]]
        XCTAssertTrue(try accepted(supported))
        var unknown = supported; unknown.removeValue(forKey: "boundedReadOnly")
        XCTAssertFalse(try accepted(unknown))
        var readOnly = supported; readOnly["boundedReadOnly"] = true
        XCTAssertFalse(try accepted(readOnly))
        var refused = supported; refused["accepted"] = false
        XCTAssertFalse(try accepted(refused))
        var old = supported; old["capabilities"] = ["light-inspector-v1", "light-fixture-aim-v1"]
        XCTAssertFalse(try accepted(old))
    }
}
