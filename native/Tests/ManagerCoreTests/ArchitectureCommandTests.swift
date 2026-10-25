// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class ArchitectureCommandTests: XCTestCase {
    private func fixture() throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new()
        var candidate = draft.candidate
        var room = draft.rooms[0]
        room["extensions"] = .object(["future-room": .string("preserved")])
        candidate["rooms"] = .array([room])
        let wall = UUID().uuidString.lowercased(), opening = UUID().uuidString.lowercased()
        candidate["surfaces"] = .array([.object(["id": .string(wall), "roomId": room["id"], "type": .string("wall"), "dimensions": .object(["width": .number(8), "height": .number(4)]), "transform": NativeExhibitionDraft.identity, "extensions": .object(["future-wall": .bool(true)])])])
        candidate["openings"] = .array([.object(["id": .string(opening), "surfaceId": .string(wall), "type": .string("door"), "dimensions": .object(["width": .number(1), "height": .number(2)]), "offset": .vector([0, -1]), "exterior": .bool(true), "connectsToOpeningId": .string(UUID().uuidString.lowercased())])])
        candidate["extensions"] = .object(["future-exhibition": .string("preserved")])
        try draft.replaceCandidate(candidate)
        return draft
    }
    func testRoomProposalPreservesOpaqueFieldsAndNeverRescalesChildren() throws {
        let base = try fixture(), id = base.rooms[0]["id"].string!
        let selected = ViewportSelection(kind: "room", id: id)
        let command = ArchitectureCommand.room(id: id, name: "공간 이름", width: 15, height: 5, depth: 9, position: [2, 0, 3])
        let action = try command.encodedValue(in: base, selected: selected)
        XCTAssertEqual(action["type"].string, "set-room")
        XCTAssertEqual(action["room"]["id"], .null)
        XCTAssertEqual(action["room"]["extensions"], base.rooms[0]["extensions"])
        XCTAssertEqual(action["room"]["transform"]["rotation"], base.rooms[0]["transform"]["rotation"])
        var proposal = base.candidate, row = action["room"]
        row["id"] = .string(id); proposal["rooms"] = .array([row])
        try command.checkProposal(proposal, base: base, selected: selected)
        for key in ["surfaces", "openings", "artworks", "extensions"] { XCTAssertEqual(proposal[key], base.candidate[key]) }
        var unexpected = proposal
        unexpected["title"] = .string("unrelated change")
        XCTAssertThrowsError(try command.checkProposal(unexpected, base: base, selected: selected))
        unexpected = proposal; var dropped = row; dropped["extensions"] = .null
        unexpected["rooms"] = .array([dropped])
        XCTAssertThrowsError(try command.checkProposal(unexpected, base: base, selected: selected))
        unexpected = proposal; unexpected["surfaces"] = .array([])
        XCTAssertThrowsError(try command.checkProposal(unexpected, base: base, selected: selected))
    }
    func testSurfaceAndOpeningUseCompleteExistingRecordsAndBoundSelection() throws {
        let base = try fixture(), wall = base.candidate["surfaces"].items![0], door = base.candidate["openings"].items![0]
        let surface = ArchitectureCommand.surface(id: wall["id"].string!, width: 7, height: 3, position: [1, 2, 3])
        let wallAction = try surface.encodedValue(in: base, selected: surface.selection)
        XCTAssertEqual(wallAction["surface"]["roomId"], wall["roomId"])
        XCTAssertEqual(wallAction["surface"]["extensions"], wall["extensions"])
        XCTAssertEqual(wallAction["surface"]["transform"]["scale"], wall["transform"]["scale"])
        let opening = ArchitectureCommand.opening(id: door["id"].string!, width: 1.2, height: 2.1, offset: [0.4, -0.7])
        let doorAction = try opening.encodedValue(in: base, selected: opening.selection)
        XCTAssertEqual(doorAction["opening"]["surfaceId"], door["surfaceId"])
        XCTAssertEqual(doorAction["opening"]["connectsToOpeningId"], door["connectsToOpeningId"])
        XCTAssertEqual(doorAction["opening"]["exterior"], door["exterior"])
        XCTAssertThrowsError(try surface.encodedValue(in: base, selected: opening.selection))
        XCTAssertThrowsError(try opening.encodedValue(in: base, selected: nil))
        XCTAssertThrowsError(try NativeGeometryAction.architecture(surface).encodedValue())
    }
    func testInvalidTextAndNumbersDoNotMutateTheDraft() throws {
        let base = try fixture(), selected = ViewportSelection(kind: "room", id: base.rooms[0]["id"].string!)
        var fields = try ArchitectureFormBuffer(draft: base, selection: selected)
        let original = fields
        fields.width = "unfinished"
        XCTAssertThrowsError(try fields.command())
        XCTAssertEqual(fields.width, "unfinished")
        fields = original; fields.width = "0"
        XCTAssertThrowsError(try fields.command().encodedValue(in: base, selected: selected))
        let bad = ArchitectureCommand.room(id: selected.id, name: "방", width: .nan, height: 4, depth: 8, position: [0, 0, 0])
        XCTAssertThrowsError(try bad.encodedValue(in: base, selected: selected))
        XCTAssertEqual(try ArchitectureFormBuffer(draft: base, selection: selected), original)
    }
    func testTemplateModifiedFlagCorrespondsWithoutAllowingOtherExtensionChanges() throws {
        let template = "org.exhibitos.studio/template"
        var draft = try fixture(), candidate = draft.candidate
        candidate["extensions"][template] = .object(["version": .integer(1), "name": .string("Gallery"), "creator": .string("ExhibitOS test"), "license": .string("CC0-1.0"), "source": .string("https://example.com/template"), "modified": .bool(false), "profile": .string("oes-rectangles-v1")])
        try draft.replaceCandidate(candidate)
        let base = draft, id = draft.rooms[0]["id"].string!
        let command = ArchitectureCommand.room(id: id, name: "바꾼 방", width: 12, height: 4, depth: 8, position: [0, 0, 0])
        var row = try command.encodedValue(in: base, selected: command.selection)["room"]
        row["id"] = .string(id)
        var proposal = base.candidate; proposal["rooms"] = .array([row])
        proposal["extensions"][template]["modified"] = .bool(true)
        try draft.acceptArchitectureGeometry(proposal, command: command, selected: command.selection)
        XCTAssertEqual(draft.candidate["extensions"][template]["modified"], .bool(true))
        XCTAssertEqual(base.candidate["extensions"][template]["modified"], .bool(false))
        XCTAssertEqual(draft.document["editVersion"].integer, base.document["editVersion"].integer! + 1)
        for key in ["license", "creator", "source", "profile"] {
            var wrong = proposal; wrong["extensions"][template][key] = .string("unrelated")
            XCTAssertThrowsError(try command.checkProposal(wrong, base: base, selected: command.selection))
        }
        var wrong = proposal; wrong["extensions"]["future-exhibition"] = .string("changed")
        XCTAssertThrowsError(try command.checkProposal(wrong, base: base, selected: command.selection))
    }
}
