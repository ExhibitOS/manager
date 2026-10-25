// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class SurfaceMaterialTests: XCTestCase {
    let first = "aaaaaaaa-0000-4000-8000-000000000001"
    let other = "bbbbbbbb-0000-4000-8000-000000000002"
    let namespace = SurfaceMaterialCommand.namespace
    let template = "org.exhibitos.studio/template"
    var selected: ViewportSelection { .init(kind: "surface", id: first) }
    func fixture(materials: Bool = true) throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        let room = draft.rooms[0]["id"]
        candidate["surfaces"] = .array([first, other].map { id in
            .object(["id": .string(id), "roomId": room, "type": .string("wall"), "dimensions": .object(["width": .number(4), "height": .number(3)]), "transform": NativeExhibitionDraft.identity, "extensions": .object(["future-surface": .string("keep")])])
        })
        candidate["extensions"] = .object(["future-exhibition": .object(["opaque": .array([.string("keep"), .integer(7)])]), template: .object(["modified": .bool(false), "license": .string("CC0-1.0")])])
        if materials {
            candidate["extensions"][namespace] = .object(["version": .integer(1), "surfaces": .object([
                first.uppercased(): try SurfaceMaterial(color: "#123456", roughness: 0.2, metalness: 0.3).value,
                other: try SurfaceMaterial(color: "#abcdef", roughness: 1, metalness: 0).value
            ])])
        }
        try draft.replaceCandidate(candidate); return draft
    }
    func proposal(_ base: NativeExhibitionDraft, material: SurfaceMaterial?) -> DocumentValue {
        var value = base.candidate
        var entries = value["extensions"][namespace]["surfaces"]
        if entries == .null { entries = .object([:]) }
        if case .object(var map) = entries {
            map.removeValue(forKey: first.uppercased())
            map.removeValue(forKey: first)
            if let material { map[first] = material.value }
            value["extensions"][namespace] = .object(["version": .integer(1), "surfaces": .object(map)])
        }
        value["extensions"][template]["modified"] = .bool(true)
        return value
    }
    func testPreviewIsTransientAndApplyPreservesUnrelatedSettings() throws {
        let base = try fixture(), material = try SurfaceMaterial(color: "#334455", roughness: 0.5, metalness: 0)
        let command = SurfaceMaterialCommand(surfaceId: first, material: material)
        let action = try NativeGeometryAction.surfaceMaterial(command).encodedValue(in: base, selected: selected)
        XCTAssertEqual(action, .object(["type": .string("set-material"), "surfaceId": .string(first), "material": material.value]))
        let candidate = proposal(base, material: material)
        let preview = try SurfaceMaterialPreview(candidate: candidate, base: base, command: command, selected: selected)
        XCTAssertEqual(preview.displayDraft.candidate, candidate)
        XCTAssertEqual(base.candidate["extensions"][template]["modified"], .bool(false))
        let applied = try preview.apply(to: base, selected: selected, command: command)
        XCTAssertEqual(applied.candidate, candidate)
        XCTAssertEqual(applied.document["editVersion"].integer, base.document["editVersion"].integer! + 1)
        for key in ["surfaces", "rooms", "openings", "artworks", "placements", "accessibility"] { XCTAssertEqual(applied.candidate[key], base.candidate[key]) }
        XCTAssertEqual(applied.candidate["extensions"]["future-exhibition"], base.candidate["extensions"]["future-exhibition"])
        XCTAssertEqual(applied.candidate["extensions"][namespace]["surfaces"][other], base.candidate["extensions"][namespace]["surfaces"][other])
        XCTAssertEqual(applied.candidate["extensions"][namespace]["surfaces"][first.uppercased()], .null)
        let roundTrip = try JSONDecoder().decode(NativeExhibitionDraft.self, from: JSONEncoder().encode(applied))
        XCTAssertEqual(roundTrip, applied)
    }
    func testNullResetRemovesOnlySelectedOverrideAndLegacyDoesNotGainSettingsOnRead() throws {
        let base = try fixture(), command = SurfaceMaterialCommand(surfaceId: first, material: nil)
        let candidate = proposal(base, material: nil)
        try command.checkProposal(candidate, base: base, selected: selected)
        XCTAssertEqual(candidate["extensions"][namespace]["surfaces"][first], .null)
        XCTAssertEqual(candidate["extensions"][namespace]["surfaces"][other], base.candidate["extensions"][namespace]["surfaces"][other])
        XCTAssertEqual(try command.encodedValue(in: base, selected: selected)["material"], .null)
        let legacy = try fixture(materials: false), fields = try SurfaceMaterialFormBuffer(draft: legacy, selection: selected)
        XCTAssertFalse(fields.usesOverride)
        XCTAssertEqual(fields.color, "#f4f1e8"); XCTAssertEqual(fields.roughness, "0.8"); XCTAssertEqual(fields.metalness, "0.0")
        XCTAssertEqual(legacy.candidate["extensions"][namespace], .null)
    }
    func testStaleBaseSelectionAndChangedInputCannotApplyPreview() throws {
        let base = try fixture(), command = SurfaceMaterialCommand(surfaceId: first, material: nil)
        let preview = try SurfaceMaterialPreview(candidate: proposal(base, material: nil), base: base, command: command, selected: selected)
        var changed = base; try changed.rename("changed")
        XCTAssertThrowsError(try preview.apply(to: changed, selected: selected, command: command))
        XCTAssertThrowsError(try preview.apply(to: base, selected: .init(kind: "surface", id: other), command: command))
        XCTAssertThrowsError(try preview.apply(to: base, selected: selected, command: .init(surfaceId: first, material: try .init(color: "#ffffff", roughness: 0, metalness: 0))))
        XCTAssertThrowsError(try command.encodedValue(in: base, selected: nil))
        XCTAssertThrowsError(try command.encodedValue(in: base, selected: .init(kind: "room", id: first)))
        XCTAssertThrowsError(try NativeGeometryAction.surfaceMaterial(command).encodedValue())
    }
    func testUnrelatedProposalChangesAndUnsupportedKnownMetadataAreRefused() throws {
        let base = try fixture(), command = SurfaceMaterialCommand(surfaceId: first, material: nil)
        var candidate = proposal(base, material: nil); candidate["title"] = .string("changed")
        XCTAssertThrowsError(try command.checkProposal(candidate, base: base, selected: selected))
        candidate = proposal(base, material: nil); candidate["extensions"]["future-exhibition"] = .null
        XCTAssertThrowsError(try command.checkProposal(candidate, base: base, selected: selected))
        candidate = proposal(base, material: nil); candidate["extensions"][template]["license"] = .string("changed")
        XCTAssertThrowsError(try command.checkProposal(candidate, base: base, selected: selected))
        var unsupported = base, raw = base.candidate
        raw["extensions"][namespace]["future-setting"] = .string("must not disappear")
        try unsupported.replaceCandidate(raw)
        XCTAssertThrowsError(try SurfaceMaterialFormBuffer(draft: unsupported, selection: selected))
        XCTAssertThrowsError(try command.encodedValue(in: unsupported, selected: selected))
        XCTAssertEqual(unsupported.candidate["extensions"][namespace]["future-setting"], .string("must not disappear"))
        raw = base.candidate; raw["extensions"][namespace]["surfaces"][first] = raw["extensions"][namespace]["surfaces"][first.uppercased()]
        try unsupported.replaceCandidate(raw)
        XCTAssertThrowsError(try SurfaceMaterialFormBuffer(draft: unsupported, selection: selected))
    }
    func testIncompleteTextAndOutOfRangePBRValuesRemainInBuffer() throws {
        let base = try fixture()
        var fields = try SurfaceMaterialFormBuffer(draft: base, selection: selected)
        fields.roughness = "unfinished"
        XCTAssertThrowsError(try fields.command()); XCTAssertEqual(fields.roughness, "unfinished")
        fields.roughness = "0.5"; fields.color = "#AABBCC"
        XCTAssertEqual(try fields.command().material?.color, "#aabbcc")
        for text in ["-0.01", "1.01", "nan", "inf"] {
            fields.roughness = text
            XCTAssertThrowsError(try fields.command()); XCTAssertEqual(fields.roughness, text)
        }
        XCTAssertThrowsError(try SurfaceMaterial(color: "#12345678", roughness: 0, metalness: 0))
        XCTAssertThrowsError(try SurfaceMaterial(color: "#ffffff", roughness: 0, metalness: .infinity))
        XCTAssertEqual(try SurfaceMaterialFormBuffer(draft: base, selection: selected).color, "#123456")
    }
}
