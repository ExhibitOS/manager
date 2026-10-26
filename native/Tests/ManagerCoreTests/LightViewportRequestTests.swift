// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class LightViewportRequestTests: XCTestCase {
    func fixture() throws -> (NativeExhibitionDraft, LightCommand, ViewportFrame) {
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        let id = "aaaaaaaa-0000-4000-8000-000000000001"
        candidate["lights"] = .array([.object(["id": .string(id), "roomId": draft.rooms[0]["id"], "type": .string("point"),
                                              "unit": .string("candela"), "transform": NativeExhibitionDraft.identity,
                                              "color": .vector([1, 1, 1]), "intensity": .integer(100), "castsShadow": .bool(false)])])
        try draft.replaceCandidate(candidate)
        var fields = try LightFormBuffer(draft: draft, selectedLightId: id); fields.intensity = "700"
        let frame = ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 8, tenantId: UUID().uuidString.lowercased(), principalId: UUID().uuidString.lowercased(), draft: draft, selection: nil, mode: "translate", view: "isometric", disabled: false)
        return (draft, try fields.command(), frame)
    }
    func reply(_ request: LightViewportRequest) throws -> DocumentValue {
        var fields: [String: DocumentValue] = [:]
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "lightId"] { fields[key] = request.value[key] }
        fields["candidate"] = try request.command.proposedCandidate(in: request.frame.draft, selectedLightId: request.command.lightId)
        return .object(fields)
    }
    func testOptionalClosedEnvelopeAcceptsOnlyBoundCorrespondingResult() throws {
        let (draft, command, frame) = try fixture()
        let request = try LightViewportRequest(frame: frame, command: command, selectedLightId: command.lightId)
        guard case .object(let fields) = request.value else { return XCTFail("Expected object") }
        XCTAssertEqual(Set(fields.keys), Set(["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "type", "lightId", "light"]))
        XCTAssertEqual(request.value["type"], .string("set-light")); XCTAssertEqual(request.value["light"]["id"], .null)
        let preview = try request.checkedReply(reply(request), selectedLightId: command.lightId)
        XCTAssertEqual(preview.base, draft)
        XCTAssertEqual(preview.candidate["lights"].items![0]["intensity"], .number(700))
        let old = try JSONDecoder().decode(ViewportAcceptance.self, from: Data("{\"accepted\":true}".utf8))
        XCTAssertFalse(old.supportsLightInspector)
        let current = try JSONDecoder().decode(ViewportAcceptance.self, from: Data("{\"accepted\":true,\"capabilities\":[\"light-inspector-v1\"]}".utf8))
        XCTAssertTrue(current.supportsLightInspector)
        XCTAssertFalse(ViewportSelection(kind: "light", id: command.lightId).belongs(to: draft))
    }
    func testLateWrongScopeExtraFieldsAndUnrelatedCandidateAreRefused() throws {
        let (_, command, frame) = try fixture()
        let request = try LightViewportRequest(frame: frame, command: command, selectedLightId: command.lightId)
        let good = try reply(request)
        for key in ["version", "sequence", "epoch", "tenantId", "principalId", "requestId", "lightId"] {
            var changed = good; changed[key] = .string("stale")
            XCTAssertThrowsError(try request.checkedReply(changed, selectedLightId: command.lightId))
        }
        var changed = good; changed["extra"] = .bool(true)
        XCTAssertThrowsError(try request.checkedReply(changed, selectedLightId: command.lightId))
        changed = good; changed["candidate"]["title"] = .string("unrelated")
        XCTAssertThrowsError(try request.checkedReply(changed, selectedLightId: command.lightId))
        XCTAssertThrowsError(try request.checkedReply(good, selectedLightId: nil))
        let disabled = ViewportFrame(epoch: frame.epoch, sequence: frame.sequence, tenantId: frame.tenantId, principalId: frame.principalId, draft: frame.draft, selection: nil, mode: "translate", view: "isometric", disabled: true)
        XCTAssertThrowsError(try LightViewportRequest(frame: disabled, command: command, selectedLightId: command.lightId))
    }
}
