// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class LightFixtureTests: XCTestCase {
    let first = "aaaaaaaa-0000-4000-8000-000000000001"
    let other = "bbbbbbbb-0000-4000-8000-000000000002"
    let namespace = NativeLightFixture.namespace
    func fixture(_ type: NativeLightType = .spot) throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        var pose = NativeExhibitionDraft.identity; pose["position"] = .vector([0, 2, 0])
        var row: DocumentValue = .object(["id": .string(first), "roomId": draft.rooms[0]["id"], "type": .string(type.rawValue),
            "unit": .string(type.unit), "color": .vector([0.7, 0.8, 0.9]), "intensity": .integer(100),
            "transform": pose, "castsShadow": .bool(false)])
        if type == .spot { row["beamAngle"] = .number(0.6123456789012345) }
        if type == .area { row["dimensions"] = .object(["width": .number(1.25), "height": .number(0.75)]) }
        var second = row; second["id"] = .string(other)
        candidate["lights"] = .array([row, second]); candidate["extensions"] = .object(["future": .object(["retain": .integer(17)])])
        try draft.replaceCandidate(candidate); return draft
    }
    func testFixtureEditUsesExistingOptionalEnvelopeAndPreservesBaseLights() throws {
        for (type, fixture) in [(NativeLightType.spot, NativeLightFixture.spotCan), (.point, .disc), (.area, .panel)] {
            let draft = try self.fixture(type); var fields = try LightFormBuffer(draft: draft, selectedLightId: first)
            XCTAssertFalse(fields.fixtureChanged); XCTAssertNil(try fields.command().fixture)
            XCTAssertEqual(try fields.command().proposedCandidate(in: draft, selectedLightId: first)["extensions"][namespace], .null)
            fields.fixture = fixture
            let command = try fields.command(), expected = try command.proposedCandidate(in: draft, selectedLightId: first)
            XCTAssertEqual(expected["lights"], draft.candidate["lights"])
            XCTAssertEqual(expected["extensions"]["future"], draft.candidate["extensions"]["future"])
            XCTAssertEqual(expected["extensions"][namespace]["lights"].items?[0]["fixture"], .string(fixture.rawValue))
            let frame = ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 5, tenantId: UUID().uuidString.lowercased(), principalId: UUID().uuidString.lowercased(), draft: draft, selection: nil, mode: "translate", view: "isometric", disabled: false)
            let request = try LightViewportRequest(frame: frame, command: command, selectedLightId: first)
            XCTAssertEqual(request.value["type"], .string("set-light-details"))
            XCTAssertEqual(request.value["settings"], .object(["fixture": .string(fixture.rawValue)]))
            var response = request.value
            if case .object(var values) = response {
                values.removeValue(forKey: "type"); values.removeValue(forKey: "light"); values.removeValue(forKey: "settings")
                values["candidate"] = expected; response = .object(values)
            }
            let preview = try request.checkedReply(response, selectedLightId: first)
            let applied = try preview.apply(to: draft, selectedLightId: first, command: command)
            XCTAssertEqual(applied.candidate, expected)
            var reopened = try JSONDecoder().decode(NativeExhibitionDraft.self, from: JSONEncoder().encode(applied))
            XCTAssertEqual(reopened.candidate, expected)
            try reopened.replaceCandidate(draft.candidate)
            XCTAssertEqual(reopened.candidate, draft.candidate, "Existing Undo candidate restores exact absence")
            var tampered = response; tampered["candidate"]["title"] = .string("foreign")
            XCTAssertThrowsError(try request.checkedReply(tampered, selectedLightId: first))
            tampered = response; tampered["sequence"] = .integer(4)
            XCTAssertThrowsError(try request.checkedReply(tampered, selectedLightId: first))
            XCTAssertThrowsError(try request.checkedReply(response, selectedLightId: other))
            if let directory = ProcessInfo.processInfo.environment["NATIVE_LIGHT_FIXTURE_PROOF_DIR"] {
                let proof: DocumentValue = .object(["base": draft.candidate, "request": request.value, "expected": expected])
                try JSONEncoder().encode(proof).write(to: URL(fileURLWithPath: directory).appendingPathComponent(fixture.rawValue + ".json"), options: .atomic)
            }
        }
    }
    func testRemovingFixturePreservesAimTemperatureRangeSoftnessAndOtherBindings() throws {
        var draft = try fixture(), candidate = draft.candidate
        let retained: DocumentValue = .object(["lightId": .string(other), "fixture": .string("none"), "range": .number(9.87654321)])
        let settings: DocumentValue = .object(["lightId": .string(first), "fixture": .string("spot-can"),
            "aim": .object(["kind": .string("point"), "position": .vector([1, 1, 0])]),
            "temperatureKelvin": .integer(4500), "penumbra": .number(0.123456789), "range": .number(7.123456789)])
        candidate["extensions"][namespace] = .object(["version": .integer(1), "lights": .array([settings, retained])])
        try draft.replaceCandidate(candidate)
        var fields = try LightFormBuffer(draft: draft, selectedLightId: first); fields.fixture = .none
        let next = try fields.command().proposedCandidate(in: draft, selectedLightId: first)
        var expected = settings; expected["fixture"] = .string("none")
        XCTAssertEqual(next["extensions"][namespace]["lights"], .array([expected, retained]))
        XCTAssertEqual(next["lights"], candidate["lights"])
    }
    func testIncompatibleUnsupportedAndOversizedMetadataRefuseWithoutReplacement() throws {
        let draft = try fixture(.directional); var fields = try LightFormBuffer(draft: draft, selectedLightId: first)
        fields.fixture = .disc
        XCTAssertThrowsError(try fields.command().proposedCandidate(in: draft, selectedLightId: first))
        var changed = try fixture(), candidate = changed.candidate
        candidate["extensions"][namespace] = .object(["version": .integer(2), "future": .string("preserve")])
        try changed.replaceCandidate(candidate)
        fields = try LightFormBuffer(draft: changed, selectedLightId: first)
        XCTAssertFalse(fields.fixtureEditable)
        fields.intensity = "125.5"
        XCTAssertEqual(try fields.command().proposedCandidate(in: changed, selectedLightId: first)["extensions"][namespace], candidate["extensions"][namespace])
        fields.fixture = .spotCan; XCTAssertThrowsError(try fields.command())
        candidate["extensions"][namespace] = .object(["version": .integer(1), "lights": .array((0..<33).map { _ in .object(["lightId": .string(UUID().uuidString), "fixture": .string("none")]) })])
        try changed.replaceCandidate(candidate)
        XCTAssertThrowsError(try NativeLightFixture.settings(in: changed, lightId: first))
        candidate["extensions"][namespace] = .object(["version": .integer(1), "lights": .array([.object(["lightId": .string(first), "fixture": .string("future")])])])
        try changed.replaceCandidate(candidate)
        XCTAssertThrowsError(try NativeLightFixture.settings(in: changed, lightId: first))
    }
    func testOptionalCapabilityCannotBeInferredFromBasicLightOrAcceptedFalse() throws {
        func acceptance(_ json: String) throws -> ViewportAcceptance { try JSONDecoder().decode(ViewportAcceptance.self, from: Data(json.utf8)) }
        XCTAssertFalse(try acceptance("{\"accepted\":true,\"capabilities\":[\"light-inspector-v1\"]}").supportsLightFixture)
        XCTAssertFalse(try acceptance("{\"accepted\":false,\"capabilities\":[\"light-fixture-aim-v1\"]}").supportsLightFixture)
        XCTAssertTrue(try acceptance("{\"accepted\":true,\"capabilities\":[\"light-fixture-aim-v1\"]}").supportsLightFixture)
    }
}
