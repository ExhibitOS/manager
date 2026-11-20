// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class RecordedFloorProposalTests: XCTestCase {
    private let owner = "10000000-0000-4000-8000-000000000001"

    private func document(_ draft: NativeExhibitionDraft) -> NativeDocumentState {
        NativeDocumentState(format: 1, revision: 1, workspaceId: owner, tenantId: owner, principalId: owner,
                            draft: draft, etag: "\"studio-r1-\(String(repeating: "a", count: 64))\"",
                            savedEditVersion: draft.document["editVersion"].integer, pending: nil)
    }

    private func reply(_ proposal: RecordedFloorProposal, candidate: DocumentValue,
                       status: RecordedFloorStatus.State) -> DocumentValue {
        var result: DocumentValue = .object([:])
        for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"] {
            result[key] = proposal.value[key]
        }
        result["candidate"] = candidate
        result["floorRecording"] = .object(["roomId": .string(proposal.roomId), "state": .string(status.rawValue)])
        return result
    }

    func testWholeChoiceChecksExactTargetAndPreservesAuthoredMute() throws {
        var draft = try NativeExhibitionDraft.new()
        let target = FloorSettings.Target(kind: .roomBase, id: try XCTUnwrap(draft.rooms.first?["id"].string))
        let defaults = FloorSettings.defaults(.wood)
        let wood = try FloorSettings.applying(preset: .wood, defaults: defaults, fields: Set(defaults.keys),
                                             target: target, prior: nil, candidate: draft.candidate)
        let muted = try wood.replacing(.sound, with: .object(["mode": .string("none"), "gain": .number(0.6)]),
                                      candidate: draft.candidate)
        try draft.setFloor(muted, target: target)
        let proposal = try RecordedFloorProposal(command: .select(target, .tile), document: document(draft))
        let tileDefaults = FloorSettings.defaults(.tile)
        let tile = try FloorSettings.applying(preset: .tile, defaults: tileDefaults, fields: Set(tileDefaults.keys),
                                             target: target, prior: nil, candidate: draft.candidate)
        var invalid = draft
        try invalid.setFloor(tile, target: target, declareDefaultSound: true)
        XCTAssertThrowsError(try proposal.checkedReply(reply(proposal, candidate: invalid.candidate, status: .recorded)))

        let mutedTile = try tile.replacing(.sound, with: muted.value["sound"], candidate: draft.candidate)
        var expected = draft
        try expected.setFloor(mutedTile, target: target)
        let accepted = try proposal.checkedReply(reply(proposal, candidate: expected.candidate, status: .muted))
        XCTAssertEqual(try accepted.applying(to: draft), expected)
        var unrelated = expected.candidate
        unrelated["title"] = .string("Changed separately")
        XCTAssertThrowsError(try proposal.checkedReply(reply(proposal, candidate: unrelated, status: .muted)))
    }

    func testResetWithoutRecordedMaterialCanRemainAnUnassignedNoOp() throws {
        let draft = try NativeExhibitionDraft.new()
        let target = FloorSettings.Target(kind: .roomBase, id: try XCTUnwrap(draft.rooms.first?["id"].string))
        let proposal = try RecordedFloorProposal(command: .reset(target), document: document(draft))
        let accepted = try proposal.checkedReply(reply(proposal, candidate: draft.candidate, status: .unassigned))
        XCTAssertEqual(accepted.recording.state, .unassigned)
        XCTAssertEqual(try accepted.applying(to: draft), draft)
    }
}
