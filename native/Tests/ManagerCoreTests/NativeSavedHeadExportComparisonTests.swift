// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class NativeSavedHeadExportComparisonTests: XCTestCase {
    private let owner = "11111111-1111-4111-8111-111111111111"
    private func state(_ title: String, revision: UInt64) throws -> NativeDocumentState {
        let draft = try NativeExhibitionDraft.new(title: title)
        return NativeDocumentState(format: 1, revision: revision, workspaceId: owner, tenantId: owner,
            principalId: owner, draft: draft, etag: "\"studio-r16-" + String(repeating: "a", count: 64) + "\"",
            savedEditVersion: draft.document["editVersion"].integer, pending: nil)
    }
    func testPrivateHeadAndProtectedOfficialUseTheirOwnRevisionDomains() throws {
        let selected = try state("Official", revision: 304), host = try state("Private", revision: 17)
        let before = try NativePrivateHost.bytes(selected)
        let head = try NativeSavedHeadExportComparison.head(current: host, protecting: selected,
            requestId: owner, revisionSHA256: String(repeating: "b", count: 64))
        XCTAssertEqual(head["exhibitionId"], .string(host.draft!.id))
        XCTAssertEqual(head["revision"], .integer(17))
        XCTAssertEqual(head["protectedDocument"]["exhibitionId"], .string(selected.draft!.id))
        XCTAssertEqual(head["protectedDocument"]["revision"], .integer(304))
        XCTAssertEqual(try NativePrivateHost.bytes(selected), before)
    }
    func testMissingUnsavedSameOfficialForeignAndOverflowContextsAreRejected() throws {
        let selected = try state("Official", revision: 304), host = try state("Private", revision: 17)
        func rejects(_ current: NativeDocumentState, _ protected: NativeDocumentState) {
            XCTAssertThrowsError(try NativeSavedHeadExportComparison.head(current: current, protecting: protected,
                requestId: owner, revisionSHA256: String(repeating: "b", count: 64)))
        }
        rejects(selected, selected)
        var missing = selected; missing.etag = nil; rejects(host, missing)
        var unsaved = selected; unsaved.savedEditVersion = nil; rejects(host, unsaved)
        var pending = selected; pending.pending = .init(etag: selected.etag, draft: selected.draft!); rejects(host, pending)
        var huge = host; huge.revision = UInt64.max; rejects(huge, selected)
        let foreign = NativeDocumentState(format: 1, revision: 304, workspaceId: owner, tenantId: owner,
            principalId: UUID().uuidString.lowercased(), draft: selected.draft, etag: selected.etag,
            savedEditVersion: selected.savedEditVersion, pending: nil)
        rejects(host, foreign)
    }
    func testOrdinarySavedHeadPreservesExistingWireAndExactRuntimeGate() throws {
        let host = try state("Saved", revision: 17)
        let head = try NativeSavedHeadExportComparison.head(current: host, protecting: nil,
            requestId: owner, revisionSHA256: String(repeating: "b", count: 64))
        XCTAssertEqual(head["protectedDocument"], .null)
        for manifest in ["1b542833230e3f80c3850b09fb677cc119415a04b15e53c4904942eb76b5a238",
            "0f04520a8742f9774206d12c8a2b56da8ed2a4e07c095b3cb8ad3b3e988dd988",
            "74bd7aaa8500a4bd52240827b9c6a2da0e22b2757111bb57a3b05c969ea308be"] {
            XCTAssertTrue(AuthoringAPI.supportsOrdinarySavedHeadStaticRuntime(manifest: manifest))
        }
        XCTAssertFalse(AuthoringAPI.supportsOrdinarySavedHeadStaticRuntime(manifest: String(repeating: "f", count: 64)))
        XCTAssertNil(try NativeAutomationSession.replayResponse(for: "private-exhibition-export-static",
            prior: (Data(), .object([:])), matching: Data()))
    }
}
