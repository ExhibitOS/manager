// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class NativeAutomationSceneTests: XCTestCase {
    let placement = "aaaaaaaa-0000-4000-8000-000000000001"
    let light = "bbbbbbbb-0000-4000-8000-000000000002"
    func fixture() throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        candidate["placements"] = .array([.object(["id": .string(placement), "roomId": draft.rooms[0]["id"], "transform": NativeExhibitionDraft.identity])])
        candidate["lights"] = .array([.object(["id": .string(light), "roomId": draft.rooms[0]["id"], "type": .string("point"), "unit": .string("candela"), "transform": NativeExhibitionDraft.identity, "color": .vector([1,1,1]), "intensity": .integer(100), "castsShadow": .bool(false)])])
        try draft.replaceCandidate(candidate); return draft
    }
    func frame(_ draft: NativeExhibitionDraft) -> ViewportFrame {
        ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 1, tenantId: UUID().uuidString.lowercased(), principalId: UUID().uuidString.lowercased(), draft: draft, selection: nil, mode: "translate", view: "isometric", disabled: false)
    }
    func reply(_ request: NativeAutomationScene.Proposal) -> DocumentValue {
        var fields: [String: DocumentValue] = [:]
        for k in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"] { fields[k] = request.value[k] }
        fields[request.targetKey] = .string(request.targetId); fields["candidate"] = request.expected
        return .object(fields)
    }
    func testPedestalPreviewAndStaleApply() throws {
        let base = try fixture(), f = frame(base)
        let ped: DocumentValue = .object(["kind": .string("box"), "position": .vector([0,0,1]), "yaw": .number(0), "dimensions": .object(["width": .number(1), "depth": .number(1), "height": .number(0.8)]), "material": .object(["preset": .string("paint"), "color": .string("#F2F2F2"), "roughness": .number(0.7), "metalness": .number(0)])])
        let request = try NativeAutomationScene.pedestalRequest(.object(["placementId": .string(placement), "pedestal": ped]), frame: f)
        let preview = try request.checkedReply(reply(request)); let applied = try preview.apply(to: base)
        XCTAssertEqual(try applied.installationSettings(for: placement)?.pedestal?.kind, .box)
        XCTAssertThrowsError(try preview.apply(to: applied))
        var bad = reply(request); bad["candidate"]["title"] = .string("changed"); XCTAssertThrowsError(try request.checkedReply(bad))
    }
    func testLightUpdateUsesExistingDetailValidation() throws {
        let base = try fixture(), f = frame(base)
        let input: DocumentValue = .object(["lightId": .string(light), "type": .string("point"), "color": .vector([1,1,1]), "intensity": .number(300), "position": .vector([1,2,3])])
        let request = try NativeAutomationScene.lightRequest(input, frame: f)
        XCTAssertEqual(request.value["type"], .string("set-light-details"))
        var fields: [String: DocumentValue] = [:]
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "lightId"] { fields[key] = request.value[key] }
        fields["candidate"] = try request.command.proposedCandidate(in: base, selectedLightId: light)
        let preview = try request.checkedReply(.object(fields), selectedLightId: light)
        XCTAssertEqual(try preview.apply(to: base, selectedLightId: light, command: request.command).candidate["lights"].items?[0]["intensity"], .number(300))
        var bad = input; bad["unexpected"] = .bool(true); XCTAssertThrowsError(try NativeAutomationScene.lightRequest(bad, frame: f))
    }
    func testNewLightBindsExactCandidateAndRejectsDuplicate() throws {
        let base = try fixture(), f = frame(base), id = UUID().uuidString.lowercased()
        let row: DocumentValue = .object(["id": .string(id), "roomId": base.rooms[0]["id"], "type": .string("point"), "unit": .string("candela"), "transform": NativeExhibitionDraft.identity, "color": .vector([1,1,1]), "intensity": .integer(2), "castsShadow": .bool(false)])
        var templated = base, templateCandidate = base.candidate
        templateCandidate["extensions"] = .object(["org.exhibitos.studio/template": .object(["modified": .bool(false)])])
        try templated.replaceCandidate(templateCandidate)
        XCTAssertEqual(templated.candidate["extensions"]["org.exhibitos.studio/template"]["modified"], .bool(false))
        let templatedReq = try NativeAutomationScene.additionRequest(.object(["light": row]), frame: frame(templated))
        XCTAssertEqual(templatedReq.expected["extensions"]["org.exhibitos.studio/template"]["modified"], .bool(true))
        let req = try NativeAutomationScene.additionRequest(.object(["light": row]), frame: f)
        let result = try req.checkedReply(reply(req)).apply(to: base)
        XCTAssertEqual(result.candidate["lights"].items?.count, 2)
        XCTAssertThrowsError(try NativeAutomationScene.additionRequest(.object(["light": row]), frame: frame(result)))
        var changed = reply(req); changed["lightId"] = .string(light); XCTAssertThrowsError(try req.checkedReply(changed))
    }
}
