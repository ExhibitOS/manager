// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class FurnishingWalkingCollisionEvaluationTests: XCTestCase {
    private let id = "ad000000-0000-4000-8000-000000000001"

    func testExplicitOffRemainsDistinctFromAnInheritedDefault() throws {
        let draft = try NativeExhibitionDraft.new()
        let inherited = row()
        let inheritedResponse = response([inherited])
        XCTAssertEqual(try FurnishingWalkingCollisionEvaluation(inheritedResponse, in: draft).value, inheritedResponse)

        var edited = draft
        var candidate = draft.candidate
        candidate["extensions"] = .object([
            "org.exhibitos.studio/furnishing-walking-collision": .object([
                "version": .integer(1),
                "overrides": .array([.object(["kind": .string("workspace"), "id": .string(id), "enabled": .bool(false)])])
            ])
        ])
        try edited.replaceCandidate(candidate)
        var off = inherited
        off["override"] = .bool(false)
        let offResponse = response([off])
        XCTAssertEqual(try FurnishingWalkingCollisionEvaluation(offResponse, in: edited).value, offResponse)
        XCTAssertThrowsError(try FurnishingWalkingCollisionEvaluation(inheritedResponse, in: edited))
        XCTAssertThrowsError(try FurnishingWalkingCollisionEvaluation(offResponse, in: draft))
        XCTAssertThrowsError(try FurnishingWalkingCollisionEvaluation(response([]), in: edited))
    }

    func testRejectsDuplicateTargetsUnknownKindsAndNullOverrides() throws {
        let draft = try NativeExhibitionDraft.new()
        var duplicate = row()
        duplicate["id"] = .string(id.uppercased())
        XCTAssertThrowsError(try FurnishingWalkingCollisionEvaluation(response([row(), duplicate]), in: draft))

        var unknown = row()
        unknown["kind"] = .string("wall")
        XCTAssertThrowsError(try FurnishingWalkingCollisionEvaluation(response([unknown]), in: draft))
        var nullOverride = row()
        nullOverride["override"] = .null
        XCTAssertThrowsError(try FurnishingWalkingCollisionEvaluation(response([nullOverride]), in: draft))
        XCTAssertThrowsError(try FurnishingWalkingCollisionEvaluation(response(Array(repeating: row(), count: 1025)), in: draft))
    }

    private func row() -> DocumentValue {
        .object(["kind": .string("workspace"), "id": .string(id), "defaultEnabled": .bool(true)])
    }

    private func response(_ rows: [DocumentValue]) -> DocumentValue { .object(["targets": .array(rows)]) }
}
