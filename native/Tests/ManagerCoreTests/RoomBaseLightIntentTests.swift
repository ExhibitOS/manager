// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class RoomBaseLightIntentTests: XCTestCase {
    func testOfficialRoomLightingRequiresTheExactOwnerCapabilities() {
        let occupied = "495cb1d9bb395c805b64e5572a9e1647e67b18c4e53f1668f658ce41f0ce2818"
        let fresh = "91833cef7d7cba12423e8feff506dc1fb072d107c1eb63b395d071b3f84e1b98"
        var owner = AuthoringSession(userId: "owner", tenantId: "tenant", role: "admin",
            csrfToken: "test", expiresAt: "future")
        owner.nativeSceneAuthoringProposalProfile = "saved-head-scene-v1"
        owner.nativeLightObjectAuthoringProposalProfile = LightObjectAuthoring.profile
        owner.roomBaseLightProfile = RoomBaseLightAction.profile
        owner.roomBaseLightPlacementProfile = RoomBaseLightPlacement.profile
        owner.roomCeilingFillProfile = RoomCeilingFill.profile
        owner.nativeRoomCeilingFillDefaultProfile = RoomCeilingFill.defaultProfile
        owner.nativePublicationProfile = "bounded-local-v1"
        XCTAssertTrue(NativeAutomationSession.admitsOfficialRoomLighting(manifest: occupied, session: owner))
        for rejected in [fresh, occupied.uppercased(), String(repeating: "0", count: 64)] {
            XCTAssertFalse(NativeAutomationSession.admitsOfficialRoomLighting(manifest: rejected, session: owner))
        }
        for capability: WritableKeyPath<AuthoringSession, String?> in [
            \.nativeSceneAuthoringProposalProfile, \.nativeLightObjectAuthoringProposalProfile,
            \.roomBaseLightProfile, \.roomBaseLightPlacementProfile, \.roomCeilingFillProfile,
            \.nativeRoomCeilingFillDefaultProfile, \.nativePublicationProfile,
        ] {
            var incomplete = owner
            incomplete[keyPath: capability] = nil
            XCTAssertFalse(NativeAutomationSession.admitsOfficialRoomLighting(manifest: occupied, session: incomplete))
        }
    }
    private let namespace = NativeLightFixture.namespace
    private let attachments = "org.exhibitos.studio/light-attachments"
    private func fixture(_ placement: RoomBaseLightPlacement? = .auto) throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(roomBaseLight: true), candidate = draft.candidate
        candidate["lights"][0]["intensity"] = .integer(0)
        candidate["lights"][0]["color"] = .vector([0.2, 0.3, 0.4])
        if let placement { candidate["extensions"][namespace]["lights"][0]["basePlacement"] = .string(placement.rawValue) }
        else {
            var entry = candidate["extensions"][namespace]["lights"][0]
            entry = without(entry, "basePlacement"); candidate["extensions"][namespace]["lights"][0] = entry
        }
        try draft.replaceCandidate(candidate); return draft
    }
    private func without(_ value: DocumentValue, _ key: String) -> DocumentValue {
        guard case .object(var fields) = value else { return value }; fields.removeValue(forKey: key); return .object(fields)
    }
    private func light(_ draft: NativeExhibitionDraft) -> DocumentValue { draft.candidate["lights"][0] }
    private func id(_ draft: NativeExhibitionDraft) throws -> String { try XCTUnwrap(light(draft)["id"].string) }
    private func details(_ candidate: DocumentValue) -> DocumentValue { candidate["extensions"][namespace]["lights"][0] }
    private func request(_ command: DocumentValue, _ draft: NativeExhibitionDraft) throws -> LightObjectRequest {
        try LightObjectRequest(command: command, frame: ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 1,
            tenantId: UUID().uuidString.lowercased(), principalId: UUID().uuidString.lowercased(), draft: draft,
            selection: nil, mode: "translate", view: "isometric", disabled: false))
    }
    private func reply(_ request: LightObjectRequest, candidate: DocumentValue) -> DocumentValue {
        var result: [String: DocumentValue] = [:]
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"] { result[key] = request.value[key] }
        result["candidate"] = candidate; result["lightId"] = .string(request.sourceLightId); return .object(result)
    }
    private func setCommand(_ draft: NativeExhibitionDraft, row: DocumentValue, settings: DocumentValue? = nil) throws -> DocumentValue {
        .object(["type": .string("set-light-object"), "lightId": .string(try id(draft)), "light": without(row, "id"),
            "settings": settings ?? without(details(draft.candidate), "lightId")])
    }
    func testPinnedNode24RangeGoldenValues() throws {
        // Node24.21.0 roomBaseLightRange(34959f7), not a sampled geometry fit.
        let vectors: [(Double, Double, Double, Double)] = [
            (12, 4, 8, 8.059776671843954), (4, 3, 4, 3.9102429592034307),
            (0.2, 0.1, 0.2, 0.1676305461424021), (18, 5, 13, 11.97914855071094),
            (1, 1, 1, 1.1445523142259597), (2.333, 3.7, 4.111, 4.083466970602309),
            (100, 40, 80, 50), (0.01, 0.01, 0.01, 0.1)]
        for (w, h, d, expected) in vectors {
            XCTAssertEqual(try RoomBaseLightIntent.range(width: w, height: h, depth: d), expected)
        }
        for bad in [0.0, -1, .infinity, .nan] {
            XCTAssertThrowsError(try RoomBaseLightIntent.range(width: bad, height: 4, depth: 8))
        }
    }

    func testCeilingFillUsesExactProducerBitsAndRetainsManualPoseAtZero() throws {
        var draft = try NativeExhibitionDraft.new(roomBaseLight: true, ceilingFill: true)
        let lightId = try id(draft)
        let roomId = try XCTUnwrap(draft.rooms.first?["id"].string)
        XCTAssertEqual(light(draft)["type"], .string("area"))
        XCTAssertEqual(light(draft)["intensity"].double?.bitPattern, 0x40565381fbdf09e0)
        XCTAssertEqual(light(draft)["transform"]["position"], .vector([0, 3.98, 0]))
        XCTAssertEqual(details(draft.candidate)["ceilingFill"]["brightness"], .number(0.5))
        XCTAssertEqual(details(draft.candidate)["range"], .null)
        let form = try LightFormBuffer(draft: draft, selectedLightId: lightId)
        XCTAssertEqual(form.ceilingFillBrightness, 0.5)
        XCTAssertEqual(try form.command().proposedCandidate(in: draft, selectedLightId: lightId), draft.candidate)
        var wideRoom = draft
        try wideRoom.updateRoom(roomId, name: "Large", width: 200, height: 1_500, depth: 100)
        XCTAssertGreaterThan(light(wideRoom)["intensity"].double ?? 0, 1_000_000)
        let wideForm = try LightFormBuffer(draft: wideRoom, selectedLightId: lightId)
        XCTAssertEqual(try wideForm.command().proposedCandidate(in: wideRoom, selectedLightId: lightId), wideRoom.candidate)
        try draft.updateRoom(roomId, name: "Resized", width: 8, height: 5, depth: 12)
        XCTAssertEqual(light(draft)["intensity"].double?.bitPattern, 0x405ab58eb3763fe8)

        let manual = try RoomBaseLightIntent.setPlacement(.manual, lightId: lightId, in: draft.candidate)
        try draft.replaceCandidate(manual)
        let edit = try request(.object(["type": .string("set-room-ceiling-fill-brightness"),
            "lightId": .string(lightId), "brightness": .integer(0)]), draft)
        let expected = try RoomBaseLightIntent.setCeilingFillBrightness(0, lightId: lightId, in: manual)
        XCTAssertEqual(expected["lights"][0]["intensity"].double, 0)
        XCTAssertEqual(expected["lights"][0]["transform"], manual["lights"][0]["transform"])
        XCTAssertEqual(expected["lights"][0]["dimensions"], manual["lights"][0]["dimensions"])
        XCTAssertEqual(details(expected)["basePlacement"], .string("manual"))
        _ = try edit.checkedReply(reply(edit, candidate: expected))
        var altered = expected
        altered["lights"][0]["transform"]["position"] = .vector([0, 4, 0])
        XCTAssertThrowsError(try edit.checkedReply(reply(edit, candidate: altered)))
    }

    func testNewDefaultRoomPreservesPowerUntilAnExplicitSameBrightnessEdit() throws {
        let legacy = try NativeExhibitionDraft.new(roomBaseLight: true, ceilingFill: true)
        var draft = try NativeExhibitionDraft.new(roomBaseLight: true, defaultCeilingFill: true)
        let lightId = try id(draft)
        let roomId = try XCTUnwrap(draft.rooms.first?["id"].string)
        XCTAssertEqual(draft.candidate["lights"].items?.map { $0["type"] }, [.string("area")])
        XCTAssertEqual(light(draft)["dimensions"], light(legacy)["dimensions"])
        XCTAssertEqual(light(draft)["transform"], light(legacy)["transform"])
        let legacyIntensity = try XCTUnwrap(light(legacy)["intensity"].double)
        XCTAssertEqual(light(draft)["intensity"].double, 5 * legacyIntensity)
        XCTAssertTrue(try LightFormBuffer(draft: draft, selectedLightId: lightId).ceilingFillUsesDefault)
        let beforeEdit = draft.candidate
        let edited = try RoomBaseLightIntent.setCeilingFillBrightness(0.5, lightId: lightId, in: beforeEdit)
        XCTAssertEqual(edited["lights"][0]["intensity"].double, legacyIntensity)
        XCTAssertEqual(details(edited)["ceilingFill"]["mode"], .null)
        XCTAssertEqual(try RoomBaseLightIntent.setCeilingFillBrightness(0.5, usesDefault: true, lightId: lightId, in: edited), beforeEdit)

        try draft.updateRoom(roomId, name: "Resized", width: 8, height: 5, depth: 12)
        let manual = try RoomBaseLightIntent.setPlacement(.manual, lightId: lightId, in: draft.candidate)
        XCTAssertEqual(details(manual)["ceilingFill"]["mode"], .string("default"))
        XCTAssertEqual(manual["lights"][0]["intensity"], light(draft)["intensity"])
        XCTAssertEqual(try RoomBaseLightIntent.setPlacement(.auto, lightId: lightId, in: manual), draft.candidate)
    }

    func testUnsupportedDefaultEnsureDoesNotGenerateAPointLight() throws {
        var draft = try NativeExhibitionDraft.new()
        var candidate = draft.candidate
        candidate["rooms"][0]["transform"]["scale"] = .vector([2, 1, 1])
        try draft.replaceCandidate(candidate)
        let roomId = try XCTUnwrap(draft.rooms.first?["id"].string)
        let selection = ViewportSelection(kind: "room", id: roomId)
        let action = RoomBaseLightAction.ensure(roomId: roomId, defaultCeilingFill: true)
        XCTAssertEqual(try action.encodedValue(in: draft, selected: selection)["ceilingFillProfile"],
                       .string(RoomCeilingFill.defaultProfile))
        XCTAssertFalse(try action.apply(candidate, to: &draft, selected: selection))
        XCTAssertEqual(draft.candidate, candidate)
        let point = try RoomBaseLightAction.appendingLight(to: candidate, room: draft.rooms[0],
            lightId: UUID().uuidString.lowercased(), ceilingFill: true)
        XCTAssertEqual(point["lights"][0]["type"], .string("point"))
        XCTAssertThrowsError(try action.apply(point, to: &draft, selected: selection))
    }

    func testNegotiatedCeilingFillAcceptsNearUnitRoomAndChecksExplicitCeilingDepth() throws {
        var draft = try NativeExhibitionDraft.new()
        var base = draft.candidate
        let roomId = try XCTUnwrap(base["rooms"][0]["id"].string)
        base["rooms"][0]["transform"]["rotation"] = .vector([0, 0, 0, 1.00000075])
        let ceilingId = UUID().uuidString.lowercased()
        base["surfaces"] = .array([.object([
            "id": .string(ceilingId), "roomId": .string(roomId), "type": .string("ceiling"),
            "dimensions": .object(["width": .integer(12), "height": .integer(8)]),
            "transform": .object(["position": .vector([0, 4, 0]),
                "rotation": .vector([0.7071067811865476, 0, 0, 0.7071067811865476]), "scale": .vector([1, 1, 1])])
        ])])
        try draft.replaceCandidate(base)
        let selection = ViewportSelection(kind: "room", id: roomId)
        let action = RoomBaseLightAction.ensure(roomId: roomId, ceilingFill: true)
        let lightId = UUID().uuidString.lowercased()
        XCTAssertEqual(try action.encodedValue(in: draft, selected: selection)["ceilingFillProfile"], .string(RoomCeilingFill.profile))
        let reply = try RoomBaseLightAction.appendingLight(to: base, room: base["rooms"][0], lightId: lightId, ceilingFill: true)
        XCTAssertEqual(reply["lights"][0]["intensity"].double?.bitPattern, 0x40565381fbdf09e0)
        XCTAssertTrue(try action.apply(reply, to: &draft, selected: selection))

        var thick = base
        thick["surfaces"][0]["dimensions"]["depth"] = .number(0.03)
        let fallback = try RoomBaseLightAction.appendingLight(to: thick, room: thick["rooms"][0], lightId: lightId, ceilingFill: true)
        XCTAssertEqual(fallback["lights"][0]["type"], .string("point"))
        XCTAssertEqual(try RoomBaseLightIntent.preserveCeilingFillAfterCeilingEdit(
            replyWithSurfaces(reply, surfaces: thick["surfaces"]), roomId: roomId
        )["extensions"][namespace]["lights"][0]["basePlacement"], .string("manual"))
    }

    private func replyWithSurfaces(_ candidate: DocumentValue, surfaces: DocumentValue) -> DocumentValue {
        var next = candidate
        next["surfaces"] = surfaces
        return next
    }
    func testPlacementMarkerIsIndependentOptionalCapability() throws {
        var raw = ["userId": "u", "tenantId": "t", "role": "admin", "csrfToken": "csrf", "expiresAt": "later", "roomBaseLightProfile": "room-base-light-v1"]
        func decode() throws -> AuthoringSession { try JSONDecoder().decode(AuthoringSession.self, from: JSONEncoder().encode(raw)) }
        XCTAssertFalse(try decode().advertisesRoomBaseLightPlacement)
        raw["roomBaseLightPlacementProfile"] = "room-base-light-placement-v1"
        XCTAssertTrue(try decode().advertisesRoomBaseLightPlacement)
        raw["roomBaseLightPlacementProfile"] = "unknown"
        XCTAssertFalse(try decode().advertisesRoomBaseLightPlacement)
        let unknown = try fixture(nil), fields = try LightFormBuffer(draft: unknown, selectedLightId: id(unknown))
        XCTAssertTrue(fields.isRoomBaseLight); XCTAssertNil(fields.basePlacement)
        XCTAssertEqual(try JSONDecoder().decode(NativeExhibitionDraft.self, from: JSONEncoder().encode(unknown)), unknown)
    }
    func testRGBAndOffEditKeepAutoAndGeneratedRange() throws {
        let draft = try fixture(), lightId = try id(draft)
        var fields = try LightFormBuffer(draft: draft, selectedLightId: lightId)
        fields.red = "0.5"; fields.intensity = "0"
        let candidate = try fields.command().proposedCandidate(in: draft, selectedLightId: lightId)
        XCTAssertEqual(details(candidate), details(draft.candidate))
        XCTAssertEqual(candidate["lights"][0]["color"], .vector([0.5, 0.3, 0.4]))
        XCTAssertEqual(candidate["lights"][0]["intensity"], .integer(0))
        var row = light(draft); row["color"] = .vector([0.4, 0.4, 0.4])
        let request = try request(setCommand(draft, row: row, settings: .object(["fixture": .string("none")])), draft)
        var expected = draft.candidate; expected["lights"][0] = row
        XCTAssertNoThrow(try request.checkedReply(reply(request, candidate: expected)))
        var bad = expected; bad["extensions"][namespace]["lights"][0] = without(details(expected), "basePlacement")
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: bad)))
    }
    func testSpatialEditsManualizeOnlyExplicitAuto() throws {
        for mode in [RoomBaseLightPlacement.auto, .manual, nil] as [RoomBaseLightPlacement?] {
            let draft = try fixture(mode), lightId = try id(draft)
            let command = LightCommand(lightId: lightId, type: .point, color: [0.2, 0.3, 0.4], intensity: 0, position: [1, 3.6, 0])
            let next = try command.proposedCandidate(in: draft, selectedLightId: lightId)
            XCTAssertEqual(details(next)["basePlacement"], mode == nil ? .null : .string("manual"))
            XCTAssertEqual(next["lights"][0]["intensity"], .integer(0))
        }
    }
    func testEquivalentQuaternionRetainsAutoButRealRotationManualizes() throws {
        let draft = try fixture()
        for q in [[0.0, 0, 0, -1], [0, 0, 0, 1.00000000000001]] {
            var row = light(draft); row["transform"]["rotation"] = .vector(q)
            let request = try request(setCommand(draft, row: row), draft)
            var expected = draft.candidate; expected["lights"][0] = row
            XCTAssertNoThrow(try request.checkedReply(reply(request, candidate: expected)))
        }
        var row = light(draft); row["transform"]["rotation"] = .vector([0, sqrt(0.5), 0, sqrt(0.5)])
        let request = try request(setCommand(draft, row: row), draft)
        var expected = draft.candidate; expected["lights"][0] = row; expected["extensions"][namespace]["lights"][0]["basePlacement"] = .string("manual")
        XCTAssertNoThrow(try request.checkedReply(reply(request, candidate: expected)))
        var position = light(draft)["transform"]; position["position"] = .vector([1e-15, 3.6, 0])
        XCTAssertFalse(RoomBaseLightIntent.samePose(light(draft)["transform"], position), "No positional tolerance")
        var zero = light(draft)["transform"]; zero["rotation"] = .vector([0, 0, 0, 0])
        XCTAssertFalse(RoomBaseLightIntent.samePose(light(draft)["transform"], zero))
    }
    func testRoomDimensionsReflowAutoWithoutRetuningAndWorldMoveStaysLocal() throws {
        var draft = try fixture(); let room = try XCTUnwrap(draft.rooms[0]["id"].string)
        try draft.updateRoom(room, name: "Changed", width: 18, height: 5, depth: 13)
        XCTAssertEqual(light(draft)["transform"]["position"], .vector([0, 4.5, 0]))
        XCTAssertEqual(details(draft.candidate)["range"], .number(11.97914855071094))
        XCTAssertEqual(light(draft)["intensity"], .integer(0)); XCTAssertEqual(light(draft)["color"], .vector([0.2, 0.3, 0.4]))
        let command = ArchitectureCommand.room(id: room, name: "Moved", width: 18, height: 5, depth: 13, position: [10, 1, -8])
        var expected = draft.candidate; expected["rooms"][0]["name"] = .string("Moved"); expected["rooms"][0]["transform"]["position"] = .vector([10, 1, -8])
        XCTAssertNoThrow(try command.checkProposal(expected, base: draft, selected: .init(kind: "room", id: room)))
        var bad = expected; bad["lights"][0]["transform"]["position"] = .vector([10, 5.5, -8])
        XCTAssertThrowsError(try command.checkProposal(bad, base: draft, selected: .init(kind: "room", id: room)))
    }
    func testRoomEditDoesNotMoveManualUnknownOrRecreateMissingBase() throws {
        for mode in [RoomBaseLightPlacement.manual, nil] as [RoomBaseLightPlacement?] {
            var draft = try fixture(mode); let before = draft.candidate
            try draft.updateRoom(draft.rooms[0]["id"].string!, name: "Changed", width: 18, height: 5, depth: 13)
            XCTAssertEqual(draft.candidate["lights"], before["lights"])
            XCTAssertEqual(draft.candidate["extensions"], before["extensions"])
        }
        var missing = try fixture(), raw = missing.candidate
        raw["lights"] = .array([]); raw["extensions"][namespace]["lights"] = .array([]); try missing.replaceCandidate(raw)
        try missing.updateRoom(missing.rooms[0]["id"].string!, name: "Changed", width: 18, height: 5, depth: 13)
        XCTAssertEqual(missing.candidate["lights"], .array([])); XCTAssertEqual(details(missing.candidate), .null)
    }
    func testRestoreAutoIsOneExactCandidateAndKeepsOffColourAndOtherBindings() throws {
        var draft = try fixture(.manual), raw = draft.candidate
        let lightId = try id(draft), room = draft.rooms[0]["id"], surface = UUID().uuidString.lowercased(), target = UUID().uuidString.lowercased()
        raw["surfaces"] = .array([.object(["id": .string(surface), "roomId": room, "type": .string("ceiling"), "dimensions": .object(["width": .number(12), "height": .number(8)]), "transform": NativeExhibitionDraft.identity])])
        raw["placements"] = .array([.object(["id": .string(target), "roomId": room, "artworkRevisionId": .string(UUID().uuidString.lowercased()), "assetId": .string(UUID().uuidString.lowercased()), "transform": NativeExhibitionDraft.identity])])
        raw["lights"][0]["targetPlacementId"] = .string(target)
        raw["extensions"][namespace]["lights"][0]["aim"] = .object(["kind": .string("point"), "position": .vector([1, 1, 0])])
        raw["extensions"][namespace]["lights"][0]["range"] = .integer(7)
        raw["extensions"][attachments] = .object(["version": .integer(1), "lights": .array([.object(["lightId": .string(lightId), "surfaceId": .string(surface), "anchor": .vector([0, 0]), "gap": .number(0.1)])])])
        raw["extensions"]["example/preserved"] = .object(["note": .string("untouched")]); try draft.replaceCandidate(raw)
        let command: DocumentValue = .object(["type": .string("set-room-base-light-placement"), "lightId": .string(lightId), "basePlacement": .string("auto")])
        let request = try request(command, draft)
        var expected = raw; expected["lights"][0] = without(expected["lights"][0], "targetPlacementId")
        expected["extensions"][namespace]["lights"][0] = without(details(expected), "aim")
        expected["extensions"][namespace]["lights"][0]["range"] = .number(8.059776671843954)
        expected["extensions"][namespace]["lights"][0]["basePlacement"] = .string("auto")
        expected["extensions"][attachments]["lights"] = .array([])
        let preview = try request.checkedReply(reply(request, candidate: expected))
        XCTAssertEqual(try preview.apply(to: draft).candidate, expected)
        XCTAssertEqual(expected["lights"][0]["intensity"], .integer(0)); XCTAssertEqual(expected["lights"][0]["color"], light(draft)["color"])
        var bad = expected; bad["lights"][0]["intensity"] = .integer(16)
        XCTAssertThrowsError(try request.checkedReply(reply(request, candidate: bad)))
        XCTAssertEqual(draft.candidate, raw, "Preparing never stages or saves")
    }
    func testPlacementCommandRejectsUnknownFieldsForeignPurposeAndDirectIntentSpoofing() throws {
        let draft = try fixture(), lightId = try id(draft)
        let command: DocumentValue = .object(["type": .string("set-room-base-light-placement"), "lightId": .string(lightId), "basePlacement": .string("manual")])
        var extra = command; extra["intensity"] = .integer(16)
        XCTAssertThrowsError(try request(extra, draft))
        var invalid = command; invalid["basePlacement"] = .string("unknown")
        XCTAssertThrowsError(try request(invalid, draft))
        var raw = draft.candidate; raw["extensions"][namespace]["lights"][0] = without(details(raw), "purpose")
        var unmarked = draft; try unmarked.replaceCandidate(raw)
        XCTAssertThrowsError(try request(command, unmarked))
        var spoof = without(details(draft.candidate), "lightId"); spoof["basePlacement"] = .string("manual")
        XCTAssertThrowsError(try request(setCommand(draft, row: light(draft), settings: spoof), draft))
    }
    func testPlacementAbsenceRemainsUnknownWhileNullUnknownAndForeignPurposeRefuse() throws {
        let unknown = try fixture(nil), lightId = try id(unknown)
        XCTAssertNil(try RoomBaseLightIntent.placement(in: unknown, lightId: lightId))
        for bad in [DocumentValue.null, .string("unknown"), .bool(false)] {
            var raw = unknown.candidate; raw["extensions"][namespace]["lights"][0]["basePlacement"] = bad
            var draft = unknown; try draft.replaceCandidate(raw)
            XCTAssertThrowsError(try NativeLightFixture.settings(in: draft, lightId: lightId))
            XCTAssertThrowsError(try LightFormBuffer(draft: draft, selectedLightId: lightId), "Explicit malformed intent must not become UNKNOWN")
        }
        var raw = unknown.candidate
        raw["extensions"][namespace]["lights"][0]["basePlacement"] = .string("auto")
        raw["extensions"][namespace]["lights"][0] = without(details(raw), "purpose")
        var foreign = unknown; try foreign.replaceCandidate(raw)
        XCTAssertThrowsError(try NativeLightFixture.settings(in: foreign, lightId: lightId))
    }

}


/// Test-only synthetic graph construction; no production JSON API is widened.
private extension DocumentValue {
    subscript(index: Int) -> DocumentValue {
        get { guard let rows = items, rows.indices.contains(index) else { return .null }; return rows[index] }
        set {
            guard case .array(var rows) = self, rows.indices.contains(index) else { preconditionFailure("Invalid synthetic fixture index") }
            rows[index] = newValue; self = .array(rows)
        }
    }
}
