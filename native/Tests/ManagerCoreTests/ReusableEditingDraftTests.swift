// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class ReusableEditingDraftTests: XCTestCase {
    func testLookupBindsExactPublicationDraftAndRevision() throws {
        let draft = try NativeExhibitionDraft.new()
        let publication = UUID().uuidString.lowercased()
        let draftJSON = try JSONSerialization.jsonObject(with: JSONEncoder().encode(draft))
        let base: [String: Any] = ["capability": "reusable-published-draft-v1", "sourcePublicationId": publication,
            "authoringExhibitionId": draft.id, "reused": true, "revision": 2,
            "etag": "\"studio-r2-" + String(repeating: "a", count: 64) + "\"", "draft": draftJSON,
            "currentPublication": NSNull(), "previousPublication": NSNull()]
        func decode(_ value: [String: Any]) throws -> NativeReusableEditingDraft {
            try JSONDecoder().decode(NativeReusableEditingDraft.self, from: JSONSerialization.data(withJSONObject: value))
        }
        XCTAssertEqual(try decode(base).validated(expectedPublicationId: publication).draft.id, draft.id)
        XCTAssertThrowsError(try decode(base).validated(expectedPublicationId: UUID().uuidString))
        for (key, value) in [("capability", "unsupported"), ("authoringExhibitionId", UUID().uuidString), ("etag", "\"studio-r1-" + String(repeating: "a", count: 64) + "\"")] {
            var changed = base; changed[key] = value
            XCTAssertThrowsError(try decode(changed).validated(expectedPublicationId: publication))
        }
        var changed = base; changed["reused"] = false
        XCTAssertThrowsError(try decode(changed).validated(expectedPublicationId: publication))
    }
}
