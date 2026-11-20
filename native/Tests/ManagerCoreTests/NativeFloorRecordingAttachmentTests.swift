// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class NativeFloorRecordingAttachmentTests: XCTestCase {
    private let surface = "bdedd1da-0a03-442d-a58d-181877fe0652"
    private let audio = "4657f277-fdf6-46f7-8fe5-6110555697c3"

    private func exactDraft() throws -> NativeExhibitionDraft {
        guard let path = ProcessInfo.processInfo.environment["EXHIBITOS_TEST_R219_WHEAT_FIT_JOURNAL"] else {
            throw XCTSkip("Requires the detached r219 journal fixture")
        }
        let bytes = try Data(contentsOf: URL(fileURLWithPath: path))
        return try XCTUnwrap(JSONDecoder().decode(NativeDocumentState.self, from: bytes).draft)
    }

    private var command: DocumentValue {
        .object(["surfaceId": .string(surface), "materialId": .string("cc0bfbc1-b1c8-4250-9014-dfdb6467b810"),
                 "materialVersion": .integer(1), "audioId": .string(audio), "audioRevision": .integer(1),
                 "expectedMediaAsset": .object(["id": .string(audio), "mime": .string("audio/wav"), "bytes": .integer(60_146),
                    "sha256": .string("989659bd0b4e01e1cd80287e3c5932f2220cafa07485d6d1b1d23907dedcbce4")])])
    }

    func testEntranceCandidatePreservesOtherRowsAndExactReplay() throws {
        let original = try exactDraft()
        let attachment = try NativeFloorRecordingAttachment(command)
        let candidate = try attachment.expectedCandidate(in: original)
        let namespace = ConcreteFootsteps.experienceNamespace
        var expected = original.candidate
        var rows = try XCTUnwrap(expected["extensions"][namespace]["footsteps"].items)
        let index = try XCTUnwrap(rows.firstIndex { $0["surfaceId"] == .string(surface) })
        rows[index]["assetId"] = .string(audio)
        expected["extensions"][namespace]["footsteps"] = .array(rows)
        let media = try XCTUnwrap(expected["mediaAssets"].items)
        expected["mediaAssets"] = .array(media + [command["expectedMediaAsset"]])
        XCTAssertEqual(candidate, expected)
        var applied = original
        try applied.replaceCandidate(expected)
        XCTAssertEqual(try attachment.expectedCandidate(in: applied), expected)
    }

    func testRejectsWrongAssetAndMutesWithoutChangingDraft() throws {
        var command = command
        command["expectedMediaAsset"]["sha256"] = .string(String(repeating: "0", count: 64))
        XCTAssertThrowsError(try NativeFloorRecordingAttachment(command))
        var draft = try exactDraft()
        let attachment = try NativeFloorRecordingAttachment(self.command)
        var candidate = draft.candidate
        let floorNamespace = "org.exhibitos.studio/floors"
        var bindings = try XCTUnwrap(candidate["extensions"][floorNamespace]["bindings"].items)
        var muted = try XCTUnwrap(bindings.first)
        muted["target"]["surfaceId"] = .string(surface)
        muted["sound"] = .object(["mode": .string("none"), "gain": .number(0)])
        bindings.append(muted)
        candidate["extensions"][floorNamespace]["bindings"] = .array(bindings)
        try draft.replaceCandidate(candidate)
        XCTAssertThrowsError(try attachment.expectedCandidate(in: draft)) { error in
            XCTAssertEqual((error as? AuthoringFailure)?.code, "FLOOR_SOUND_CONFLICT")
        }
        XCTAssertEqual(draft.candidate, candidate)
    }
}
