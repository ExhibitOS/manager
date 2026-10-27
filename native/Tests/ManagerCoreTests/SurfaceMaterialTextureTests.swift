// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class SurfaceMaterialTextureTests: XCTestCase {
    private let surface = "aaaaaaaa-0000-4000-8000-000000000001"
    private let other = "bbbbbbbb-0000-4000-8000-000000000002"
    private let tenant = "cccccccc-0000-4000-8000-000000000003"
    private var selection: ViewportSelection { .init(kind: "surface", id: surface) }
    private let namespace = SurfaceMaterialCommand.namespace

    private func binding(_ id: String = SurfaceMaterialTextureBinding.paint) throws -> SurfaceMaterialTextureBinding {
        let size = try SurfaceMaterialTextureBinding.recommendedRepeat(for: id)
        return try .init(catalogId: id, repeatMeters: [size, size], rotation: 0, normalScale: 1)
    }
    private func fixture() throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        candidate["surfaces"] = .array([surface, other].map {
            .object(["id": .string($0), "roomId": draft.rooms[0]["id"], "type": .string("wall"),
                     "dimensions": .object(["width": .number(4), "height": .number(3)]),
                     "transform": NativeExhibitionDraft.identity])
        })
        candidate["extensions"] = .object(["future-exhibition": .string("keep"), namespace: .object([
            "version": .integer(1), "surfaces": .object([other: try SurfaceMaterial(color: "#abcdef", roughness: 0.3, metalness: 0).value]),
            "surfaceTextures": .object([surface.uppercased(): try binding().value])
        ])])
        try draft.replaceCandidate(candidate)
        return draft
    }

    func testTextureCapabilityRequiresExplicitAcceptedHandshake() throws {
        let decoder = JSONDecoder()
        for json in ["{\"accepted\":true}", "{\"accepted\":true,\"capabilities\":[\"space-settings-v1\"]}",
                     "{\"accepted\":false,\"capabilities\":[\"surface-material-textures-v1\"]}"] {
            XCTAssertFalse(try decoder.decode(ViewportAcceptance.self, from: Data(json.utf8)).supportsSurfaceMaterialTextures)
        }
        let accepted = try decoder.decode(ViewportAcceptance.self, from: Data("{\"accepted\":true,\"capabilities\":[\"surface-material-textures-v1\"]}".utf8))
        XCTAssertTrue(accepted.supportsSurfaceMaterialTextures)
        let base = try fixture(), texture = try binding()
        let command = SurfaceMaterialTextureCommand(surfaceId: surface, material: nil, texture: texture)
        XCTAssertEqual(try NativeGeometryAction.surfaceMaterialTexture(command).encodedValue(in: base, selected: selection),
                       try command.encodedValue(in: base, selected: selection))
    }

    func testNullableCatalogueKeepsRealDefaultsAndRefusesForeignPreviewURLs() throws {
        func item(_ id: String) throws -> DocumentValue {
            .object(["id": .string(id), "version": .integer(1), "name": .string(id),
                     "defaults": .object(["material": try SurfaceMaterial(color: "#ffffff", roughness: 1, metalness: 0).value,
                                          "texture": try binding(id).value]), "preview": .null])
        }
        var value: DocumentValue = .object(["version": .integer(1), "items": .array([
            try item(SurfaceMaterialTextureBinding.paint), try item(SurfaceMaterialTextureBinding.plaster)
        ])])
        let catalog = try NativeSurfaceMaterialCatalog(value: value, tenantId: tenant)
        XCTAssertEqual(catalog.items.count, 2)
        XCTAssertTrue(catalog.items.allSatisfy { $0.preview == nil })
        XCTAssertEqual(catalog.items[1].texture.repeatMeters, [2, 2])
        var rows = value["items"].items!
        rows[0]["preview"] = .object(["url": .string("https://example.com/preview.png"), "mime": .string("image/png"),
                                      "bytes": .integer(1), "sha256": .string(String(repeating: "a", count: 64)),
                                      "width": .integer(640), "height": .integer(640)])
        value["items"] = .array(rows)
        XCTAssertThrowsError(try NativeSurfaceMaterialCatalog(value: value, tenantId: tenant))
        rows[0]["preview"]["url"] = .string("/api/v1/tenants/\(surface)/studio/materials/museum-matte-paint/1/preview")
        value["items"] = .array(rows)
        XCTAssertThrowsError(try NativeSurfaceMaterialCatalog(value: value, tenantId: tenant))
        rows[0]["preview"] = .null
        rows[1]["id"] = .string(SurfaceMaterialTextureBinding.paint)
        value["items"] = .array(rows)
        XCTAssertThrowsError(try NativeSurfaceMaterialCatalog(value: value, tenantId: tenant))
    }

    func testAtomicTexturePreviewRefusesOtherSurfaceMutationAndStaleApply() throws {
        let base = try fixture(), texture = try binding(SurfaceMaterialTextureBinding.plaster)
        let command = SurfaceMaterialTextureCommand(surfaceId: surface, material: nil, texture: texture)
        let action = try command.encodedValue(in: base, selected: selection)
        XCTAssertEqual(action["type"], .string("set-material-texture"))
        XCTAssertEqual(action["material"], .null)
        var candidate = base.candidate
        candidate["extensions"][namespace]["surfaceTextures"] = .object([surface: texture.value])
        let preview = try SurfaceMaterialTexturePreview(candidate: candidate, base: base, command: command, selected: selection)
        XCTAssertEqual(base.candidate["extensions"][namespace]["surfaceTextures"][surface.uppercased()], try binding().value)
        let applied = try preview.apply(to: base, selected: selection, command: command)
        XCTAssertEqual(applied.candidate["extensions"][namespace]["surfaces"], base.candidate["extensions"][namespace]["surfaces"])
        XCTAssertEqual(applied.candidate["extensions"]["future-exhibition"], .string("keep"))
        var forged = candidate
        forged["extensions"][namespace]["surfaces"][other]["color"] = .string("#000000")
        XCTAssertThrowsError(try command.checkProposal(forged, base: base, selected: selection))
        XCTAssertThrowsError(try preview.apply(to: applied, selected: selection, command: command))
        XCTAssertThrowsError(try preview.apply(to: base, selected: .init(kind: "surface", id: other), command: command))
        XCTAssertThrowsError(try command.encodedValue(in: base, selected: nil))
    }

    func testTextureRemovalOmitsEmptyMapAndLegacyScalarPreservesIt() throws {
        var base = try fixture(), candidate = base.candidate
        let removal = SurfaceMaterialTextureCommand(surfaceId: surface, material: nil, texture: nil)
        if case .object(var fields) = candidate["extensions"][namespace] {
            fields.removeValue(forKey: "surfaceTextures")
            candidate["extensions"][namespace] = .object(fields)
        }
        XCTAssertNoThrow(try removal.checkProposal(candidate, base: base, selected: selection))
        var inventedEmpty = candidate
        inventedEmpty["extensions"][namespace]["surfaceTextures"] = .object([:])
        XCTAssertThrowsError(try removal.checkProposal(inventedEmpty, base: base, selected: selection))
        let scalar = SurfaceMaterialCommand(surfaceId: surface, material: nil)
        XCTAssertNoThrow(try scalar.checkProposal(base.candidate, base: base, selected: selection))
        var stripped = base.candidate
        stripped["extensions"][namespace] = candidate["extensions"][namespace]
        XCTAssertThrowsError(try scalar.checkProposal(stripped, base: base, selected: selection))
        try base.replaceCandidate(inventedEmpty)
        XCTAssertNoThrow(try scalar.checkProposal(base.candidate, base: base, selected: selection))
    }

    func testTexturedDefaultMatchesSourceAndUnionLimitCannotBeSplitAcrossMaps() throws {
        var base = try fixture()
        let fields = try SurfaceMaterialFormBuffer(draft: base, selection: selection)
        XCTAssertFalse(fields.usesOverride)
        XCTAssertEqual(fields.color, "#ffffff")
        XCTAssertEqual(fields.roughness, "1.0")
        var candidate = base.candidate
        var surfaces = candidate["surfaces"].items!
        var scalars: [String: DocumentValue] = [other: try SurfaceMaterial(color: "#abcdef", roughness: 1, metalness: 0).value]
        for index in 0..<63 {
            let id = String(format: "dddddddd-0000-4000-8000-%012d", index)
            var added = surfaces[0]; added["id"] = .string(id); surfaces.append(added)
            scalars[id] = try SurfaceMaterial(color: "#ffffff", roughness: 1, metalness: 0).value
        }
        candidate["surfaces"] = .array(surfaces)
        candidate["extensions"][namespace]["surfaces"] = .object(scalars)
        try base.replaceCandidate(candidate)
        XCTAssertThrowsError(try SurfaceMaterialFormBuffer(draft: base, selection: selection))
        XCTAssertThrowsError(try SurfaceMaterialTextureCommand(surfaceId: surface, material: nil, texture: binding()).encodedValue(in: base, selected: selection))
    }

    func testClosedPackAndNumericBoundaryRefusePathsAndNonFiniteControls() throws {
        let valid = try SurfaceMaterialTextureBinding(catalogId: SurfaceMaterialTextureBinding.paint, repeatMeters: [1, 1], rotation: 0, normalScale: 1)
        for field in ["path", "url", "height", "ao"] {
            var value = valid.value; value[field] = .string("not-a-supported-role")
            XCTAssertThrowsError(try SurfaceMaterialTextureBinding(value: value))
        }
        XCTAssertThrowsError(try SurfaceMaterialTextureBinding(catalogId: "../maps/BaseColor.png", repeatMeters: [1, 1], rotation: 0, normalScale: 1))
        XCTAssertThrowsError(try SurfaceMaterialTextureBinding(catalogId: SurfaceMaterialTextureBinding.paint, catalogVersion: 2, repeatMeters: [1, 1], rotation: 0, normalScale: 1))
        for repeats in [[0.049, 1], [1, 20.001], [Double.nan, 1], [Double.infinity, 1], [1]] {
            XCTAssertThrowsError(try SurfaceMaterialTextureBinding(catalogId: SurfaceMaterialTextureBinding.paint, repeatMeters: repeats, rotation: 0, normalScale: 1))
        }
        XCTAssertThrowsError(try SurfaceMaterialTextureBinding(catalogId: SurfaceMaterialTextureBinding.paint, repeatMeters: [1, 1], rotation: .infinity, normalScale: 1))
        XCTAssertThrowsError(try SurfaceMaterialTextureBinding(catalogId: SurfaceMaterialTextureBinding.paint, repeatMeters: [1, 1], rotation: 0, normalScale: 2.001))
    }

    func testPackSelectionPreservesIncompleteAndAdvancedText() throws {
        let binding = try SurfaceMaterialTextureBinding(catalogId: SurfaceMaterialTextureBinding.paint, repeatMeters: [3.5, 4], rotation: 0.27, normalScale: 0.4)
        var fields = SurfaceMaterialTextureFormBuffer(binding: binding)
        fields.repeatU = "-"; fields.rotationDegrees = "12.30"; fields.normalScale = "0.40"
        try fields.select(SurfaceMaterialTextureBinding.plaster)
        XCTAssertEqual(fields.repeatU, "-"); XCTAssertEqual(fields.repeatV, "4.0")
        XCTAssertEqual(fields.rotationDegrees, "12.30"); XCTAssertEqual(fields.normalScale, "0.40")
        XCTAssertThrowsError(try fields.binding())
        let before = fields
        XCTAssertThrowsError(try fields.select("unqualified-material"))
        XCTAssertEqual(fields, before)
    }

    func testUnchangedAuthoredAngleIsNotRoundedThroughDegrees() throws {
        let original = try SurfaceMaterialTextureBinding(catalogId: SurfaceMaterialTextureBinding.paint, repeatMeters: [0.05, 20], rotation: 0.12345678901234567, normalScale: 2)
        var fields = SurfaceMaterialTextureFormBuffer(binding: original)
        XCTAssertEqual(try fields.binding(), original)
        try fields.select(SurfaceMaterialTextureBinding.plaster)
        XCTAssertEqual(try fields.binding()?.rotation, original.rotation)
        fields.rotationDegrees = "-180"
        XCTAssertEqual(try fields.binding()?.rotation, -Double.pi)
    }
}
