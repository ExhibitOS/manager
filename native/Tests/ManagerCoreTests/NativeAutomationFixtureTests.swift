// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class NativeAutomationFixtureTests: XCTestCase {
    let lightId = "aaaaaaaa-0000-4000-8000-000000000001"
    let surfaceId = "bbbbbbbb-0000-4000-8000-000000000002"
    func fixture() throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        let room = draft.rooms[0]["id"]
        candidate["lights"] = .array([.object(["id": .string(lightId), "roomId": room, "type": .string("spot"), "unit": .string("candela"), "transform": NativeExhibitionDraft.identity, "color": .vector([1,1,1]), "intensity": .integer(100), "castsShadow": .bool(false), "beamAngle": .number(0.5)])])
        candidate["surfaces"] = .array([.object(["id": .string(surfaceId), "roomId": room, "type": .string("ceiling"), "dimensions": .object(["width": .number(4), "height": .number(4)]), "transform": .object(["position": .vector([0,3,0]), "rotation": .vector([1,0,0,0]), "scale": .vector([1,1,1])])])])
        try draft.replaceCandidate(candidate); return draft
    }
    func frame(_ draft: NativeExhibitionDraft) -> ViewportFrame {
        ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 1, tenantId: UUID().uuidString.lowercased(), principalId: UUID().uuidString.lowercased(), draft: draft, selection: nil, mode: "translate", view: "isometric", disabled: false)
    }
    func input() -> DocumentValue {
        .object(["lightId": .string(lightId), "attachment": .object(["surfaceId": .string(surfaceId), "anchor": .vector([0.5,0.25]), "gap": .number(0.2)])])
    }
    func reply(_ request: NativeAutomationFixture.AttachmentRequest) -> DocumentValue {
        var fields: [String: DocumentValue] = [:]
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "lightId"] { fields[key] = request.value[key] }
        fields["candidate"] = request.expected; return .object(fields)
    }
    func testAttachmentChecksWholeCandidateAndNoOp() throws {
        let base = try fixture(), request = try NativeAutomationFixture.AttachmentRequest(input(), frame: frame(base))
        let preview = try request.checkedReply(reply(request)), applied = try preview.apply(to: base)
        XCTAssertEqual(applied.candidate["lights"].items?[0]["transform"]["position"], .vector([0.5,2.75,-0.2]))
        XCTAssertThrowsError(try preview.apply(to: applied))
        var corrupted = reply(request)
        corrupted["candidate"]["title"] = .string("unrelated")
        XCTAssertThrowsError(try request.checkedReply(corrupted))
        let again = try NativeAutomationFixture.AttachmentRequest(input(), frame: frame(applied))
        XCTAssertTrue(again.unchanged)
        XCTAssertEqual(try again.checkedReply(reply(again)).apply(to: applied), applied)
    }
    func testAttachmentRejectsWallOutOfBoundsAndExtraInput() throws {
        let base = try fixture()
        var bad = input(); bad["attachment"]["anchor"] = .vector([9,0]); XCTAssertThrowsError(try NativeAutomationFixture.AttachmentRequest(bad, frame: frame(base)))
        bad = input(); bad["unexpected"] = .bool(true); XCTAssertThrowsError(try NativeAutomationFixture.AttachmentRequest(bad, frame: frame(base)))
        var wall = base, candidate = base.candidate, surfaces = try XCTUnwrap(candidate["surfaces"].items)
        surfaces[0]["type"] = .string("wall"); candidate["surfaces"] = .array(surfaces); try wall.replaceCandidate(candidate)
        XCTAssertThrowsError(try NativeAutomationFixture.AttachmentRequest(input(), frame: frame(wall)))
    }
    func testFixtureRequiresExistingSpotAndClosedInput() throws {
        let base = try fixture(), f = frame(base)
        let value: DocumentValue = .object(["lightId": .string(lightId), "pose": .object(["position": .vector([0,0.15,0.0755]), "yaw": .number(0), "scale": .number(1)])])
        let request = try NativeAutomationFixture.fixtureRequest(value, frame: f)
        XCTAssertEqual(request.value["type"], .string("set-furnishing-visual"))
        XCTAssertEqual(request.value["catalogId"], .string("museum-track-spot"))
        var bad = value; bad["unknown"] = .bool(true); XCTAssertThrowsError(try NativeAutomationFixture.fixtureRequest(bad, frame: f))
    }
    func testFixtureExistingNoOpPreservesTemplateAndRejectsScopeDrift() throws {
        var base = try fixture(), candidate = base.candidate
        let assetId = UUID().uuidString.lowercased()
        let asset = try SharedFurnishingCatalog.asset(catalogId: "museum-track-spot", id: assetId)
        let pose: DocumentValue = .object(["position": .vector([0,0.15,0.0755]), "yaw": .number(0), "scale": .number(1)])
        candidate["extensions"] = .object(["org.exhibitos.studio/furnishing-visuals": .object(["version": .integer(1), "assets": .array([asset]), "bindings": .array([.object(["hostKind": .string("light"), "hostId": .string(lightId), "assetId": .string(assetId), "pose": pose])])]), "org.exhibitos.studio/template": .object(["modified": .bool(false)])])
        try base.replaceCandidate(candidate)
        let input: DocumentValue = .object(["lightId": .string(lightId), "pose": pose])
        let request = try NativeAutomationFixture.fixtureRequest(input, frame: frame(base))
        var fields: [String: DocumentValue] = [:]
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"] { fields[key] = request.value[key] }
        fields["candidate"] = base.candidate
        let preview = try NativeAutomationFixture.fixtureReply(.object(fields), request: request)
        XCTAssertEqual(try preview.apply(to: base), base)
        fields["requestId"] = .string(UUID().uuidString.lowercased())
        XCTAssertThrowsError(try NativeAutomationFixture.fixtureReply(.object(fields), request: request))
    }
    func testAttachmentRejectsForgedTransformAndReplyFields() throws {
        let base = try fixture(), request = try NativeAutomationFixture.AttachmentRequest(input(), frame: frame(base))
        var bad = reply(request)
        var candidate = bad["candidate"], lights = try XCTUnwrap(candidate["lights"].items)
        lights[0]["transform"]["position"] = .vector([0.5,2.75,-0.1]); candidate["lights"] = .array(lights); bad["candidate"] = candidate
        XCTAssertThrowsError(try request.checkedReply(bad))
        bad = reply(request); bad["extra"] = .bool(true); XCTAssertThrowsError(try request.checkedReply(bad))
        bad = reply(request); bad["lightId"] = .string(UUID().uuidString.lowercased()); XCTAssertThrowsError(try request.checkedReply(bad))
    }

    func testDownwardCeilingAffinePreservesOtherLightAndBinding() throws {
        var base = try fixture(), candidate = base.candidate
        var surfaces = try XCTUnwrap(candidate["surfaces"].items)
        let h = sqrt(0.5)
        surfaces[0]["transform"]["rotation"] = .vector([h,0,0,h])
        surfaces[0]["transform"]["scale"] = .vector([1.000001,1,1]); candidate["surfaces"] = .array(surfaces)
        let other = "cccccccc-0000-4000-8000-000000000003"
        var lights = try XCTUnwrap(candidate["lights"].items), otherLight = lights[0]
        otherLight["id"] = .string(other); lights.append(otherLight); candidate["lights"] = .array(lights)
        let otherBinding: DocumentValue = .object(["lightId": .string(other), "surfaceId": .string(surfaceId), "anchor": .vector([-0.5,0]), "gap": .number(0.1)])
        candidate["extensions"] = .object(["org.exhibitos.studio/light-attachments": .object(["version": .integer(1), "lights": .array([otherBinding])])])
        try base.replaceCandidate(candidate)
        let request = try NativeAutomationFixture.AttachmentRequest(input(), frame: frame(base))
        let accepted = try request.checkedReply(reply(request))
        let rows = try XCTUnwrap(accepted.candidate["lights"].items)
        let position = try XCTUnwrap(rows[0]["transform"]["position"].items?.compactMap(\.double))
        let rotation = try XCTUnwrap(rows[0]["transform"]["rotation"].items?.compactMap(\.double))
        for (actual, expected) in zip(position, [0.5000005, 2.8, 0.25]) { XCTAssertEqual(actual, expected, accuracy: 1e-9) }
        for (actual, expected) in zip(rotation, [0, h, h, 0]) { XCTAssertEqual(actual, expected, accuracy: 1e-9) }
        XCTAssertEqual(rows[1], otherLight)
        XCTAssertEqual(accepted.candidate["extensions"]["org.exhibitos.studio/light-attachments"]["lights"].items?.first, otherBinding)
        var forged = reply(request), changed = forged["candidate"], changedLights = try XCTUnwrap(changed["lights"].items)
        changedLights[0]["transform"]["rotation"] = .vector([0,h,-h,0]); changed["lights"] = .array(changedLights); forged["candidate"] = changed
        XCTAssertThrowsError(try request.checkedReply(forged))
    }

}
