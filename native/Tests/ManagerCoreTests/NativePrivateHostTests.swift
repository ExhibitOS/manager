// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class NativePrivateHostTests: XCTestCase {
    private func uuid() -> String { UUID().uuidString.lowercased() }
    private func etag(_ revision: Int, _ letter: String = "a") -> String {
        "\"studio-r\(revision)-\(String(repeating: letter, count: 64))\""
    }
    private func protected() throws -> NativeDocumentState {
        let draft = try NativeExhibitionDraft.new(title: "Protected")
        return NativeDocumentState(format: 1, revision: 304, workspaceId: uuid(), tenantId: uuid(),
            principalId: uuid(), draft: draft, etag: etag(7),
            savedEditVersion: draft.document["editVersion"].integer, pending: nil)
    }
    private func makeHost(_ current: NativeDocumentState) throws -> NativePrivateHost {
        try NativePrivateHost(requestId: uuid(), title: "Private", protecting: current, roomBaseLight: false)
    }
    private func response(_ host: NativePrivateHost, revision: Int = 1) throws -> NativeStudioResponse {
        NativeStudioResponse(revision: revision, etag: etag(revision), draft: try XCTUnwrap(host.document.draft))
    }
    func testProtected304IsPreservedAcrossCreatePrepareAndAccept() throws {
        let current = try protected(), before = try NativePrivateHost.bytes(current)
        var host = try makeHost(current)
        XCTAssertNotEqual(host.document.draft?.id, current.draft?.id)
        XCTAssertEqual(host.document.workspaceId, current.workspaceId)
        XCTAssertNoThrow(try host.checkProtected(current))
        let pending = try host.prepareSave(requestId: uuid())
        XCTAssertNil(pending.etag)
        XCTAssertEqual(pending.draft, host.document.draft)
        XCTAssertNoThrow(try host.checkProtected(current))
        let saved = try response(host)
        try host.acceptSaved(saved)
        XCTAssertFalse(host.document.dirty)
        XCTAssertNil(host.document.pending)
        XCTAssertNoThrow(try host.checkProtected(current))
        XCTAssertEqual(try NativePrivateHost.bytes(current), before)
        XCTAssertNoThrow(try host.checkRemote(saved))
        XCTAssertNoThrow(try host.requireSaved(saved))
    }
    func testForeignNamespaceAndCurrentDocumentChangesAreRejected() throws {
        let current = try protected(), host = try makeHost(current)
        var foreign = current
        foreign = NativeDocumentState(format: 1, revision: current.revision, workspaceId: uuid(),
            tenantId: current.tenantId, principalId: current.principalId, draft: current.draft,
            etag: current.etag, savedEditVersion: current.savedEditVersion, pending: nil)
        XCTAssertThrowsError(try host.checkProtected(foreign))
        var changed = current; changed.revision += 1
        XCTAssertThrowsError(try host.checkProtected(changed))
        changed = current; changed.etag = etag(8)
        XCTAssertThrowsError(try host.checkProtected(changed))
        changed = current
        var alteredDraft = try XCTUnwrap(changed.draft); try alteredDraft.rename("Changed protected document")
        changed.draft = alteredDraft; changed.savedEditVersion = alteredDraft.document["editVersion"].integer
        XCTAssertFalse(changed.dirty)
        XCTAssertThrowsError(try host.checkProtected(changed))
        var dirty = current; dirty.savedEditVersion = 0
        XCTAssertThrowsError(try makeHost(dirty))
        XCTAssertThrowsError(try NativePrivateHost(requestId: "not-a-uuid", title: "Private", protecting: current, roomBaseLight: false))
    }
    func testUncertainSaveRetainsExactRequestAndRejectsDifferentRequest() throws {
        let current = try protected(); var host = try makeHost(current)
        let request = uuid(), first = try host.prepareSave(requestId: request)
        let repeatPending = try host.prepareSave(requestId: request)
        XCTAssertEqual(repeatPending.requestId, first.requestId)
        XCTAssertEqual(repeatPending.draft, first.draft)
        XCTAssertEqual(repeatPending.etag, first.etag)
        XCTAssertThrowsError(try host.prepareSave(requestId: uuid()))
        XCTAssertEqual(host.document.pending?.requestId, request)
        XCTAssertEqual(host.document.pending?.draft, first.draft)
        XCTAssertEqual(host.document.pending?.etag, first.etag)
        XCTAssertNoThrow(try host.checkProtected(current))
        let saved = try response(host); try host.acceptSaved(saved)
        var changed = try XCTUnwrap(host.document.draft); try changed.rename("Retry after prior save")
        try host.stage(changed)
        let nextRequest = uuid(), next = try host.prepareSave(requestId: nextRequest)
        XCTAssertEqual(next.etag, etag(1))
        XCTAssertEqual(try host.prepareSave(requestId: nextRequest).etag, next.etag)
        XCTAssertThrowsError(try host.prepareSave(requestId: uuid()))
        XCTAssertEqual(host.document.pending?.draft, changed)
        XCTAssertEqual(host.document.pending?.etag, etag(1))
    }
    func testInvalidResponsesPreservePendingAndRejectStaleTargets() throws {
        let current = try protected(); var host = try makeHost(current)
        let request = uuid(), pending = try host.prepareSave(requestId: request)
        let foreign = try NativeExhibitionDraft.new(title: "Foreign")
        for value in [NativeStudioResponse(revision: 1, etag: etag(1), draft: foreign),
                      NativeStudioResponse(revision: 2, etag: etag(2), draft: pending.draft),
                      NativeStudioResponse(revision: 1, etag: "invalid", draft: pending.draft),
                      NativeStudioResponse(revision: 1, etag: etag(2), draft: pending.draft)] {
            XCTAssertThrowsError(try host.acceptSaved(value))
            XCTAssertEqual(host.document.pending?.requestId, request)
            XCTAssertEqual(host.document.pending?.draft, pending.draft)
            XCTAssertEqual(host.document.pending?.etag, pending.etag)
        }
        let privateID = try XCTUnwrap(host.document.draft?.id)
        XCTAssertNoThrow(try host.checkTarget(id: privateID, revision: host.document.revision))
        XCTAssertThrowsError(try host.checkTarget(id: uuid(), revision: host.document.revision))
        XCTAssertThrowsError(try host.checkTarget(id: privateID, revision: host.document.revision + 1))
        XCTAssertThrowsError(try host.stage(foreign))
    }
    func testStagedChangesKeepProtectedStateAndRejectConflictingRemote() throws {
        let current = try protected(), before = try NativePrivateHost.bytes(current)
        var host = try makeHost(current)
        _ = try host.prepareSave(requestId: uuid())
        let saved = try response(host); try host.acceptSaved(saved)
        let foreign = try NativeExhibitionDraft.new(title: "Foreign")
        XCTAssertThrowsError(try host.stage(foreign))
        var edited = try XCTUnwrap(host.document.draft); try edited.rename("Edited private")
        let oldRevision = host.document.revision
        try host.stage(edited)
        XCTAssertTrue(host.document.dirty)
        XCTAssertThrowsError(try host.checkTarget(id: edited.id, revision: oldRevision))
        XCTAssertNoThrow(try host.checkRemote(saved))
        XCTAssertThrowsError(try host.requireSaved(saved))
        let conflict = NativeStudioResponse(revision: 2, etag: etag(2, "b"), draft: saved.draft)
        XCTAssertThrowsError(try host.checkRemote(conflict))
        XCTAssertThrowsError(try host.requireSaved(conflict))
        XCTAssertNoThrow(try host.checkProtected(current))
        XCTAssertEqual(try NativePrivateHost.bytes(current), before)
    }
}
