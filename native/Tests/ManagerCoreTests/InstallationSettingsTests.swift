// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class InstallationSettingsTests: XCTestCase {
    private let first = "12345678-1234-1234-1234-123456789abc"
    private let second = "12345678-1234-1234-1234-123456789def"
    private let opaque: DocumentValue = .object(["literal": .string("12345678-1234-1234-1234-123456789abc"), "large": .integer(9_007_199_254_740_991)])
    func testVerifiedPlinthCommandRequiresSelectedFrameAndIsSeparatelyNegotiated() throws {
        let base = try draft(), selection = ViewportSelection(kind: "placement", id: first)
        let command = InstallationCommand.alignOnVerifiedPlinth(placementId: first)
        XCTAssertEqual(command.translationAxes, [0, 1, 2])
        XCTAssertEqual(try NativeGeometryAction.installation(command).encodedValue(in: base, selected: selection), .object(["type": .string("installation-command"), "placementId": .string(first), "command": .string("align-on-verified-plinth")]))
        XCTAssertThrowsError(try NativeGeometryAction.installation(command).encodedValue(in: base, selected: .init(kind: "placement", id: second)))
        XCTAssertThrowsError(try NativeGeometryAction.installation(command).encodedValue(in: base, selected: nil))
        let old = try JSONDecoder().decode(ViewportAcceptance.self, from: Data("{\"accepted\":true,\"capabilities\":[\"furnishing-visuals-v2\"]}".utf8))
        let current = try JSONDecoder().decode(ViewportAcceptance.self, from: Data("{\"accepted\":true,\"capabilities\":[\"plinth-envelope-align-v1\"]}".utf8))
        XCTAssertFalse(old.supportsPlinthEnvelopeAlign); XCTAssertTrue(current.supportsPlinthEnvelopeAlign)
        var altered = base.candidate, rows = try XCTUnwrap(altered["placements"].items)
        rows[0]["transform"]["position"] = .vector([4, 5, 6]); altered["placements"] = .array(rows)
        var accepted = base; try accepted.acceptInstallationTranslation(altered, placementId: first, axes: command.translationAxes)
        XCTAssertEqual(accepted.candidate["placements"].items?[0]["transform"]["position"], .vector([4, 5, 6]))
        altered["title"] = .string("unexpected")
        XCTAssertThrowsError(try accepted.acceptInstallationTranslation(altered, placementId: first, axes: command.translationAxes))
        XCTAssertEqual(InstallationCommandReason.fromJavaScriptMessage("Error: INSTALLATION_PLINTH_OVERHANG"), .plinthOverhang)
        XCTAssertEqual(InstallationCommandReason.fromJavaScriptMessage("Error: INSTALLATION_PLINTH_UNSUPPORTED"), .plinthUnsupported)
    }
    private func draft() throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(title: "설치 설정")
        var value = draft.candidate
        value["placements"] = .array([first, second].map { id in
            .object(["id": .string(id), "roomId": draft.rooms[0]["id"], "opaque": opaque,
                     "transform": .object(["position": .vector([1, 2, 3]), "rotation": .vector([0, 0, 0]), "scale": .vector([1, 1, 1])])])
        })
        value["extensions"] = .object(["org.example/future": opaque])
        value["rights"] = opaque
        try draft.replaceCandidate(value)
        return draft
    }
    func testExplicitBasePointZeroFactorsAndOpaquePreservation() throws {
        var draft = try draft()
        XCTAssertNil(try draft.installationSettings(for: first))
        let before = draft
        var settings = InstallationSettings(pedestal: .init(kind: .box))
        XCTAssertThrowsError(try draft.setInstallation(settings, for: first))
        XCTAssertEqual(draft, before) // Never invent a floor point.
        settings.pedestal!.position = [0, 0, 0]
        settings.pedestal!.material.roughness = 0
        settings.pedestal!.material.metalness = 0
        try draft.setInstallation(settings, for: first.uppercased())
        XCTAssertEqual(try draft.installationSettings(for: first), settings)
        XCTAssertEqual(draft.candidate["placements"], before.candidate["placements"])
        XCTAssertEqual(draft.candidate["rights"], opaque)
        XCTAssertEqual(draft.candidate["extensions"]["org.example/future"], opaque)
        let saved = draft
        try draft.setInstallation(settings, for: first)
        XCTAssertEqual(draft, saved) // Reapplying does not create an undo step.
        let reopened = try JSONDecoder().decode(NativeExhibitionDraft.self, from: JSONEncoder().encode(draft))
        XCTAssertEqual(reopened, draft)
        try draft.setInstallation(nil, for: first)
        XCTAssertNil(try draft.installationSettings(for: first))
        XCTAssertEqual(draft.candidate["extensions"]["org.example/future"], opaque)
    }
    func testStrictNestedSchemaAndCaseInsensitiveDuplicateReferences() throws {
        var valid = try draft()
        try valid.setInstallation(.init(frame: .init(kind: .floating, gap: 0.2)), for: first)
        let path = InstallationSettings.namespace
        let rows = valid.candidate["extensions"][path]["placements"].items!
        var variants: [DocumentValue] = []
        var unknown = rows[0]; unknown["frame"]["future"] = .bool(true)
        variants.append(.array([unknown]))
        var wrongMaterial = rows[0]; wrongMaterial["frame"]["material"]["preset"] = .string("stone")
        variants.append(.array([wrongMaterial]))
        var invalidGap = rows[0]; invalidGap["frame"]["kind"] = .string("flat")
        variants.append(.array([invalidGap]))
        var duplicate = rows[0]; duplicate["placementId"] = .string(first.uppercased())
        variants.append(.array(rows + [duplicate]))
        var dangling = rows[0]; dangling["placementId"] = .string(UUID().uuidString)
        variants.append(.array([dangling]))
        for rows in variants {
            var malformed = valid, candidate = valid.candidate
            candidate["extensions"][path]["placements"] = rows
            try malformed.replaceCandidate(candidate)
            let before = malformed
            XCTAssertThrowsError(try malformed.installationSettings(for: first))
            XCTAssertThrowsError(try malformed.setInstallation(nil, for: first))
            XCTAssertEqual(malformed, before)
        }
    }
    func testTypedRemapUsesDestinationCasingAndLeavesOpaqueStrings() throws {
        var source = try draft()
        try source.setInstallation(.init(pedestal: .init(kind: .cylinder, position: [1, 0, 2])), for: first)
        var candidate = source.candidate
        var placement = candidate["placements"].items![0]
        placement["id"] = .string(second.uppercased())
        candidate["placements"] = .array([placement])
        let mapped = try InstallationSettings.remappingPlacementReferences(in: candidate, using: [first.uppercased(): second])
        XCTAssertEqual(mapped["extensions"][InstallationSettings.namespace]["placements"].items![0]["placementId"].string, second.uppercased())
        XCTAssertEqual(mapped["extensions"]["org.example/future"], opaque)
        XCTAssertEqual(mapped["placements"].items![0]["opaque"], opaque)
        XCTAssertThrowsError(try InstallationSettings.remappingPlacementReferences(in: candidate, using: [:]))
        let identity = try InstallationSettings.remappingPlacementReferences(in: source.candidate)
        XCTAssertEqual(identity, source.candidate)
        let copy = try source.copyForPublication()
        XCTAssertNotEqual(copy.id, source.id)
        XCTAssertEqual(copy.placements, source.placements)
        XCTAssertEqual(copy.candidate["extensions"], source.candidate["extensions"])
    }
    func testAlignmentAcceptsOnlySelectedTranslationAndAllowedAxes() throws {
        var draft = try draft(), next = draft.candidate
        var rows = next["placements"].items!
        rows[0]["transform"]["position"] = .vector([1, 4, 3])
        next["placements"] = .array(rows)
        try draft.acceptInstallationTranslation(next, placementId: first, axes: [1])
        let accepted = draft
        for change in 0..<4 {
            var invalid = accepted.candidate, placements = invalid["placements"].items!
            switch change {
            case 0: placements[0]["transform"]["position"] = .vector([2, 4, 3])
            case 1: placements[0]["transform"]["rotation"] = .vector([0, 1, 0])
            case 2: placements[1]["transform"]["position"] = .vector([1, 5, 3])
            default: invalid["extensions"]["org.example/future"] = .null
            }
            invalid["placements"] = .array(placements)
            XCTAssertThrowsError(try draft.acceptInstallationTranslation(invalid, placementId: first, axes: [1]))
            XCTAssertEqual(draft, accepted)
        }
        try draft.acceptInstallationTranslation(accepted.candidate, placementId: first, axes: [1])
        XCTAssertEqual(draft, accepted)
    }
    func testPartialTextStaysBufferedAndUnchangedYawRetainsExactValue() throws {
        let original = InstallationSettings(pedestal: .init(kind: .box, position: [0, 0, 0], yaw: 0.123456789012345))
        var buffer = InstallationFormBuffer(settings: original)
        XCTAssertEqual(try buffer.value(), original)
        buffer.fields[.width] = "-"
        XCTAssertThrowsError(try buffer.value())
        XCTAssertEqual(buffer.fields[.width], "-")
        buffer.fields[.width] = "1,25"
        let parsed = try XCTUnwrap(buffer.value(locale: Locale(identifier: "de_DE")))
        XCTAssertEqual(parsed.pedestal?.width, 1.25)
        XCTAssertEqual(parsed.pedestal?.yaw, original.pedestal?.yaw)
        var fresh = InstallationFormBuffer(settings: nil)
        XCTAssertNil(try fresh.value())
        fresh.pedestalKind = .box
        XCTAssertThrowsError(try fresh.value())
        XCTAssertEqual(fresh.fields[.x], "")
        fresh.fields[.x] = "0"; fresh.fields[.y] = "0"; fresh.fields[.z] = "0"
        XCTAssertNotNil(try fresh.value())
    }
    func testClosedCommandShapeAndBounds() throws {
        let center = InstallationCommand.centerArtwork(placementId: first)
        XCTAssertEqual(center.translationAxes, [0, 2])
        XCTAssertEqual(try center.encodedValue(), .object(["type": .string("installation-command"), "placementId": .string(first), "command": .string("center-artwork")]))
        let height = try InstallationCommand.installHeight(placementId: first, height: 0, floorSurfaceId: nil).encodedValue()
        guard case .object(let values) = height else { return XCTFail("command object") }
        XCTAssertNil(values["floorSurfaceId"])
        XCTAssertEqual(values["height"]?.double, 0)
        XCTAssertThrowsError(try InstallationCommand.installHeight(placementId: first, height: .infinity, floorSurfaceId: nil).encodedValue())
        XCTAssertThrowsError(try InstallationCommand.wallGap(placementId: first, gap: 0.1, wallSurfaceId: "").encodedValue())
        XCTAssertThrowsError(try InstallationCommand.wallGap(placementId: first, gap: 1.01, wallSurfaceId: second).encodedValue())
        for reason in InstallationCommandReason.allCases {
            XCTAssertEqual(InstallationCommandReason.fromJavaScriptMessage("Error: " + reason.rawValue), reason)
            XCTAssertFalse(reason.explanation.isEmpty)
        }
        XCTAssertNil(InstallationCommandReason.fromJavaScriptMessage("INSTALLATION_source-not-planar"))
        XCTAssertNil(InstallationCommandReason.fromJavaScriptMessage("arbitrary INSTALLATION_SOURCE_NOT_PLANAR"))
        XCTAssertNil(InstallationCommandReason.fromJavaScriptMessage("Error: INSTALLATION_FUTURE_REASON"))
    }
}
