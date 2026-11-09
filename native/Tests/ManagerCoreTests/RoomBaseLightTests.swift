// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class RoomBaseLightTests: XCTestCase {
    private let roomId = "aaaaaaaa-0000-4000-8000-000000000001"
    private let lightId = "bbbbbbbb-0000-4000-8000-000000000002"
    private let template = "org.exhibitos.studio/template"

    private func proposedLight(roomId: String) -> DocumentValue {
        .object(["id": .string(lightId), "roomId": .string(roomId), "type": .string("point"),
                 "transform": .object(["position": .vector([0, 3.6, 0]), "rotation": .vector([0, 0, 0, 1]),
                                       "scale": .vector([1, 1, 1])]),
                 "color": .vector([1, 1, 1]), "intensity": .integer(16), "unit": .string("candela"),
                 "castsShadow": .bool(false)])
    }

    private var detail: DocumentValue {
        .object(["lightId": .string(lightId), "fixture": .string("none"), "purpose": .string("room-base"),
                 "range": .integer(12)])
    }

    private func proposal(_ base: NativeExhibitionDraft, roomId: String) -> DocumentValue {
        var candidate = base.candidate
        candidate["lights"] = .array(candidate["lights"].items! + [proposedLight(roomId: roomId)])
        if candidate["extensions"] == .null { candidate["extensions"] = .object([:]) }
        candidate["extensions"][NativeLightFixture.namespace] = .object(["version": .integer(1), "lights": .array([detail])])
        return candidate
    }

    func testSessionAndViewportRequireExactProfile() throws {
        let fields = ["userId": "u", "tenantId": "t", "role": "admin", "csrfToken": "csrf", "expiresAt": "later"]
        func session(_ profile: String? = nil) throws -> AuthoringSession {
            var result = fields
            result["roomBaseLightProfile"] = profile
            return try JSONDecoder().decode(AuthoringSession.self, from: JSONEncoder().encode(result))
        }
        XCTAssertFalse(try session().advertisesRoomBaseLight)
        XCTAssertFalse(try session("room-base-light-v2").advertisesRoomBaseLight)
        XCTAssertTrue(try session("room-base-light-v1").advertisesRoomBaseLight)

        func ack(_ json: String) throws -> ViewportAcceptance {
            try JSONDecoder().decode(ViewportAcceptance.self, from: Data(json.utf8))
        }
        XCTAssertFalse(try ack(#"{"accepted":true,"capabilities":["light-fixture-aim-v1"]}"#).supportsRoomBaseLight)
        XCTAssertFalse(try ack(#"{"accepted":false,"capabilities":["room-base-light-v1"]}"#).supportsRoomBaseLight)
        XCTAssertFalse(try ack(#"{"accepted":true,"capabilities":["room-base-light-v1"]}"#).supportsRoomBaseLight)
        XCTAssertFalse(try ack(#"{"accepted":true,"boundedReadOnly":true,"capabilities":["room-base-light-v1"]}"#).supportsRoomBaseLight)
        XCTAssertTrue(try ack(#"{"accepted":true,"boundedReadOnly":false,"capabilities":["room-base-light-v1"]}"#).supportsRoomBaseLight)
    }

    func testSeededFirstRoomMatchesProducerWithoutChangingLegacyOrEditVersion() throws {
        let legacy = try NativeExhibitionDraft.new()
        XCTAssertEqual(legacy.candidate["lights"], .array([]))
        XCTAssertEqual(legacy.candidate["extensions"], .null)

        let draft = try NativeExhibitionDraft.new(roomBaseLight: true)
        let id = try XCTUnwrap(draft.candidate["lights"].items?.first?["id"].string)
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        var light = proposedLight(roomId: room)
        light["id"] = .string(id)
        var binding = detail
        binding["lightId"] = .string(id)
        XCTAssertEqual(draft.candidate["lights"], .array([light]))
        XCTAssertEqual(draft.candidate["extensions"][NativeLightFixture.namespace]["lights"], .array([binding]))
        XCTAssertEqual(draft.document["editVersion"], .integer(1))
        XCTAssertEqual(try JSONDecoder().decode(NativeExhibitionDraft.self, from: JSONEncoder().encode(draft)), draft)
    }

    func testAddRoomAcceptsOnlyTheExactRoomLightAndTemplateDelta() throws {
        var draft = try NativeExhibitionDraft.new()
        var original = draft.candidate
        original["extensions"] = .object(["opaque": .string("preserved"),
                                           template: .object(["modified": .bool(false), "id": .string("preset")])])
        try draft.replaceCandidate(original)
        let action = RoomBaseLightAction.addRoom(name: "두 번째 방")
        let room: DocumentValue = .object(["id": .string(roomId), "name": .string("두 번째 방"),
                                         "dimensions": .object(["width": .integer(12), "height": .integer(4), "depth": .integer(8)]),
                                         "transform": NativeExhibitionDraft.identity])
        var reply = proposal(draft, roomId: roomId)
        reply["rooms"] = .array(draft.rooms + [room])
        reply["extensions"][template]["modified"] = .bool(true)

        let before = draft
        var wrong = reply
        wrong["extensions"]["opaque"] = .string("changed")
        XCTAssertThrowsError(try action.apply(wrong, to: &draft, selected: nil))
        XCTAssertThrowsError(try draft.acceptGeometry(reply))
        XCTAssertEqual(draft, before)
        XCTAssertTrue(try action.apply(reply, to: &draft, selected: nil))
        XCTAssertEqual(draft.candidate, reply)
        XCTAssertEqual(draft.document["editVersion"], .integer(3))
    }

    func testEnsureRequiresSelectedRoomAndReapplicationIsAnExactNoOp() throws {
        var draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let selected = ViewportSelection(kind: "room", id: room)
        let action = RoomBaseLightAction.ensure(roomId: room)
        var unmarked = proposedLight(roomId: room)
        unmarked["id"] = .string("cccccccc-0000-4000-8000-000000000003")
        unmarked["intensity"] = .integer(29)
        var original = draft.candidate
        original["lights"] = .array([unmarked])
        try draft.replaceCandidate(original)

        let reply = proposal(draft, roomId: room)
        XCTAssertThrowsError(try action.apply(reply, to: &draft, selected: nil))
        XCTAssertThrowsError(try action.apply(original, to: &draft, selected: selected))
        XCTAssertTrue(try action.apply(reply, to: &draft, selected: selected))
        XCTAssertEqual(draft.candidate["lights"].items?.first, unmarked)

        var fields = try LightFormBuffer(draft: draft, selectedLightId: lightId)
        fields.intensity = "9"
        fields.range = "7"
        let command = try fields.command()
        let edited = try command.proposedCandidate(in: draft, selectedLightId: lightId)
        XCTAssertEqual(edited["extensions"][NativeLightFixture.namespace]["lights"].items?.first?["purpose"],
                       .string("room-base"))
        XCTAssertEqual(edited["extensions"][NativeLightFixture.namespace]["lights"].items?.first?["range"],
                       .integer(7))
        try draft.replaceCandidate(edited)
        let before = draft
        XCTAssertThrowsError(try action.apply(reply, to: &draft, selected: selected))
        XCTAssertFalse(try action.apply(edited, to: &draft, selected: selected))
        XCTAssertEqual(draft, before)
    }
}
