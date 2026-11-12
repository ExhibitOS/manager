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
        let detailed = try NativeAutomationScene.additionRequest(.object(["light": row, "settings": .object(["fixture": .string("none"), "range": .number(3)])]), frame: f)
        XCTAssertEqual(detailed.expected["extensions"][NativeLightFixture.namespace]["lights"].items?.last?["range"], .number(3))
        let req = try NativeAutomationScene.additionRequest(.object(["light": row]), frame: f)
        let result = try req.checkedReply(reply(req)).apply(to: base)
        XCTAssertEqual(result.candidate["lights"].items?.count, 2)
        XCTAssertThrowsError(try NativeAutomationScene.additionRequest(.object(["light": row]), frame: frame(result)))
        var changed = reply(req); changed["lightId"] = .string(light); XCTAssertThrowsError(try req.checkedReply(changed))
    }
    func testRoomBaseCreationAndExistingEditedLightNoOp() throws {
        let base = try fixture(), room = try XCTUnwrap(base.rooms.first?["id"].string)
        let id = UUID().uuidString.lowercased()
        let input: DocumentValue = .object(["type": .string("ensure-room-base-light"), "roomId": .string(room), "lightId": .string(id)])
        guard case .addition(let request) = try NativeAutomationScene.roomBaseRequest(input, frame: frame(base)) else { return XCTFail("Expected addition") }
        XCTAssertEqual(request.value["type"], .string("add-light"))
        XCTAssertEqual(request.value["light"]["id"], .string(id))
        XCTAssertEqual(request.value["settings"]["purpose"], .string("room-base"))
        XCTAssertEqual(request.value["settings"]["basePlacement"], .string("auto"))
        XCTAssertEqual(request.value["settings"]["range"], .number(8.059776671843954))
        let preview = try request.checkedReply(reply(request))
        var applied = try preview.apply(to: base)
        XCTAssertThrowsError(try preview.apply(to: applied))
        var edited = applied.candidate, lights = try XCTUnwrap(edited["lights"].items)
        lights[lights.count - 1]["intensity"] = .number(37); edited["lights"] = .array(lights)
        try applied.replaceCandidate(edited)
        guard case .existing(let existing) = try NativeAutomationScene.roomBaseRequest(input, frame: frame(applied)) else { return XCTFail("Expected existing light") }
        let unchanged = try existing.apply(to: applied)
        XCTAssertEqual(unchanged, applied)
        XCTAssertEqual(unchanged.candidate["lights"].items?.count, 2)
    }
    func testRoomBaseRejectsWrongRoomDuplicateIdAndAlteredReply() throws {
        let base = try fixture(), room = try XCTUnwrap(base.rooms.first?["id"].string)
        let input: DocumentValue = .object(["type": .string("ensure-room-base-light"), "roomId": .string(room), "lightId": .string(light)])
        XCTAssertThrowsError(try NativeAutomationScene.roomBaseRequest(input, frame: frame(base)))
        var bad = input; bad["roomId"] = .string(UUID().uuidString.lowercased()); XCTAssertThrowsError(try NativeAutomationScene.roomBaseRequest(bad, frame: frame(base)))
        bad = input; bad["extra"] = .bool(true); XCTAssertThrowsError(try NativeAutomationScene.roomBaseRequest(bad, frame: frame(base)))
        bad = input; bad["lightId"] = .string(UUID().uuidString.lowercased())
        guard case .addition(let request) = try NativeAutomationScene.roomBaseRequest(bad, frame: frame(base)) else { return XCTFail("Expected addition") }
        var changed = reply(request); changed["candidate"]["title"] = .string("changed")
        XCTAssertThrowsError(try request.checkedReply(changed))
    }

}
