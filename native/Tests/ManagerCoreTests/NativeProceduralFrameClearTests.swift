// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class NativeProceduralFrameClearTests: XCTestCase {
    private let first = "12345678-1234-1234-1234-123456789abc"
    private let second = "12345678-1234-1234-1234-123456789def"

    private func command(_ id: String? = nil) throws -> NativeProceduralFrameClear.Command {
        try .init(.object(["placementId": .string(id ?? first)]))
    }

    private func makeDraft() throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(now: Date(timeIntervalSince1970: 1_000))
        var candidate = draft.candidate
        candidate["placements"] = .array([first, second].map { id in
            .object(["id": .string(id), "roomId": draft.rooms[0]["id"],
                     "artworkRevisionId": .string("art-" + id), "assetId": .string("asset-" + id),
                     "transform": NativeExhibitionDraft.identity])
        })
        candidate["artworks"] = .array([.object(["opaque": .string("preserve artwork")])])
        candidate["rights"] = .object(["grant": .string("preserve"), "count": .integer(1)])
        candidate["extensions"] = .object(["org.example/other": .object(["value": .integer(9)])])
        try draft.replaceCandidate(candidate)
        try draft.setInstallation(.init(pedestal: .init(kind: .box, position: [1, 0, 2], yaw: 0.123),
                                        frame: .init(kind: .floating, borderWidth: 0.08, gap: 0.03, matWidth: 0.04)), for: first)
        try draft.setInstallation(.init(frame: .init(kind: .stepped)), for: second)
        return draft
    }

    private func assertFailure(_ expected: String, _ body: () throws -> Void,
                               file: StaticString = #filePath, line: UInt = #line) {
        do { try body(); XCTFail("Expected \(expected)", file: file, line: line) }
        catch let failure as AuthoringFailure { XCTAssertEqual(failure.code, expected, file: file, line: line) }
        catch { XCTFail("Unexpected error: \(error)", file: file, line: line) }
    }

    func testPreviewAndApplyChangeOnlyTargetFrameAndRetainRecovery() throws {
        let base = try makeDraft()
        let proposal = try NativeProceduralFrameClear.Proposal(command: command(), draft: base)
        let namespace = InstallationSettings.namespace
        var expected = base.candidate
        var rows = try XCTUnwrap(expected["extensions"][namespace]["placements"].items)
        let original = rows[0]["frame"]
        rows[0]["frame"] = .object(["kind": .string("none")])
        expected["extensions"][namespace]["placements"] = .array(rows)
        XCTAssertEqual(proposal.originalFrame, original)
        XCTAssertEqual(proposal.base, base)
        XCTAssertEqual(proposal.draft.candidate, expected)
        XCTAssertEqual(proposal.draft.document["editVersion"].integer, try XCTUnwrap(base.document["editVersion"].integer) + 1)
        XCTAssertEqual(try proposal.apply(to: base), proposal.draft)
        XCTAssertEqual(try proposal.draft.installationSettings(for: first)?.pedestal, try base.installationSettings(for: first)?.pedestal)
        XCTAssertEqual(try proposal.draft.installationSettings(for: second), try base.installationSettings(for: second))
        XCTAssertEqual(proposal.draft.candidate["placements"], base.candidate["placements"])
        XCTAssertEqual(proposal.draft.candidate["artworks"], base.candidate["artworks"])
        XCTAssertEqual(proposal.draft.candidate["rights"], base.candidate["rights"])
    }

    func testCommandIsClosedAndCanonical() throws {
        let values: [DocumentValue] = [.null, .object([:]), .object(["placementId": .string("bad")]),
            .object(["placementId": .string(first.uppercased())]),
            .object(["placementId": .string(first), "candidate": .object([:])]),
            .object(["placementId": .integer(1)])]
        for value in values { XCTAssertThrowsError(try NativeProceduralFrameClear.Command(value)) }
        XCTAssertEqual(try command().placementId, first)
    }

    func testMissingDuplicateAndNoFrameRefused() throws {
        let base = try makeDraft()
        assertFailure("PROCEDURAL_FRAME_TARGET_MISSING") {
            _ = try NativeProceduralFrameClear.Proposal(command: command("12345678-1234-1234-1234-123456789aaa"), draft: base)
        }
        var duplicate = base, candidate = base.candidate
        var placements = try XCTUnwrap(candidate["placements"].items)
        placements.append(placements[0]); candidate["placements"] = .array(placements)
        try duplicate.replaceCandidate(candidate)
        assertFailure("PROCEDURAL_FRAME_TARGET_DUPLICATE") { _ = try NativeProceduralFrameClear.Proposal(command: command(), draft: duplicate) }
        var absent = base
        try absent.setInstallation(.init(pedestal: .init(kind: .box, position: [1, 0, 2])), for: first)
        assertFailure("PROCEDURAL_FRAME_ABSENT") { _ = try NativeProceduralFrameClear.Proposal(command: command(), draft: absent) }
        var none = base
        try none.setInstallation(.init(frame: .init(kind: .none)), for: first)
        assertFailure("PROCEDURAL_FRAME_ALREADY_NONE") { _ = try NativeProceduralFrameClear.Proposal(command: command(), draft: none) }
    }

    func testModelAttachmentConflictAndMalformedAttachmentsRefused() throws {
        let base = try makeDraft()
        var conflict = base, candidate = base.candidate
        let namespace = WorkspaceFurnishingPlacement.namespace
        candidate["extensions"][namespace] = .object(["version": .integer(4), "furnishings": .array([]),
            "placements": .array([]), "frameAttachments": .array([.object(["placementId": .string(first.uppercased())])])])
        try conflict.replaceCandidate(candidate)
        assertFailure("PROCEDURAL_FRAME_MODEL_CONFLICT") { _ = try NativeProceduralFrameClear.Proposal(command: command(), draft: conflict) }
        candidate["extensions"][namespace]["frameAttachments"] = .string("invalid")
        var malformed = base; try malformed.replaceCandidate(candidate)
        assertFailure("PROCEDURAL_FRAME_ATTACHMENTS_INVALID") { _ = try NativeProceduralFrameClear.Proposal(command: command(), draft: malformed) }
    }

    func testApplyRejectsAnyChangedBaseIncludingNumericRepresentation() throws {
        let base = try makeDraft(), proposal = try NativeProceduralFrameClear.Proposal(command: command(), draft: base)
        var altered = base, candidate = base.candidate
        candidate["title"] = .string("changed after preview")
        try altered.replaceCandidate(candidate)
        assertFailure("PROCEDURAL_FRAME_PROPOSAL_STALE") { _ = try proposal.apply(to: altered) }
        var document = base.document
        document["candidate"]["rights"]["count"] = .number(1)
        let representationChange = try NativeExhibitionDraft(document: document)
        assertFailure("PROCEDURAL_FRAME_PROPOSAL_STALE") { _ = try proposal.apply(to: representationChange) }
    }
}
