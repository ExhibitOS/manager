// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class LightSettingsTests: XCTestCase {
    let first = "aaaaaaaa-0000-4000-8000-000000000001"
    let other = "bbbbbbbb-0000-4000-8000-000000000002"
    let target = "cccccccc-0000-4000-8000-000000000003"
    let template = "org.exhibitos.studio/template"
    func fixture(type: NativeLightType = .spot) throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        var pose = NativeExhibitionDraft.identity
        pose["position"] = .vector([1, 3, -2]); pose["rotation"] = .vector([0, 0.6, 0, 0.8])
        pose["future-pose"] = .string("keep")
        var row: DocumentValue = .object([
            "id": .string(first), "roomId": draft.rooms[0]["id"], "type": .string(type.rawValue),
            "color": .vector([0.0123456789, 0.3, 1]), "intensity": .integer(450), "unit": .string(type.unit),
            "transform": pose, "targetPlacementId": .string(target), "castsShadow": .bool(true),
            "future-light": .object(["opaque": .array([.string("retain"), .integer(19)])])
        ])
        if type == .spot { row["beamAngle"] = .number(0.6123456789012345) }
        if type == .area { row["dimensions"] = .object(["width": .number(1.25), "height": .number(0.75), "future-size": .string("keep")]) }
        var second = row; second["id"] = .string(other)
        candidate["lights"] = .array([row, second])
        candidate["extensions"] = .object([template: .object(["modified": .bool(false), "license": .string("CC0-1.0")]), "future-exhibition": .integer(37)])
        try draft.replaceCandidate(candidate); return draft
    }
    func testUnchangedInputPreservesEveryStoredLightFieldAndRadianBits() throws {
        let draft = try fixture(), fields = try LightFormBuffer(draft: draft, selectedLightId: first)
        let command = try fields.command(), candidate = try command.proposedCandidate(in: draft, selectedLightId: first)
        XCTAssertEqual(command.beamAngle, draft.candidate["lights"].items![0]["beamAngle"].double)
        XCTAssertEqual(candidate["lights"], draft.candidate["lights"])
        XCTAssertEqual(candidate["extensions"][template]["modified"], .bool(true))
        XCTAssertFalse(ViewportSelection(kind: "light", id: first).belongs(to: draft), "Base strict-v1 selection must stay closed")
        XCTAssertEqual(draft.document["editVersion"], .integer(2), "Form/proposal never mutates original")
    }
    func testEditedLightPreviewApplyAndUndoKeepOpaqueFieldsAndOtherLights() throws {
        let draft = try fixture()
        var fields = try LightFormBuffer(draft: draft, selectedLightId: first)
        fields.red = "0.7"; fields.intensity = "700"; fields.x = "2.25"; fields.beamDegrees = "60"
        let command = try fields.command(), candidate = try command.proposedCandidate(in: draft, selectedLightId: first)
        let light = candidate["lights"].items![0], original = draft.candidate["lights"].items![0]
        XCTAssertEqual(light["color"], .vector([0.7, 0.3, 1]))
        XCTAssertEqual(light["intensity"], .number(700)); XCTAssertEqual(light["beamAngle"].double!, .pi / 3, accuracy: 1e-15)
        XCTAssertEqual(light["transform"]["position"], .vector([2.25, 3, -2]))
        for key in ["roomId", "targetPlacementId", "castsShadow", "future-light"] { XCTAssertEqual(light[key], original[key]) }
        for key in ["rotation", "scale", "future-pose"] { XCTAssertEqual(light["transform"][key], original["transform"][key]) }
        XCTAssertEqual(candidate["lights"].items![1], draft.candidate["lights"].items![1])
        XCTAssertEqual(candidate["extensions"]["future-exhibition"], .integer(37))
        let preview = try LightPreview(candidate: candidate, base: draft, command: command, selectedLightId: first)
        XCTAssertEqual(draft.candidate["lights"].items![0]["intensity"], .integer(450))
        var applied = try preview.apply(to: draft, selectedLightId: first, command: command)
        XCTAssertEqual(applied.document["editVersion"], .integer(3))
        try applied.replaceCandidate(draft.candidate)
        XCTAssertEqual(applied.candidate, draft.candidate, "Existing undo candidate path restores all fields")
        let reopened = try JSONDecoder().decode(NativeExhibitionDraft.self, from: JSONEncoder().encode(preview.displayDraft))
        XCTAssertEqual(reopened.candidate, candidate)
    }
    func testTypeUnitAndSupportedParametersFollowExistingProducer() throws {
        let draft = try fixture(type: .point)
        var fields = try LightFormBuffer(draft: draft, selectedLightId: first)
        fields.changeType(.spot)
        XCTAssertEqual(fields.intensity, "450.0", "Point/spot share candela; don't invent a conversion")
        XCTAssertEqual(try fields.command().beamAngle!, .pi / 4, accuracy: 1e-15)
        fields.changeType(.area)
        XCTAssertEqual(fields.intensity, "100.0", "Different units use explicit existing producer default, never a conversion")
        fields.width = "2"; fields.height = "0.4"
        var row = try fields.command().proposedCandidate(in: draft, selectedLightId: first)["lights"].items![0]
        XCTAssertEqual(row["unit"], .string("lumen")); XCTAssertEqual(row["beamAngle"], .null)
        XCTAssertEqual(row["dimensions"]["width"], .number(2)); XCTAssertEqual(row["dimensions"]["height"], .number(0.4))
        fields.changeType(.directional)
        row = try fields.command().proposedCandidate(in: draft, selectedLightId: first)["lights"].items![0]
        XCTAssertEqual(row["unit"], .string("lux")); XCTAssertEqual(row["dimensions"], .null)
        let area = try fixture(type: .area)
        var areaFields = try LightFormBuffer(draft: area, selectedLightId: first)
        areaFields.width = "3"
        row = try areaFields.command().proposedCandidate(in: area, selectedLightId: first)["lights"].items![0]
        XCTAssertEqual(row["dimensions"]["future-size"], .string("keep"))
        areaFields.changeType(.point)
        XCTAssertThrowsError(try areaFields.command().proposedCandidate(in: area, selectedLightId: first), "Don't discard opaque dimensions on type switch")
    }
    func testIncompleteInvalidNumbersStayInBufferAndCannotProduceCandidate() throws {
        let draft = try fixture()
        let original = try LightFormBuffer(draft: draft, selectedLightId: first)
        for key in [\LightFormBuffer.red, \.green, \.blue, \.intensity, \.x, \.y, \.z, \.beamDegrees] {
            for text in ["-", "nan", "inf", "1e300"] {
                var fields = original; fields[keyPath: key] = text
                XCTAssertThrowsError(try fields.command().proposedCandidate(in: draft, selectedLightId: first))
                XCTAssertEqual(fields[keyPath: key], text)
            }
        }
        for degrees in ["0", "-1", "90.001"] {
            var fields = original; fields.beamDegrees = degrees
            XCTAssertThrowsError(try fields.command().proposedCandidate(in: draft, selectedLightId: first))
        }
        let area = try fixture(type: .area)
        for key in [\LightFormBuffer.width, \.height] {
            for text in ["0.009", "100.01", "nan"] {
                var fields = try LightFormBuffer(draft: area, selectedLightId: first); fields[keyPath: key] = text
                XCTAssertThrowsError(try fields.command().proposedCandidate(in: area, selectedLightId: first))
            }
        }
    }
    func testStaleSelectionBaseAndUnrelatedChangesAreRejected() throws {
        let draft = try fixture(), fields = try LightFormBuffer(draft: draft, selectedLightId: first), command = try fields.command()
        let candidate = try command.proposedCandidate(in: draft, selectedLightId: first)
        let preview = try LightPreview(candidate: candidate, base: draft, command: command, selectedLightId: first)
        for selected in [nil, other, first.uppercased()] {
            XCTAssertThrowsError(try command.proposedCandidate(in: draft, selectedLightId: selected))
            XCTAssertThrowsError(try preview.apply(to: draft, selectedLightId: selected, command: command))
        }
        var changed = draft; try changed.rename("Changed after preview")
        XCTAssertThrowsError(try preview.apply(to: changed, selectedLightId: first, command: command))
        var changedFields = fields; changedFields.intensity = "5"
        XCTAssertThrowsError(try preview.apply(to: draft, selectedLightId: first, command: changedFields.command()))
        var altered = candidate; altered["title"] = .string("unrelated")
        XCTAssertThrowsError(try LightPreview(candidate: altered, base: draft, command: command, selectedLightId: first))
        altered = candidate; var rows = altered["lights"].items!; rows[1]["intensity"] = .integer(1); altered["lights"] = .array(rows)
        XCTAssertThrowsError(try command.checkProposal(altered, base: draft, selectedLightId: first))
        altered = candidate; rows = altered["lights"].items!; rows[0]["targetPlacementId"] = .null; altered["lights"] = .array(rows)
        XCTAssertThrowsError(try command.checkProposal(altered, base: draft, selectedLightId: first))
    }
    func testUnsupportedExistingParametersAndAmbiguousIdsArePreservedAndRefused() throws {
        let draft = try fixture()
        for key in ["beamAngle", "unit", "roomId"] {
            var changed = draft, candidate = draft.candidate, rows = candidate["lights"].items!
            if case .object(var map) = rows[0] { map.removeValue(forKey: key); rows[0] = .object(map) }
            candidate["lights"] = .array(rows); try changed.replaceCandidate(candidate)
            XCTAssertThrowsError(try LightFormBuffer(draft: changed, selectedLightId: first))
            XCTAssertEqual(changed.candidate, candidate)
        }
        var changed = draft, candidate = draft.candidate, rows = candidate["lights"].items!
        var duplicate = rows[0]; duplicate["id"] = .string(first.uppercased()); rows.append(duplicate)
        candidate["lights"] = .array(rows); try changed.replaceCandidate(candidate)
        XCTAssertThrowsError(try LightFormBuffer(draft: changed, selectedLightId: first))
    }
}
