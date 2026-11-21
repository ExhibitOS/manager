// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class NativePairedWallStructureTests: XCTestCase {
    private let namespace = "org.exhibitos.studio/paired-wall-structure"
    private let first = "10000000-0000-4000-8000-000000000001"
    private let peer = "10000000-0000-4000-8000-000000000002"

    private func draft() throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new()
        var candidate = draft.candidate
        candidate["extensions"] = .object(["retained": .object(["rights": .string("unchanged")])])
        try draft.replaceCandidate(candidate)
        return draft
    }

    private func target(installed: Double? = nil) -> NativePairedWallStructure.Target {
        .init(openingId: first, peerOpeningId: peer, peerRoomName: "Other room",
              side: .positive, extensionMeters: installed)
    }

    private func request(_ command: NativePairedWallStructure, draft: NativeExhibitionDraft) -> DocumentValue {
        let owner = "10000000-0000-4000-8000-000000000003"
        let state = NativeDocumentState(format: 1, revision: 5, workspaceId: owner, tenantId: owner,
                                        principalId: owner, draft: draft, etag: nil,
                                        savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        return command.request(in: state, requestId: "10000000-0000-4000-8000-000000000004")
    }

    private func reply(_ request: DocumentValue, candidate: DocumentValue) -> DocumentValue {
        var reply = Dictionary(uniqueKeysWithValues:
            ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"].map { ($0, request[$0]) })
        reply["candidate"] = candidate
        return .object(reply)
    }

    func testSetAndRemovePreserveFullCandidateOutsideExactPair() throws {
        let base = try draft()
        let set = try NativePairedWallStructure(target: target(), extensionMeters: 0.11)
        let setRequest = request(set, draft: base)
        XCTAssertEqual(setRequest["type"], .string("set-paired-wall-structure"))
        var changed = base.candidate
        changed["extensions"][namespace] = .object(["version": .integer(1), "pairs": .array([.object([
            "openingId": .string(first), "peerOpeningId": .string(peer),
            "side": .string("positive-normal"), "extensionMeters": .number(0.11)
        ])])])
        let staged = try XCTUnwrap(set.checkedReply(reply(setRequest, candidate: changed), request: setRequest, draft: base))
        XCTAssertEqual(staged.candidate, changed)
        XCTAssertEqual(staged.document["id"], base.document["id"])
        let remove = try NativePairedWallStructure(target: target(installed: 0.11), extensionMeters: nil)
        let removeRequest = request(remove, draft: staged)
        XCTAssertEqual(removeRequest["type"], .string("remove-paired-wall-structure"))
        XCTAssertEqual(removeRequest["extensionMeters"], .null)
        let restored = try XCTUnwrap(remove.checkedReply(reply(removeRequest, candidate: base.candidate), request: removeRequest, draft: staged))
        XCTAssertEqual(restored.candidate, base.candidate)
        XCTAssertGreaterThan(restored.document["editVersion"].integer!, staged.document["editVersion"].integer!)
    }

    func testRefusesUnscopedOrMismatchedReply() throws {
        let base = try draft()
        let command = try NativePairedWallStructure(target: target(), extensionMeters: 0.11)
        let wire = request(command, draft: base)
        var candidate = base.candidate
        candidate["extensions"][namespace] = .object(["version": .integer(1), "pairs": .array([.object([
            "openingId": .string(first), "peerOpeningId": .string(peer),
            "side": .string("positive-normal"), "extensionMeters": .number(0.11)
        ])])])
        var badBinding = reply(wire, candidate: candidate)
        badBinding["sequence"] = .integer(2)
        XCTAssertThrowsError(try command.checkedReply(badBinding, request: wire, draft: base))
        candidate["extensions"]["retained"]["rights"] = .string("changed")
        XCTAssertThrowsError(try command.checkedReply(reply(wire, candidate: candidate), request: wire, draft: base))
        XCTAssertThrowsError(try NativePairedWallStructure(target: target(), extensionMeters: 0))
        XCTAssertThrowsError(try NativePairedWallStructure(target: target(), extensionMeters: 0.121))
        XCTAssertThrowsError(try NativePairedWallStructure(target: target(), extensionMeters: nil))
    }
}
