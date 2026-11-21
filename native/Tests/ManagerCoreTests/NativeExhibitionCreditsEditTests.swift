// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class NativeExhibitionCreditsEditTests: XCTestCase {
    func testChangesOnlyCreditsAndSupportsExactInverse() throws {
        var base = try NativeExhibitionDraft.new()
        let namespace = BenchSettings.presentationNamespace
        let room = try XCTUnwrap(base.rooms.first?["id"].string)
        var candidate = base.candidate
        candidate["extensions"] = .object([namespace: .object([
            "version": .integer(1),
            "viewpoints": .array([.object([
                "id": .string("e8087f03-4e4d-4e8b-bd20-92bd50c691fd"),
                "name": .string("Retained viewpoint"), "roomId": .string(room),
                "position": .vector([0, 1.6, 1]), "target": .vector([0, 1.6, 0]), "fov": .integer(55)
            ])]),
            "startCamera": .object(["roomId": .string(room), "position": .vector([0, 1.6, 1]),
                                    "target": .vector([0, 1.6, 0]), "fov": .integer(55)]),
            "credits": .string("Retained credit")
        ]), "org.example/other": .bool(true)])
        try base.replaceCandidate(candidate)
        let edit = try NativeExhibitionCreditsEdit(.string("Retained credit\nApproved asset credit"))
        let staged = try XCTUnwrap(edit.applying(to: base))
        var expected = candidate
        expected["extensions"][namespace]["credits"] = .string(edit.credits)
        XCTAssertEqual(staged.candidate, expected)
        XCTAssertNil(try edit.applying(to: staged))
        let inverse = try NativeExhibitionCreditsEdit(.string("Retained credit"))
        XCTAssertEqual(try inverse.applying(to: staged)?.candidate, base.candidate)
    }

    func testEnforcesUTF16LimitAndRequiresExistingPresentation() throws {
        XCTAssertNoThrow(try NativeExhibitionCreditsEdit(.string(String(repeating: "🌾", count: 2048))))
        XCTAssertThrowsError(try NativeExhibitionCreditsEdit(.string(String(repeating: "🌾", count: 2049))))
        XCTAssertThrowsError(try NativeExhibitionCreditsEdit(.integer(1)))
        let edit = try NativeExhibitionCreditsEdit(.string("credit"))
        XCTAssertThrowsError(try edit.applying(to: NativeExhibitionDraft.new()))
    }
}
