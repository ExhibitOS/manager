// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class BenchSettingsTests: XCTestCase {
    func newFields(_ draft: NativeExhibitionDraft) throws -> BenchFormBuffer {
        try BenchFormBuffer(roomId: XCTUnwrap(draft.rooms.first?["id"].string))
    }
    func add(_ fields: BenchFormBuffer, to draft: NativeExhibitionDraft) throws -> NativeExhibitionDraft {
        let command = BenchCommand.add(try fields.settings(), viewpoint: try fields.newViewpoint())
        return try BenchPreview(candidate: command.proposedCandidate(in: draft, selectedBenchId: fields.id), base: draft, command: command, selectedBenchId: fields.id)
            .apply(to: draft, selectedBenchId: fields.id, command: command)
    }
    func fixture() throws -> (NativeExhibitionDraft, String, String) {
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        candidate["extensions"] = .object(["future-private": .object(["opaque": .string("retain")]),
                                            "org.exhibitos.studio/template": .object(["modified": .bool(false)])])
        candidate["mediaAssets"] = .array([.object(["fixture": .string("synthetic-original-reference")])])
        try draft.replaceCandidate(candidate)
        var fields = try newFields(draft); fields.kind = .backed; fields.yawDegrees = "19.123456789"
        try fields.matchNewViewpointToBench()
        draft = try add(fields, to: draft)
        candidate = draft.candidate
        candidate["extensions"][BenchSettings.presentationNamespace]["credits"] = .string("Keep credits")
        var start = try XCTUnwrap(fields.newViewpoint()).value
        if case .object(var row) = start { row.removeValue(forKey: "id"); row.removeValue(forKey: "name"); start = .object(row) }
        candidate["extensions"][BenchSettings.presentationNamespace]["startCamera"] = start
        try draft.replaceCandidate(candidate)
        return (draft, fields.id, fields.freshViewpointId)
    }
    func testLegacyAbsenceAndFreshEmptyExhibitionCreateStableStandardCameraAtomically() throws {
        let draft = try NativeExhibitionDraft.new()
        XCTAssertEqual(try BenchSettings.list(in: draft), [])
        XCTAssertEqual(try BenchViewpoint.list(in: draft), [])
        let fields = try newFields(draft)
        let command = BenchCommand.add(try fields.settings(), viewpoint: try fields.newViewpoint())
        let first = try command.proposedCandidate(in: draft, selectedBenchId: fields.id)
        XCTAssertEqual(first, try command.proposedCandidate(in: draft, selectedBenchId: fields.id))
        XCTAssertEqual(first["extensions"][BenchSettings.namespace]["benches"].items?.count, 1)
        let presentation = first["extensions"][BenchSettings.presentationNamespace]
        XCTAssertEqual(presentation["viewpoints"].items?.first?["id"], .string(fields.freshViewpointId))
        XCTAssertEqual(presentation["viewpoints"].items?.first?["position"], .vector([0, 1.2, 0]))
        XCTAssertEqual(presentation["credits"], .string(""))
        XCTAssertEqual(draft.candidate["extensions"], .null, "Preview must never change source")
        XCTAssertFalse(ViewportSelection(kind: "bench", id: fields.id).belongs(to: draft))
    }
    func testSetFreshCameraRelinksWithoutChangingSharedViewpointsStartCreditsOrOriginals() throws {
        let (draft, id, oldCamera) = try fixture()
        var fields = try BenchFormBuffer(draft: draft, benchId: id)
        let exactYaw = try fields.settings().yaw
        XCTAssertEqual(exactYaw, try BenchSettings.list(in: draft)[0].yaw)
        XCTAssertNil(try fields.newViewpoint())
        fields.x = "1.25"; fields.color = "#123456"
        try fields.matchNewViewpointToBench()
        let command = BenchCommand.set(try fields.settings(), viewpoint: try fields.newViewpoint())
        let candidate = try command.proposedCandidate(in: draft, selectedBenchId: id)
        let before = draft.candidate["extensions"][BenchSettings.presentationNamespace]
        let after = candidate["extensions"][BenchSettings.presentationNamespace]
        XCTAssertEqual(after["viewpoints"].items?.first, before["viewpoints"].items?.first)
        XCTAssertEqual(after["viewpoints"].items?.first?["id"], .string(oldCamera))
        XCTAssertEqual(after["viewpoints"].items?.count, 2)
        XCTAssertEqual(after["startCamera"], before["startCamera"]); XCTAssertEqual(after["credits"], before["credits"])
        XCTAssertEqual(candidate["extensions"][BenchSettings.namespace]["benches"].items?.first?["viewpointId"], .string(fields.freshViewpointId))
        for key in ["future-private", "org.exhibitos.studio/template"] { XCTAssertEqual(candidate["extensions"][key], draft.candidate["extensions"][key]) }
        for key in ["artworks", "placements", "mediaAssets", "rooms", "lights"] { XCTAssertEqual(candidate[key], draft.candidate[key]) }
        let overwrite = try BenchViewpoint(value: XCTUnwrap(before["viewpoints"].items?.first))
        XCTAssertThrowsError(try BenchCommand.set(try BenchFormBuffer(draft: draft, benchId: id).settings(), viewpoint: overwrite).proposedCandidate(in: draft, selectedBenchId: id))
    }
    func testRemoveRetainsViewpointAndExistingDraftHistoryRoundTripRestoresBench() throws {
        let (draft, id, _) = try fixture()
        let command = BenchCommand.remove(id: id), candidate = try command.proposedCandidate(in: draft, selectedBenchId: id)
        XCTAssertEqual(candidate["extensions"][BenchSettings.presentationNamespace], draft.candidate["extensions"][BenchSettings.presentationNamespace])
        let preview = try BenchPreview(candidate: candidate, base: draft, command: command, selectedBenchId: id)
        XCTAssertEqual(try BenchSettings.list(in: draft).count, 1)
        var applied = try preview.apply(to: draft, selectedBenchId: id, command: command)
        XCTAssertEqual(try BenchSettings.list(in: applied), [])
        XCTAssertEqual(applied.document["editVersion"].integer, draft.document["editVersion"].integer! + 1)
        applied = try JSONDecoder().decode(NativeExhibitionDraft.self, from: JSONEncoder().encode(applied))
        XCTAssertEqual(applied.candidate, candidate)
        try applied.replaceCandidate(draft.candidate)
        XCTAssertEqual(applied.candidate, draft.candidate)
        XCTAssertThrowsError(try command.proposedCandidate(in: preview.displayDraft, selectedBenchId: id))
    }
    func testMalformedAndIncompleteFieldsStayBufferedAndUnsupportedMetadataIsPreserved() throws {
        let (draft, id, _) = try fixture(), original = try BenchFormBuffer(draft: draft, benchId: id)
        for key in [\BenchFormBuffer.x, \.y, \.z, \.yawDegrees, \.width, \.depth, \.height, \.roughness, \.metalness] {
            for text in ["-", "nan", "inf", "1e300"] {
                var fields = original; fields[keyPath: key] = text
                XCTAssertThrowsError(try fields.settings()); XCTAssertEqual(fields[keyPath: key], text)
            }
        }
        for key in [\BenchFormBuffer.viewX, \.viewY, \.viewZ, \.targetX, \.targetY, \.targetZ, \.fov] {
            var fields = original; fields.usesFreshViewpoint = true; fields[keyPath: key] = "-"
            XCTAssertThrowsError(try fields.newViewpoint()); XCTAssertEqual(fields[keyPath: key], "-")
        }
        var fields = original; fields.name = String(repeating: "😀", count: 65)
        XCTAssertThrowsError(try fields.settings(), "Names use the producer's UTF-16 budget")
        fields.name = " "; XCTAssertThrowsError(try fields.settings())
        fields = original; fields.color = "red"; XCTAssertThrowsError(try fields.settings())
        for key in ["version", "future"] {
            var changed = draft, candidate = draft.candidate
            candidate["extensions"][BenchSettings.namespace][key] = .integer(2)
            try changed.replaceCandidate(candidate)
            XCTAssertThrowsError(try BenchSettings.list(in: changed)); XCTAssertEqual(changed.candidate, candidate)
        }
    }
    func testBoundsPresetAndExplicitCameraAlignmentUseSeatHeightAndStoredYaw() throws {
        let draft = try NativeExhibitionDraft.new()
        var fields = try newFields(draft)
        fields.x = "2"; fields.y = "0.1"; fields.z = "-1"; fields.height = "0.65"; fields.yawDegrees = "90"
        try fields.matchNewViewpointToBench()
        let camera = try XCTUnwrap(fields.newViewpoint())
        XCTAssertEqual(camera.position[1], 1.5, accuracy: 1e-14)
        XCTAssertEqual(camera.target[0], 0, accuracy: 1e-14); XCTAssertEqual(camera.target[2], -1, accuracy: 1e-14)
        XCTAssertEqual(try fields.settings().yaw, .pi / 2, accuracy: 1e-15)
        let fresh = fields.freshViewpointId
        try fields.matchNewViewpointToBench(); XCTAssertEqual(fields.freshViewpointId, fresh)
        for preset in InstallationSettings.Preset.allCases {
            fields.applyMaterialPreset(preset)
            XCTAssertEqual(try fields.settings().material, .defaults(preset))
        }
        for (key, value) in [(\BenchFormBuffer.width, "0.59"), (\.depth, "1.01"), (\.height, "0.651"), (\.yawDegrees, "181")] {
            var invalid = fields; invalid[keyPath: key] = value; XCTAssertThrowsError(try invalid.settings())
        }
    }
    func testNarrowPreviewRejectsOtherChangesSelectionStaleBaseAndChangedCommand() throws {
        let (draft, id, _) = try fixture()
        var fields = try BenchFormBuffer(draft: draft, benchId: id); fields.name = "Edited bench"
        let command = BenchCommand.set(try fields.settings(), viewpoint: nil)
        let candidate = try command.proposedCandidate(in: draft, selectedBenchId: id)
        let preview = try BenchPreview(candidate: candidate, base: draft, command: command, selectedBenchId: id)
        var changed = draft; try changed.rename("Other edit")
        XCTAssertThrowsError(try preview.apply(to: changed, selectedBenchId: id, command: command))
        XCTAssertThrowsError(try preview.apply(to: draft, selectedBenchId: nil, command: command))
        XCTAssertThrowsError(try preview.apply(to: draft, selectedBenchId: id, command: .remove(id: id)))
        for key in ["title", "artworks", "lights"] {
            var unrelated = candidate; unrelated[key] = .string("changed")
            XCTAssertThrowsError(try BenchPreview(candidate: unrelated, base: draft, command: command, selectedBenchId: id))
        }
        XCTAssertThrowsError(try BenchCommand.add(try fields.settings(), viewpoint: nil).proposedCandidate(in: draft, selectedBenchId: id))
    }
    func testBenchCountAndFreshCameraReferenceLimitsRefuseWithoutDroppingData() throws {
        var draft = try NativeExhibitionDraft.new()
        for _ in 0..<32 { draft = try add(newFields(draft), to: draft) }
        XCTAssertEqual(try BenchSettings.list(in: draft).count, 32)
        let fields = try newFields(draft)
        XCTAssertThrowsError(try add(fields, to: draft)); XCTAssertEqual(try BenchSettings.list(in: draft).count, 32)
        let empty = try NativeExhibitionDraft.new(), missingCamera = try newFields(empty)
        XCTAssertThrowsError(try BenchCommand.add(missingCamera.settings(), viewpoint: nil).proposedCandidate(in: empty, selectedBenchId: missingCamera.id))
        var cameraFull = empty, candidate = empty.candidate
        let view = try XCTUnwrap(missingCamera.newViewpoint())
        var views: [DocumentValue] = []
        for _ in 0..<64 {
            var row = view.value; row["id"] = .string(UUID().uuidString.lowercased()); views.append(row)
        }
        candidate["extensions"] = .object([BenchSettings.presentationNamespace: .object(["version": .integer(1), "viewpoints": .array(views), "credits": .string("")])])
        try cameraFull.replaceCandidate(candidate)
        XCTAssertThrowsError(try add(missingCamera, to: cameraFull))
        XCTAssertEqual(try BenchViewpoint.list(in: cameraFull).count, 64)
    }
}
