// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class ViewingPointFormBufferTests: XCTestCase {
    private func fixture() throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        candidate["extensions"] = .object(["example.org/opaque": .object(["keep": .integer(7)])])
        try draft.replaceCandidate(candidate); return draft
    }
    func testIndependentStandingIntentIsAppendOnlyAndNeverCreatesABenchOrPlan() throws {
        let base = try fixture(), room = try XCTUnwrap(base.rooms.first?["id"].string)
        let fields = try ViewingPointFormBuffer(draft: base, roomId: room)
        let first = try fields.candidate(in: base)
        XCTAssertEqual(first, try fields.candidate(in: base), "One form retains one identity across retries")
        var next = base; try next.replaceCandidate(first)
        let point = try XCTUnwrap(BenchViewpoint.list(in: next).first)
        XCTAssertEqual(point.id, fields.id); XCTAssertEqual(point.position, [0, 1.65, 1])
        XCTAssertEqual(first["extensions"]["example.org/opaque"], base.candidate["extensions"]["example.org/opaque"])
        XCTAssertEqual(first["extensions"][BenchSettings.namespace], .null)
        XCTAssertEqual(first["extensions"][ViewingPlanSettings.namespace], .null)
        for key in ["rooms", "surfaces", "placements", "artworks", "lights", "navigation"] { XCTAssertEqual(first[key], base.candidate[key]) }
        XCTAssertThrowsError(try fields.candidate(in: next), "A prior identity is never appended again")
    }
    func testInvalidRawInputAndRoomBoundsRemainNonmutating() throws {
        let base = try fixture(), room = try XCTUnwrap(base.rooms.first?["id"].string)
        var fields = try ViewingPointFormBuffer(draft: base, roomId: room)
        for raw in ["-", "nan", "inf", "10001"] {
            fields.x = raw; XCTAssertThrowsError(try fields.candidate(in: base)); XCTAssertEqual(fields.x, raw)
        }
        fields.x = "0"; fields.y = "-0.1"; XCTAssertThrowsError(try fields.candidate(in: base))
        fields.y = "1.65"; fields.fov = "121"; XCTAssertThrowsError(try fields.candidate(in: base))
        fields.fov = "50"; fields.name = " "; XCTAssertThrowsError(try fields.candidate(in: base))
        fields.name = "View"; fields.roomId = UUID().uuidString; XCTAssertThrowsError(try fields.candidate(in: base))
        XCTAssertEqual(base.candidate["extensions"][BenchSettings.presentationNamespace], .null)
    }
    func testExistingViewpointsWatermarkAndFutureProfilesArePreserved() throws {
        var base = try fixture(), candidate = base.candidate
        let room = try XCTUnwrap(base.rooms.first?["id"].string)
        let old = try BenchViewpoint(id: UUID().uuidString, name: "Existing", roomId: room, position: [1, 1.65, 0], target: [0, 1.65, -1], fov: 60)
        candidate["extensions"][BenchSettings.presentationNamespace] = .object(["version": .integer(1), "viewpoints": .array([old.value]), "credits": .string("retain"), "watermark": .object(["version": .integer(1), "mode": .string("none")])])
        try base.replaceCandidate(candidate)
        let fields = try ViewingPointFormBuffer(draft: base, roomId: room), result = try fields.candidate(in: base)
        XCTAssertEqual(result["extensions"][BenchSettings.presentationNamespace]["viewpoints"].items?.first, old.value)
        for key in ["credits", "watermark"] { XCTAssertEqual(result["extensions"][BenchSettings.presentationNamespace][key], candidate["extensions"][BenchSettings.presentationNamespace][key]) }
        candidate["extensions"][BenchSettings.presentationNamespace]["version"] = .integer(2)
        try base.replaceCandidate(candidate)
        XCTAssertThrowsError(try fields.candidate(in: base)); XCTAssertEqual(base.candidate, candidate)
        candidate["extensions"][BenchSettings.presentationNamespace]["version"] = .integer(1)
        candidate["extensions"][BenchSettings.presentationNamespace]["viewpoints"] = .array((0..<64).map { _ in
            var value = old.value; value["id"] = .string(UUID().uuidString); return value
        })
        try base.replaceCandidate(candidate)
        XCTAssertThrowsError(try fields.candidate(in: base), "Never append beyond the existing 64-point limit")
    }
}
