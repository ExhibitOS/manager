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
}
