// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class LightColorIntentTests: XCTestCase {
    let first = "aaaaaaaa-0000-4000-8000-000000000001"
    let other = "bbbbbbbb-0000-4000-8000-000000000002"
    let namespace = NativeLightFixture.namespace
    func fixture(kelvin: Double? = nil) throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        var pose = NativeExhibitionDraft.identity; pose["position"] = .vector([0, 2, 0])
        var color = try kelvin.map { try LightColorIntent.linearRGB(kelvin: $0) } ?? [0.7, 0.8, 0.9]
        if kelvin != nil { color[1] += 1e-8 } // Valid shared tolerance; must not repaint on load.
        let row: DocumentValue = .object(["id": .string(first), "roomId": draft.rooms[0]["id"], "type": .string("spot"),
            "unit": .string("candela"), "color": .vector(color), "intensity": .integer(100),
            "beamAngle": .number(0.6123456789012345), "transform": pose, "castsShadow": .bool(false)])
        var second = row; second["id"] = .string(other)
        candidate["lights"] = .array([row, second]); candidate["extensions"] = .object(["future": .object(["retain": .integer(17)])])
        if let kelvin {
            candidate["extensions"][namespace] = .object(["version": .integer(1), "lights": .array([
                .object(["lightId": .string(first), "fixture": .string("none"), "temperatureKelvin": .number(kelvin),
                         "aim": .object(["kind": .string("point"), "position": .vector([0, 1, 1])]),
                         "range": .number(4.123456789), "penumbra": .number(0.23456789)]),
                .object(["lightId": .string(other), "fixture": .string("none"), "range": .number(8.987654321)])])])
        }
        try draft.replaceCandidate(candidate); return draft
    }
    func testSharedKelvinVectorsAndRangeRefusal() throws {
        // Values obtained from the unchanged frozen studio-contract implementation.
        let vectors: [(Double, [Double])] = [(1000, [1, 0.05780543019106723, 0]),
            (1900, [1, 0.23074004852434915, 0]), (4500, [1, 0.7011018919329731, 0.4969329950608704]),
            (6500, [1, 0.9911020971138298, 0.9559733532492861]), (6600, [1, 1, 1]),
            (6700, [0.9911020971138298, 0.9473065367331999, 1]), (20000, [0.4072402119017367, 0.5647115057049292, 1])]
        for (kelvin, expected) in vectors {
            for (actual, value) in zip(try LightColorIntent.linearRGB(kelvin: kelvin), expected) { XCTAssertEqual(actual, value, accuracy: 1e-14) }
        }
        for invalid in [999.999, 20000.001, .nan, .infinity] { XCTAssertThrowsError(try LightColorIntent.linearRGB(kelvin: invalid)) }
    }
    func testLoadAndUnrelatedEditPreserveKAndExactStoredRGBThenDirectEditClearsOnlyK() throws {
        let draft = try fixture(kelvin: 4500); var fields = try LightFormBuffer(draft: draft, selectedLightId: first)
        XCTAssertEqual(fields.colorMode, .temperature)
        XCTAssertFalse(fields.detailsChanged)
        XCTAssertEqual(try fields.command().proposedCandidate(in: draft, selectedLightId: first), draft.candidate)
        fields.intensity = "125.5"
        var expected = draft.candidate; updateLight(&expected, index: 0) { $0["intensity"] = .number(125.5) }
        XCTAssertEqual(try fields.command().proposedCandidate(in: draft, selectedLightId: first), expected)
        fields.changeColorMode(.direct)
        let command = try fields.command(), candidate = try command.proposedCandidate(in: draft, selectedLightId: first)
        XCTAssertTrue(command.usesLightDetails)
        XCTAssertEqual(command.colorIntent, .direct)
        var binding = expected["extensions"][namespace]["lights"].items![0]
        if case .object(var values) = binding { values.removeValue(forKey: "temperatureKelvin"); binding = .object(values) }
        var bindings = expected["extensions"][namespace]["lights"].items!
        bindings[0] = binding; expected["extensions"][namespace]["lights"] = .array(bindings)
        XCTAssertEqual(candidate, expected)
        XCTAssertEqual(candidate["lights"].items![0]["color"], draft.candidate["lights"].items![0]["color"])
        try writeProof("direct", base: draft, command: command, expected: candidate)
    }
    func testExplicitKUsesNegotiatedDetailsAndCheckedPreviewWithoutOtherChanges() throws {
        let draft = try fixture(); var fields = try LightFormBuffer(draft: draft, selectedLightId: first)
        fields.changeColorMode(.temperature); fields.kelvin = "6500"
        let command = try fields.command(), candidate = try command.proposedCandidate(in: draft, selectedLightId: first)
        XCTAssertEqual(command.colorIntent, .temperature(6500))
        var expected = draft.candidate; let temperatureColor = try LightColorIntent.linearRGB(kelvin: 6500)
        updateLight(&expected, index: 0) { $0["color"] = .vector(temperatureColor) }
        expected["extensions"][namespace] = .object(["version": .integer(1), "lights": .array([
            .object(["lightId": .string(first), "fixture": .string("none"), "temperatureKelvin": .number(6500)])])])
        XCTAssertEqual(candidate, expected)
        let request = try makeRequest(draft, command)
        XCTAssertEqual(request.value["type"], .string("set-light-details"))
        XCTAssertEqual(request.value["settings"]["temperatureKelvin"], .number(6500))
        var reply = request.value
        if case .object(var values) = reply {
            for key in ["type", "light", "settings"] { values.removeValue(forKey: key) }
            values["candidate"] = candidate; reply = .object(values)
        }
        let preview = try request.checkedReply(reply, selectedLightId: first)
        var applied = try preview.apply(to: draft, selectedLightId: first, command: command)
        XCTAssertEqual(try JSONDecoder().decode(NativeExhibitionDraft.self, from: JSONEncoder().encode(applied)).candidate, candidate)
        try applied.replaceCandidate(draft.candidate); XCTAssertEqual(applied.candidate, draft.candidate)
        var tampered = reply; updateLight(&tampered["candidate"], index: 1) { $0["intensity"] = .integer(999) }
        XCTAssertThrowsError(try request.checkedReply(tampered, selectedLightId: first))
        tampered = reply; tampered["sequence"] = .integer(4)
        XCTAssertThrowsError(try request.checkedReply(tampered, selectedLightId: first))
        XCTAssertThrowsError(try request.checkedReply(reply, selectedLightId: other))
        try writeProof("temperature", base: draft, command: command, expected: candidate)
    }
    func testRawInvalidTemperatureAndFutureMetadataRefuseWithoutLosingLegacyRoute() throws {
        let draft = try fixture(); var fields = try LightFormBuffer(draft: draft, selectedLightId: first)
        fields.red = "0.3"
        XCTAssertFalse(try fields.command().usesLightDetails)
        XCTAssertEqual(try makeRequest(draft, fields.command()).value["type"], .string("set-light"))
        fields.changeColorMode(.temperature)
        for raw in ["-", "nan", "999", "20001"] {
            fields.kelvin = raw; XCTAssertThrowsError(try fields.command()); XCTAssertEqual(fields.kelvin, raw)
        }
        var future = draft, candidate = draft.candidate
        candidate["extensions"][namespace] = .object(["version": .integer(2), "future": .string("keep")])
        try future.replaceCandidate(candidate)
        fields = try LightFormBuffer(draft: future, selectedLightId: first)
        XCTAssertFalse(fields.fixtureEditable)
        fields.intensity = "125.5"
        XCTAssertEqual(try fields.command().proposedCandidate(in: future, selectedLightId: first)["extensions"], candidate["extensions"])
        fields.changeColorMode(.temperature)
        XCTAssertThrowsError(try fields.command())
        XCTAssertEqual(future.candidate, candidate)
    }
    func testColorModeSwitchPreservesResolvedKWhileOtherRawFieldsAreIncomplete() throws {
        let draft = try fixture(); var fields = try LightFormBuffer(draft: draft, selectedLightId: first)
        fields.changeColorMode(.temperature); fields.kelvin = "6500"
        fields.intensity = "-"; fields.beamDegrees = "-"; fields.x = "-"
        let expected = try LightColorIntent.linearRGB(kelvin: 6500)
        XCTAssertEqual(try fields.resolvedColor(), expected)
        fields.changeColorMode(.direct)
        XCTAssertEqual(try fields.resolvedColor(), expected)
        XCTAssertEqual(fields.intensity, "-"); XCTAssertEqual(fields.beamDegrees, "-"); XCTAssertEqual(fields.x, "-")
        XCTAssertThrowsError(try fields.command())
        fields.intensity = "100"; fields.beamDegrees = "45"; fields.x = "0"
        XCTAssertEqual(try fields.command().color, expected)
        fields.red = "-"
        fields.changeColorMode(.temperature); fields.kelvin = "6500"
        XCTAssertEqual(try fields.resolvedColor(), expected)
        XCTAssertEqual(try fields.command().color, expected)
        XCTAssertEqual(fields.red, "-", "Inactive RGB raw input is retained and does not block valid K")
    }
    func updateLight(_ candidate: inout DocumentValue, index: Int, change: (inout DocumentValue) -> Void) {
        var rows = candidate["lights"].items!; change(&rows[index]); candidate["lights"] = .array(rows)
    }
    func makeRequest(_ draft: NativeExhibitionDraft, _ command: LightCommand) throws -> LightViewportRequest {
        try LightViewportRequest(frame: ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 5,
            tenantId: UUID().uuidString.lowercased(), principalId: UUID().uuidString.lowercased(), draft: draft,
            selection: nil, mode: "translate", view: "isometric", disabled: false), command: command, selectedLightId: first)
    }
    func writeProof(_ name: String, base: NativeExhibitionDraft, command: LightCommand, expected: DocumentValue) throws {
        if let directory = ProcessInfo.processInfo.environment["NATIVE_LIGHT_COLOR_PROOF_DIR"] {
            let proof: DocumentValue = .object(["base": base.candidate, "request": try makeRequest(base, command).value, "expected": expected])
            try JSONEncoder().encode(proof).write(to: URL(fileURLWithPath: directory).appendingPathComponent(name + ".json"), options: .atomic)
        }
    }
}
