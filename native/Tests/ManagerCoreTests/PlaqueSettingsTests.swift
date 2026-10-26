// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class PlaqueSettingsTests: XCTestCase {
    /// Same native metadata setup used by existing document tests; no runtime
    /// asset/scene fixture or claim of full shared geometry validation.
    func fixture() throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(title: "Existing exhibition"), candidate = draft.candidate
        candidate["extensions"] = .object(["example.org/private": .object(["opaque": .string("keep original")]),
                                            "org.exhibitos.studio/template": .object(["modified": .bool(false)])])
        try draft.replaceCandidate(candidate)
        let artwork: DocumentValue = .object(["kind": .string("artwork"), "revisionId": .string(UUID().uuidString.lowercased()),
            "primaryAssetId": .string(UUID().uuidString.lowercased()), "metadata": .object(["title": .string("Existing artwork"), "description": .string("Preserve existing prose")]),
            "rights": .object(["permissions": .object(["display": .bool(true)])]), "extensions": .object(["example.org/original": .string("unchanged")])])
        try draft.addApprovedArtwork(artwork, roomId: XCTUnwrap(draft.rooms.first?["id"].string), position: [0,1,0])
        return draft
    }
    func fields(_ draft: NativeExhibitionDraft) throws -> PlaqueFormBuffer { try PlaqueFormBuffer(roomId: XCTUnwrap(draft.rooms.first?["id"].string)) }
    func adding(_ fields: PlaqueFormBuffer, to draft: NativeExhibitionDraft) throws -> NativeExhibitionDraft {
        let command = PlaqueCommand.add(try fields.settings())
        return try PlaquePreview(candidate: command.proposedCandidate(in: draft, selectedPlaqueId: fields.id), base: draft, command: command, selectedPlaqueId: fields.id)
            .apply(to: draft, selectedPlaqueId: fields.id, command: command)
    }
    func testLegacyAbsenceAndThreeTypedSourcesStoreNoDuplicatedText() throws {
        var draft = try fixture(); let original = draft
        XCTAssertEqual(try PlaqueSettings.list(in: draft), [])
        for kind in PlaqueFormBuffer.SourceKind.allCases {
            var input = try fields(draft); input.sourceKind = kind
            input.placementId = try XCTUnwrap(draft.placements.first?["id"].string)
            draft = try adding(input, to: draft)
            let row = try XCTUnwrap(draft.candidate["extensions"][PlaqueSettings.namespace]["plaques"].items?.last)
            guard case .object(let values) = row else { return XCTFail("Expected plaque") }
            XCTAssertEqual(Set(values.keys), ["id", "roomId", "source", "position", "rotation", "dimensions", "visible"])
            XCTAssertEqual(row["source"]["kind"], .string(kind.rawValue))
            XCTAssertEqual(row["source"]["exhibitionId"], .null)
            XCTAssertEqual(row["title"], .null); XCTAssertEqual(row["body"], .null); XCTAssertEqual(row["assetId"], .null)
        }
        XCTAssertEqual(draft.artworks, original.artworks); XCTAssertEqual(draft.placements, original.placements)
        XCTAssertEqual(draft.candidate["extensions"]["example.org/private"], original.candidate["extensions"]["example.org/private"])
        XCTAssertEqual(draft.candidate["extensions"]["org.exhibitos.studio/template"]["modified"], .bool(true))
        XCTAssertEqual(original.candidate["extensions"][PlaqueSettings.namespace], .null)
    }
    func testSetPreservesUnselectedHiddenRowOriginalsAndQuaternionBits() throws {
        var draft = try fixture(), first = try fields(draft)
        first.rotationX = String(sin(Double.pi/8)); first.rotationW = String(cos(Double.pi/8))
        draft = try adding(first, to: draft)
        var hidden = try fields(draft); hidden.sourceKind = .room; hidden.visible = false
        draft = try adding(hidden, to: draft)
        let original = draft, stored = try PlaqueSettings.list(in: draft)[0]
        var input = try PlaqueFormBuffer(draft: draft, plaqueId: first.id)
        XCTAssertEqual(try input.settings().rotation, stored.rotation)
        input.width = "1.2"; input.visible = false; input.sourceKind = .artwork
        input.placementId = try XCTUnwrap(draft.placements.first?["id"].string)
        let command = PlaqueCommand.set(try input.settings()), candidate = try command.proposedCandidate(in: draft, selectedPlaqueId: first.id)
        XCTAssertEqual(candidate["extensions"][PlaqueSettings.namespace]["plaques"].items?[1], original.candidate["extensions"][PlaqueSettings.namespace]["plaques"].items?[1])
        for key in ["artworks", "placements", "rooms", "lights", "accessibility"] { XCTAssertEqual(candidate[key], original.candidate[key]) }
        XCTAssertEqual(candidate["extensions"]["example.org/private"], original.candidate["extensions"]["example.org/private"])
        XCTAssertEqual(try PlaqueFormBuffer(draft: draft, plaqueId: hidden.id).visible, false)
    }
    func testRemoveRoundTripUndoRedoAndLocalCopyPreserveSourceReferences() throws {
        var draft = try fixture(), input = try fields(draft)
        input.sourceKind = .artwork; input.placementId = try XCTUnwrap(draft.placements.first?["id"].string)
        draft = try adding(input, to: draft)
        let command = PlaqueCommand.remove(id: input.id), candidate = try command.proposedCandidate(in: draft, selectedPlaqueId: input.id)
        let preview = try PlaquePreview(candidate: candidate, base: draft, command: command, selectedPlaqueId: input.id)
        var applied = try preview.apply(to: draft, selectedPlaqueId: input.id, command: command)
        XCTAssertEqual(try PlaqueSettings.list(in: applied), [])
        XCTAssertEqual(applied.document["editVersion"].integer, draft.document["editVersion"].integer! + 1)
        applied = try JSONDecoder().decode(NativeExhibitionDraft.self, from: JSONEncoder().encode(applied))
        XCTAssertEqual(applied.candidate, candidate)
        try applied.replaceCandidate(draft.candidate); XCTAssertEqual(applied.candidate, draft.candidate)
        try applied.replaceCandidate(candidate); XCTAssertEqual(applied.candidate, candidate)
        let copy = try draft.copyForEditing()
        XCTAssertNotEqual(copy.id, draft.id)
        XCTAssertEqual(copy.candidate["extensions"][PlaqueSettings.namespace], draft.candidate["extensions"][PlaqueSettings.namespace])
        XCTAssertEqual(try PlaqueSettings.list(in: copy).first?.source, .artwork(placementId: input.placementId))
        XCTAssertEqual(copy.artworks, draft.artworks)
    }
    func testIncompleteTextBoundsQuaternionAndFacingPresetsAreExplicit() throws {
        let draft = try fixture(), original = try fields(draft)
        XCTAssertEqual(try original.settings().id, try original.settings().id)
        for key in [\PlaqueFormBuffer.x, \.y, \.z, \.width, \.height, \.rotationX, \.rotationY, \.rotationZ, \.rotationW] {
            for text in ["-", "nan", "inf", "1e300"] {
                var input = original; input[keyPath: key] = text
                XCTAssertThrowsError(try input.settings()); XCTAssertEqual(input[keyPath: key], text)
            }
        }
        for (key, value) in [(\PlaqueFormBuffer.width, "2.001"), (\.height, "0.149"), (\.x, "10001"), (\.rotationW, "0.5")] {
            var input = original; input[keyPath: key] = value; XCTAssertThrowsError(try input.settings())
        }
        for direction in PlaqueFormBuffer.Facing.allCases {
            var input = original; input.rotationW = "-"; input.face(direction)
            let pose = try input.settings(); XCTAssertEqual(pose.id, original.id)
            XCTAssertEqual(sqrt(pose.rotation.reduce(0) { $0 + $1 * $1 }), 1, accuracy: 1e-15)
        }
    }
    func testReferenceAdmissionIsSameRoomCaseInsensitiveAndHiddenDoesNotBypassIt() throws {
        let draft = try fixture(); var input = try fields(draft)
        input.sourceKind = .artwork; input.placementId = try XCTUnwrap(draft.placements.first?["id"].string).uppercased()
        XCTAssertNoThrow(try adding(input, to: draft))
        input.placementId = UUID().uuidString.lowercased(); input.visible = false
        XCTAssertThrowsError(try adding(input, to: draft))
        var other = draft, candidate = draft.candidate, room = draft.rooms[0]
        room["id"] = .string(UUID().uuidString.lowercased()); candidate["rooms"] = .array([draft.rooms[0], room]); try other.replaceCandidate(candidate)
        input.roomId = try XCTUnwrap(room["id"].string); input.placementId = try XCTUnwrap(draft.placements.first?["id"].string)
        XCTAssertThrowsError(try adding(input, to: other))
        var unknown = try fields(draft).settings().value; unknown["body"] = .string("No copied prose")
        XCTAssertThrowsError(try PlaqueSettings(value: unknown))
        candidate = draft.candidate; candidate["extensions"][PlaqueSettings.namespace] = .object(["version": .integer(2), "plaques": .array([])])
        try other.replaceCandidate(candidate); XCTAssertThrowsError(try PlaqueSettings.list(in: other)); XCTAssertEqual(other.candidate, candidate)
    }
    func testNarrowPreviewRejectsForeignDeltaStaleBaseSelectionAndChangedInput() throws {
        let draft = try fixture(), input = try fields(draft), command = PlaqueCommand.add(try input.settings())
        let candidate = try command.proposedCandidate(in: draft, selectedPlaqueId: input.id)
        let preview = try PlaquePreview(candidate: candidate, base: draft, command: command, selectedPlaqueId: input.id)
        var changed = draft; try changed.rename("Changed existing source text")
        XCTAssertThrowsError(try preview.apply(to: changed, selectedPlaqueId: input.id, command: command))
        XCTAssertThrowsError(try preview.apply(to: draft, selectedPlaqueId: nil, command: command))
        var fields = input; fields.visible = false
        XCTAssertThrowsError(try preview.apply(to: draft, selectedPlaqueId: input.id, command: .add(fields.settings())))
        for key in ["artworks", "title", "rooms", "extensions"] {
            var foreign = candidate; foreign[key] = .string("unrelated")
            XCTAssertThrowsError(try PlaquePreview(candidate: foreign, base: draft, command: command, selectedPlaqueId: input.id))
        }
    }
    func testCountLimitAndMissingIdentityRefuseWithoutDroppingExistingData() throws {
        var draft = try fixture()
        for _ in 0..<32 { draft = try adding(fields(draft), to: draft) }
        XCTAssertEqual(try PlaqueSettings.list(in: draft).count, 32)
        let original = draft; XCTAssertThrowsError(try adding(fields(draft), to: draft)); XCTAssertEqual(draft, original)
        let missing = UUID().uuidString.lowercased()
        XCTAssertThrowsError(try PlaqueCommand.remove(id: missing).proposedCandidate(in: draft, selectedPlaqueId: missing))
    }
}
