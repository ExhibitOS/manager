// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore
final class PublicationTests: XCTestCase {
    func testPublicationCopyRetainsGeometryRightsAndOpaqueDataWhileChangingOnlyDocumentIdentity() throws {
        var source = try NativeExhibitionDraft.new(title: "제작 원본")
        var candidate = source.candidate
        candidate["extensions"] = .object(["com.example/retained":.object(["note":.string("원문 보존")])])
        try source.replaceCandidate(candidate)
        let original = source
        let pending = try NativePublicationPending(source: source, etag: "\"studio-r2-" + String(repeating: "a", count: 64) + "\"")
        let copy = pending.snapshot
        XCTAssertNotEqual(copy.id, source.id); XCTAssertNotEqual(copy.document["id"], source.document["id"])
        XCTAssertNotEqual(copy.candidate["revisionId"], source.candidate["revisionId"])
        XCTAssertEqual(copy.document["editVersion"].integer, 1)
        for key in ["rooms","surfaces","openings","placements","artworks","lights","accessibility","extensions","mediaAssets","audioZones","annotations","scripts"] { XCTAssertEqual(copy.candidate[key],source.candidate[key]) }
        XCTAssertTrue(copy.title.hasSuffix("공개용 저장본")); XCTAssertEqual(source, original)
        XCTAssertNil(pending.snapshotEtag)
        let decoded = try JSONDecoder().decode(NativePublicationPending.self, from: JSONEncoder().encode(pending)); XCTAssertEqual(decoded, pending)
        XCTAssertTrue(try source.copyForEditing().title.hasSuffix("편집 사본"))
        XCTAssertThrowsError(try NativePublicationPending(source: source, etag: "untrusted"))
    }
    func testPreparedPublicationNeverAcceptsAnExternalOrChangedLocalURL() throws {
        let id = UUID().uuidString.lowercased(), hash = String(repeating: "a", count: 64)
        let valid = NativePublication(publicationId: id, publicUrl: "/p/" + id, revisionSha256: hash, publishedAt: "2026-10-23T00:00:00.000Z", status: "published")
        XCTAssertEqual(try valid.validated(), valid)
        for url in ["https://outside.invalid/p/" + id, "/p/" + UUID().uuidString, "/p/" + id + "?token=untrusted"] {
            XCTAssertThrowsError(try NativePublication(publicationId: id, publicUrl: url, revisionSha256: hash, publishedAt: valid.publishedAt, status: valid.status).validated())
        }
    }
    func testPendingComparisonAcceptsOnlyExactInitialSnapshotWithoutReplacingRequestIdentity() throws {
        var source = try NativeExhibitionDraft.new(title: "비교 원본"), candidate = source.candidate
        candidate["extensions"] = .object(["com.example/precise": .number(0.9559733532492861)])
        try source.replaceCandidate(candidate)
        let pending = try NativePublicationPending(source: source, etag: "\"studio-r7-" + String(repeating: "a", count: 64) + "\"", planBindingSha256: String(repeating: "c", count: 64))
        let etag = "\"studio-r1-" + String(repeating: "b", count: 64) + "\""
        let saved = NativeStudioResponse(revision: 1, etag: etag, draft: pending.snapshot)
        let accepted = try pending.confirmingSavedSnapshot(saved)
        var expected = pending; expected.snapshotEtag = etag
        XCTAssertEqual(accepted, expected, "Only the acknowledged ETag changes; no new draft/request/plan identity")
        XCTAssertEqual(try JSONDecoder().decode(NativePublicationPending.self, from: JSONEncoder().encode(accepted)), expected)
        XCTAssertNil(pending.snapshotEtag, "The original uncertain request remains untouched")
        for invalid in [NativeStudioResponse(revision: 2, etag: etag, draft: pending.snapshot),
                        NativeStudioResponse(revision: 1, etag: "untrusted", draft: pending.snapshot),
                        NativeStudioResponse(revision: 1, etag: "\"studio-r2-" + String(repeating: "b", count: 64) + "\"", draft: pending.snapshot),
                        NativeStudioResponse(revision: 1, etag: etag, draft: source)] {
            XCTAssertThrowsError(try pending.confirmingSavedSnapshot(invalid))
        }
        var changed = pending.snapshot.document
        changed["candidate"]["extensions"]["com.example/precise"] = .number(0.955973353249286)
        XCTAssertThrowsError(try pending.confirmingSavedSnapshot(NativeStudioResponse(revision: 1, etag: etag, draft: NativeExhibitionDraft(document: changed))))
        XCTAssertThrowsError(try accepted.confirmingSavedSnapshot(saved), "An existing acknowledgement cannot be overwritten")
    }
    func testPendingSnapshotBindingRejectsLateGenerationScopeRevisionAndRequestChanges() throws {
        let pending = try NativePublicationPending(source: NativeExhibitionDraft.new(), etag: "\"studio-r7-" + String(repeating: "a", count: 64) + "\"")
        let state = NativePublicationState(format: 1, revision: 7, workspaceId: UUID().uuidString.lowercased(),
            tenantId: UUID().uuidString.lowercased(), principalId: UUID().uuidString.lowercased(), pending: pending, completed: nil)
        let generation = UUID(), binding = try NativePendingPublicationSnapshotBinding(state, generation: generation)
        let etag = "\"studio-r1-" + String(repeating: "b", count: 64) + "\""
        let response = NativeStudioResponse(revision: 1, etag: etag, draft: pending.snapshot)
        var expected = state; expected.pending?.snapshotEtag = etag
        XCTAssertEqual(try binding.accepting(response, in: state, generation: generation), expected)
        XCTAssertThrowsError(try binding.accepting(response, in: state, generation: UUID()))
        var later = state; later.revision += 1
        XCTAssertThrowsError(try binding.accepting(response, in: later, generation: generation))
        later = state; later.pending = nil
        XCTAssertThrowsError(try binding.accepting(response, in: later, generation: generation))
        for key in ["workspaceId", "tenantId", "principalId"] {
            var changed = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(state))
            changed[key] = .string(UUID().uuidString.lowercased())
            let foreign = try JSONDecoder().decode(NativePublicationState.self, from: JSONEncoder().encode(changed))
            XCTAssertThrowsError(try binding.accepting(response, in: foreign, generation: generation))
        }
        for key in ["saveRequestId", "publishRequestId"] {
            var changed = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(state))
            changed["pending"][key] = .string(UUID().uuidString.lowercased())
            let foreign = try JSONDecoder().decode(NativePublicationState.self, from: JSONEncoder().encode(changed))
            XCTAssertThrowsError(try binding.accepting(response, in: foreign, generation: generation))
        }
    }
    func testReusableDraftIntentRetainsExactIdentityAndRequestAcrossRestart() throws {
        let draft = try NativeExhibitionDraft.new(), etag = "\"studio-r2-" + String(repeating: "a", count: 64) + "\""
        let pending = try NativePublicationPending(reusableSource: draft, etag: etag, capabilityPublicationId: UUID().uuidString.lowercased())
        XCTAssertEqual(pending.mode, "reusable-draft-v1")
        XCTAssertEqual(pending.snapshot, draft)
        XCTAssertEqual(pending.sourceExhibitionId, draft.id)
        XCTAssertEqual(pending.snapshotEtag, etag)
        let round = try JSONDecoder().decode(NativePublicationPending.self, from: JSONEncoder().encode(pending))
        XCTAssertEqual(round, pending)
        let id = UUID().uuidString.lowercased()
        let withdrawn = NativePublication(publicationId: id, publicUrl: "/p/" + id, revisionSha256: String(repeating: "c", count: 64), publishedAt: "2026-10-30T00:00:00Z", status: "unpublished")
        let completed = NativePublicationCompleted(pending: round, publication: withdrawn)
        XCTAssertEqual(completed.mode, "reusable-draft-v1")
        XCTAssertEqual(completed.sourceExhibitionId, completed.snapshotExhibitionId)
        XCTAssertFalse(completed.isReusable(sourceExhibitionId: draft.id, etag: etag, planBindingSha256: nil))
        XCTAssertThrowsError(try pending.confirmingSavedSnapshot(NativeStudioResponse(revision: 1, etag: etag, draft: draft)))
    }

}
