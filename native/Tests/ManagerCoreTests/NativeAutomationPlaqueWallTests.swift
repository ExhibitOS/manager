// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class NativeAutomationPlaqueWallTests: XCTestCase {
    func testPlaqueCommandPreservesExistingRoomAndRejectsExtraFields() throws {
        let draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        var form = try PlaqueFormBuffer(roomId: room)
        form.sourceKind = .room
        let plaque = try form.settings()
        let value: DocumentValue = .object(["type": .string("add-plaque"), "plaque": plaque.value, "roomIntroductionText": .string("Route to the next room")])
        let parsed = try NativeAutomationPlaqueWall.plaque(value)
        XCTAssertEqual(parsed, .add(plaque, roomIntroductionText: "Route to the next room"))
        XCTAssertNoThrow(try parsed.proposedCandidate(in: draft, selectedPlaqueId: plaque.id))
        var invalid = value; invalid["unexpected"] = .bool(true)
        XCTAssertThrowsError(try NativeAutomationPlaqueWall.plaque(invalid))
        invalid = value; invalid["roomIntroductionText"] = .bool(true)
        XCTAssertThrowsError(try NativeAutomationPlaqueWall.plaque(invalid))
    }

    func testWallCommandUsesNativeBoundsAndRejectsUnexpectedData() throws {
        let id = UUID().uuidString.lowercased()
        let wall: DocumentValue = .object(["surfaceId": .string(id), "depth": .number(0.02), "preset": .string("painted-edge"), "trimWidth": .number(0.02), "baseboardHeight": .number(0.06), "ceilingBand": .bool(false)])
        var command: DocumentValue = .object(["type": .string("set-wall-finish"), "wall": wall])
        XCTAssertEqual(try NativeAutomationPlaqueWall.wall(command).value, command)
        command["wall"]["baseboardHeight"] = .number(0.01)
        XCTAssertThrowsError(try NativeAutomationPlaqueWall.wall(command))
        command["wall"] = wall; command["unrelated"] = .bool(true)
        XCTAssertThrowsError(try NativeAutomationPlaqueWall.wall(command))
    }
}
