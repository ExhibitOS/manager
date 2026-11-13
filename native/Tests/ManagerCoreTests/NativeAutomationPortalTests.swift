// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class NativeAutomationPortalTests: XCTestCase {
    private var command: DocumentValue {
        .object(["openingId": .string("10000000-0000-4000-8000-000000000001"),
                 "furnishingId": .string("10000000-0000-4000-8000-000000000002"),
                 "furnishingVersion": .integer(1), "modelSha256": .string(String(repeating: "a", count: 64))])
    }
    func testPortalCommandRejectsCandidateInjectionAndUnsupportedIdentity() throws {
        _ = try NativeAutomationPortal.Command(command)
        var invalid = command; invalid["candidate"] = .object([:])
        XCTAssertThrowsError(try NativeAutomationPortal.Command(invalid))
        for (key, value) in [(String, DocumentValue)]([
            ("openingId", .string("missing")), ("furnishingId", .string("missing")),
            ("furnishingVersion", .integer(2)), ("modelSha256", .string(String(repeating: "A", count: 64)))]) {
            invalid = command; invalid[key] = value
            XCTAssertThrowsError(try NativeAutomationPortal.Command(invalid))
        }
    }
    func testProposalBindsFullLocalStateAndFreshCandidate() throws {
        let draft = try NativeExhibitionDraft.new()
        let base: DocumentValue = .object(["revision": .integer(7), "etag": .string("saved-head"),
            "tenantId": .string("original"), "pending": .null])
        let proposal = NativeAutomationPortal.Proposal(command: try .init(command), base: base, draft: draft)
        try proposal.check(current: base, refreshed: draft)
        for (key, value) in [(String, DocumentValue)]([
            ("revision", .integer(8)), ("etag", .string("new-head")),
            ("tenantId", .string("other")), ("pending", .object([:]))]) {
            var changed = base; changed[key] = value
            XCTAssertThrowsError(try proposal.check(current: changed, refreshed: draft))
        }
        var changed = draft; var candidate = draft.candidate
        candidate["title"] = .string("changed after preview")
        try changed.replaceCandidate(candidate)
        XCTAssertThrowsError(try proposal.check(current: base, refreshed: changed))
        XCTAssertThrowsError(try proposal.check(current: base, refreshed: nil))
        XCTAssertEqual(proposal.draft, draft)
    }
    func testRepreparationAcceptsOnlyRegeneratedEnvelopeTimestamp() throws {
        let draft = try NativeExhibitionDraft.new(now: Date(timeIntervalSince1970: 1_000))
        let base: DocumentValue = .object(["revision": .integer(7)])
        let proposal = NativeAutomationPortal.Proposal(command: try .init(command), base: base, draft: draft)
        var document = draft.document
        document["updatedAt"] = .string("2030-01-01T00:00:00Z")
        try proposal.check(current: base, refreshed: NativeExhibitionDraft(document: document))
        for (key, value) in [(String, DocumentValue)]([
            ("id", .string(UUID().uuidString.lowercased())), ("editVersion", .integer(2)),
            ("createdAt", .string("2030-01-01T00:00:00Z"))]) {
            var changed = document; changed[key] = value
            XCTAssertThrowsError(try proposal.check(current: base, refreshed: NativeExhibitionDraft(document: changed)))
        }
        var changed = document; changed["candidate"]["title"] = .string("different candidate")
        XCTAssertThrowsError(try proposal.check(current: base, refreshed: NativeExhibitionDraft(document: changed)))
        // A timestamp change in the actual local base still expires the proposal.
        var changedBase = base; changedBase["updatedAt"] = .string("2030-01-01T00:00:00Z")
        XCTAssertThrowsError(try proposal.check(current: changedBase, refreshed: draft))
    }
    func testNoChangeProposalCannotApplyDifferentDelta() throws {
        let base: DocumentValue = .object(["revision": .integer(7)])
        let proposal = NativeAutomationPortal.Proposal(command: try .init(command), base: base, draft: nil)
        try proposal.check(current: base, refreshed: nil)
        XCTAssertThrowsError(try proposal.check(current: base, refreshed: .new()))
    }
}
