// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class PublicationStatusTests: XCTestCase {
    private func publication(id: String = "00000000-0000-4000-8000-000000000001", hash: String = String(repeating: "a", count: 64), date: String = "2026-10-29T00:00:00Z", status: String = "published") -> NativePublication {
        NativePublication(publicationId: id, publicUrl: "/p/" + id, revisionSha256: hash, publishedAt: date, status: status)
    }
    private func state() throws -> NativePublicationState {
        let pending = try NativePublicationPending(source: NativeExhibitionDraft.new(title: "해제 검증 원본"), etag: "\"studio-r2-" + String(repeating: "b", count: 64) + "\"")
        return NativePublicationState(format: 1, revision: 7, workspaceId: UUID().uuidString, tenantId: UUID().uuidString, principalId: UUID().uuidString, pending: nil, completed: NativePublicationCompleted(pending: pending, publication: publication()))
    }
    func testStatusResponseCannotChangePublicationRevisionURLOrTimestamp() throws {
        let expected = publication(), released = publication(status: "unpublished")
        XCTAssertEqual(try expected.acceptingStatus(released, requiredStatus: "unpublished"), released)
        for wrong in [publication(id: UUID().uuidString), publication(hash: String(repeating: "c", count: 64)), publication(date: "2026-10-29T00:01:00Z"), publication(), publication(status: "unknown")] {
            XCTAssertThrowsError(try expected.acceptingStatus(wrong, requiredStatus: "unpublished"))
        }
        let external = NativePublication(publicationId: expected.publicationId, publicUrl: "https://outside.invalid/p/" + expected.publicationId, revisionSha256: expected.revisionSha256, publishedAt: expected.publishedAt, status: "unpublished")
        XCTAssertThrowsError(try expected.acceptingStatus(external))
    }
    func testLateStatusCannotReplaceAnotherJournalRevisionPendingRequestOrPreparation() throws {
        let original = try state(), binding = try NativePublicationStatusBinding(original)
        let released = publication(status: "unpublished")
        let accepted = try binding.replacingPublication(released, in: original)
        XCTAssertEqual(accepted.completed?.publication, released)
        XCTAssertEqual(accepted.revision, original.revision)
        XCTAssertEqual(original.completed?.publication.status, "published")
        var newer = original; newer.revision += 1
        XCTAssertThrowsError(try binding.replacingPublication(released, in: newer))
        newer = original; newer.pending = try NativePublicationPending(source: NativeExhibitionDraft.new(), etag: "\"studio-r1-" + String(repeating: "d", count: 64) + "\"")
        XCTAssertThrowsError(try binding.replacingPublication(released, in: newer))
        XCTAssertThrowsError(try NativePublicationStatusBinding(newer))
        newer = original; newer.completed = try state().completed
        XCTAssertThrowsError(try binding.replacingPublication(released, in: newer))
        let otherPrincipal = NativePublicationState(format: original.format, revision: original.revision, workspaceId: original.workspaceId, tenantId: original.tenantId, principalId: UUID().uuidString, pending: nil, completed: original.completed)
        XCTAssertThrowsError(try binding.replacingPublication(released, in: otherPrincipal))
        XCTAssertEqual(try JSONDecoder().decode(NativePublicationState.self, from: JSONEncoder().encode(accepted)), accepted)
    }
    func testReleasedCopyStillIdentifiesItsSourceButCannotBeReusedForOutput() throws {
        let original = try state(), binding = try NativePublicationStatusBinding(original)
        let changed = try XCTUnwrap(binding.replacingPublication(publication(status: "unpublished"), in: original).completed)
        XCTAssertTrue(changed.refersToSource(sourceExhibitionId: changed.sourceExhibitionId, etag: changed.sourceEtag, planBindingSha256: changed.planBindingSha256))
        XCTAssertFalse(changed.isReusable(sourceExhibitionId: changed.sourceExhibitionId, etag: changed.sourceEtag, planBindingSha256: changed.planBindingSha256))
        XCTAssertFalse(changed.refersToSource(sourceExhibitionId: UUID().uuidString, etag: changed.sourceEtag, planBindingSha256: changed.planBindingSha256))
    }
}
