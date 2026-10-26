// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class PlaqueViewportRequestTests: XCTestCase {
    func fixture() throws -> PlaqueViewportRequest {
        let draft = try NativeExhibitionDraft.new(), fields = try PlaqueFormBuffer(roomId: XCTUnwrap(draft.rooms.first?["id"].string))
        let frame = ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 8, tenantId: UUID().uuidString.lowercased(), principalId: UUID().uuidString.lowercased(), draft: draft, selection: nil, mode: "translate", view: "isometric", disabled: false)
        return try PlaqueViewportRequest(frame: frame, command: .add(fields.settings()), selectedPlaqueId: fields.id)
    }
    func reply(_ request: PlaqueViewportRequest) throws -> DocumentValue {
        var fields: [String: DocumentValue] = [:]
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"] { fields[key] = request.value[key] }
        fields["plaqueId"] = .string(request.command.plaqueId)
        fields["candidate"] = try request.command.proposedCandidate(in: request.frame.draft, selectedPlaqueId: request.command.plaqueId)
        return .object(fields)
    }
    func testOptionalTopLevelCommandAndLegacyStrictFrameStaySeparate() throws {
        let request = try fixture()
        guard case .object(let fields) = request.value else { return XCTFail("Expected object") }
        XCTAssertEqual(Set(fields.keys), ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "type", "plaque"])
        XCTAssertEqual(request.value["type"], .string("add-plaque"))
        XCTAssertEqual(try request.checkedReply(reply(request), selectedPlaqueId: request.command.plaqueId).base, request.frame.draft)
        let legacy = try JSONDecoder().decode(ViewportAcceptance.self, from: Data("{\"accepted\":true}".utf8))
        XCTAssertFalse(legacy.supportsPlaqueInspector)
        let current = try JSONDecoder().decode(ViewportAcceptance.self, from: Data("{\"accepted\":true,\"capabilities\":[\"plaque-inspector-v1\"]}".utf8))
        XCTAssertTrue(current.supportsPlaqueInspector)
        XCTAssertFalse(ViewportSelection(kind: "plaque", id: request.command.plaqueId).belongs(to: request.frame.draft))
    }
    func testReplyScopeReplayBindingsExtraFieldsAndUnrelatedCandidateAreRefused() throws {
        let request = try fixture(), good = try reply(request)
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "plaqueId"] {
            var changed = good; changed[key] = .string("stale")
            XCTAssertThrowsError(try request.checkedReply(changed, selectedPlaqueId: request.command.plaqueId))
        }
        var changed = good; changed["accepted"] = .bool(true)
        XCTAssertThrowsError(try request.checkedReply(changed, selectedPlaqueId: request.command.plaqueId))
        changed = good; changed["candidate"]["title"] = .string("unrelated")
        XCTAssertThrowsError(try request.checkedReply(changed, selectedPlaqueId: request.command.plaqueId))
        XCTAssertThrowsError(try request.checkedReply(good, selectedPlaqueId: nil))
        let f = request.frame
        let disabled = ViewportFrame(epoch: f.epoch, sequence: f.sequence, tenantId: f.tenantId, principalId: f.principalId, draft: f.draft, selection: nil, mode: "translate", view: "isometric", disabled: true)
        XCTAssertThrowsError(try PlaqueViewportRequest(frame: disabled, command: request.command, selectedPlaqueId: request.command.plaqueId))
    }
}
