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
    func testLegacyRoomBaseCarryPreservesThePointAndAbsentPlacementMarker() throws {
        let original = try fixture()
        let id = "cccccccc-0000-4000-8000-000000000003"
        let point: DocumentValue = .object([
            "id": .string(id), "roomId": original.rooms[0]["id"], "type": .string("point"),
            "unit": .string("candela"), "color": .vector([1, 1, 1]), "intensity": .integer(16),
            "transform": .object(["position": .vector([0, 3.6, 0]), "rotation": .vector([0, 0, 0, 1]),
                                  "scale": .vector([1, 1, 1])]), "castsShadow": .bool(false)
        ])
        let settings: DocumentValue = .object(["fixture": .string("none"), "purpose": .string("room-base"), "range": .integer(10)])
        let carried = try NativeAutomationScene.legacyRoomBaseCarry(light: point, settings: settings, draft: original)
        var expected = original.candidate
        expected["lights"] = .array(try XCTUnwrap(original.candidate["lights"].items) + [point])
        expected["extensions"] = .object([NativeLightFixture.namespace:
            .object(["version": .integer(1), "lights": .array([
                .object(["lightId": .string(id), "fixture": .string("none"), "purpose": .string("room-base"), "range": .integer(10)])
            ])])
        ])
        XCTAssertEqual(carried.candidate, expected)

        var invalidPose = point; invalidPose["transform"]["rotation"] = .vector([0, 0, 0, 2])
        XCTAssertThrowsError(try NativeAutomationScene.legacyRoomBaseCarry(light: invalidPose, settings: settings, draft: original))
        var outsideRoom = point; outsideRoom["transform"]["position"] = .vector([0, 4.1, 0])
        XCTAssertThrowsError(try NativeAutomationScene.legacyRoomBaseCarry(light: outsideRoom, settings: settings, draft: original))
        var another = point; another["id"] = .string("dddddddd-0000-4000-8000-000000000004")
        XCTAssertThrowsError(try NativeAutomationScene.legacyRoomBaseCarry(light: another, settings: settings, draft: carried))
        var auto = settings; auto["basePlacement"] = .string("auto")
        XCTAssertThrowsError(try NativeAutomationScene.legacyRoomBaseCarry(light: point, settings: auto, draft: original))
        var nullPlacement = settings
        if case .object(var fields) = settings {
            fields["basePlacement"] = .null
            nullPlacement = .object(fields)
        }
        XCTAssertThrowsError(try NativeAutomationScene.legacyRoomBaseCarry(light: point, settings: nullPlacement, draft: original))
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
    func testExplicitCeilingFillUsesTheCanonicalPanelAndRefusesPointFallback() throws {
        let base = try fixture(), room = try XCTUnwrap(base.rooms.first?["id"].string)
        let id = UUID().uuidString.lowercased()
        let input: DocumentValue = .object(["type": .string("ensure-room-base-light"), "roomId": .string(room),
                                           "lightId": .string(id), "ceilingFillProfile": .string(RoomCeilingFill.profile)])
        guard case .addition(let request) = try NativeAutomationScene.roomBaseRequest(input, frame: frame(base)) else {
            return XCTFail("Expected ceiling fill addition")
        }
        XCTAssertEqual(request.value["light"]["type"], .string("area"))
        XCTAssertEqual(request.value["light"]["unit"], .string("lumen"))
        XCTAssertEqual(request.value["settings"]["basePlacement"], .string("auto"))
        XCTAssertEqual(request.value["settings"]["ceilingFill"], .object(["version": .integer(1), "brightness": .number(0.5)]))
        let applied = try request.checkedReply(reply(request)).apply(to: base)
        XCTAssertEqual(applied.candidate["lights"].items?.count, 2)
        guard case .existing(let existing) = try NativeAutomationScene.roomBaseRequest(input, frame: frame(applied)) else {
            return XCTFail("Expected existing ceiling fill")
        }
        XCTAssertEqual(try existing.apply(to: applied), applied)
        var invalid = base, candidate = base.candidate
        var rooms = try XCTUnwrap(candidate["rooms"].items)
        rooms[0]["transform"]["scale"] = .vector([2, 1, 1]); candidate["rooms"] = .array(rooms)
        try invalid.replaceCandidate(candidate)
        XCTAssertThrowsError(try NativeAutomationScene.roomBaseRequest(input, frame: frame(invalid))) {
            XCTAssertEqual(($0 as? AuthoringFailure)?.code, "ROOM_CEILING_FILL_GEOMETRY")
        }
        var unsupported = input; unsupported["ceilingFillProfile"] = .string("different-profile")
        XCTAssertThrowsError(try NativeAutomationScene.roomBaseRequest(unsupported, frame: frame(base)))
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

    func testSupplementalFillPreservesLegacyPointAndChecksTheExactReply() throws {
        var draft = try fixture(), candidate = draft.candidate
        let point = try XCTUnwrap(candidate["lights"].items?.first)
        let legacy: DocumentValue = .object(["lightId": .string(light), "fixture": .string("none"),
                                               "purpose": .string("room-base"), "range": .integer(9)])
        let ns = NativeLightFixture.namespace
        candidate["extensions"] = .object([ns: .object(["version": .integer(1), "lights": .array([legacy])])])
        try draft.replaceCandidate(candidate)
        let nextId = "cccccccc-0000-4000-8000-000000000003"
        let input: DocumentValue = .object(["type": .string(NativeAutomationScene.supplementalCeilingFillCommand),
            "roomId": draft.rooms[0]["id"], "baseLightId": .string(light), "lightId": .string(nextId)])
        let request = try NativeAutomationScene.supplementalCeilingFillRequest(input, frame: frame(draft))
        XCTAssertEqual(request.value["baseLightId"], .string(light))
        let preview = try request.checkedReply(reply(request))
        let applied = try preview.apply(to: draft)
        let lights = try XCTUnwrap(applied.candidate["lights"].items)
        XCTAssertEqual(lights.count, 2)
        XCTAssertEqual(lights[0], point)
        XCTAssertEqual(lights[1]["type"], .string("area"))
        XCTAssertEqual(lights[1]["unit"], .string("lumen"))
        XCTAssertEqual(lights[1]["castsShadow"], .bool(false))
        let bindings = try NativeLightFixture.bindings(in: applied.candidate)
        XCTAssertEqual(bindings, [legacy, .object(["lightId": .string(nextId), "fixture": .string("none"),
            "purpose": RoomCeilingFill.supplementalPurpose, "basePlacement": .string("auto"),
            "ceilingFill": .object(["version": .integer(1), "brightness": .number(0.5)])])])

        var wrong = reply(request), changedLights = lights
        changedLights[0]["intensity"] = .number(101)
        wrong["candidate"]["lights"] = .array(changedLights)
        XCTAssertThrowsError(try request.checkedReply(wrong))
        XCTAssertThrowsError(try preview.apply(to: applied))
        var duplicate = input; duplicate["lightId"] = .string(UUID().uuidString.lowercased())
        XCTAssertThrowsError(try NativeAutomationScene.supplementalCeilingFillRequest(duplicate, frame: frame(applied)))
        var collision = input; collision["lightId"] = .string(placement)
        XCTAssertThrowsError(try NativeAutomationScene.supplementalCeilingFillRequest(collision, frame: frame(draft)))
        var marked = legacy; marked["basePlacement"] = .string("manual")
        var markedDraft = draft
        candidate["extensions"][ns]["lights"] = .array([marked])
        try markedDraft.replaceCandidate(candidate)
        XCTAssertThrowsError(try NativeAutomationScene.supplementalCeilingFillRequest(input, frame: frame(markedDraft)))

        let resized = try RoomBaseLightIntent.reflow(applied.candidate, roomId: try XCTUnwrap(draft.rooms[0]["id"].string))
        XCTAssertEqual(resized, applied.candidate)
        var larger = applied.candidate, rooms = try XCTUnwrap(larger["rooms"].items)
        rooms[0]["dimensions"]["width"] = .number(16)
        larger["rooms"] = .array(rooms)
        let refit = try RoomBaseLightIntent.reflow(larger, roomId: try XCTUnwrap(rooms[0]["id"].string))
        XCTAssertEqual(refit["lights"].items?.first, point)
        XCTAssertNotEqual(refit["lights"].items?.last, lights[1])
        XCTAssertEqual(refit["extensions"][ns]["lights"].items, bindings)
    }

}
