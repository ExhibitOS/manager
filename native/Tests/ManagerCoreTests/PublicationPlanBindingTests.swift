// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class PublicationPlanBindingTests: XCTestCase {
    private let etag = "\"studio-r2-" + String(repeating: "a", count: 64) + "\""
    private let runtime = String(repeating: "b", count: 64)
    private func binding(_ draft: NativeExhibitionDraft, etag: String? = nil, runtime: String? = nil, watermark: PublicationWatermark? = nil) throws -> String {
        try nativePublicationPlanBinding(sourceExhibitionId: draft.id, etag: etag ?? self.etag, runtimeManifestSha256: runtime ?? self.runtime, watermark: watermark)
    }
    private func completed(_ pending: NativePublicationPending) -> NativePublicationCompleted {
        let id = UUID().uuidString.lowercased()
        return NativePublicationCompleted(pending: pending, publication: NativePublication(publicationId: id, publicUrl: "/p/" + id, revisionSha256: String(repeating: "c", count: 64), publishedAt: "2026-10-28T00:00:00Z", status: "published"))
    }
    func testBoundedProfileCommitmentSurvivesColdPendingWithoutChangingLegacyHash() throws {
        let fixedID = "00000000-0000-4000-8000-000000000001"
        let legacy = try nativePublicationPlanBinding(sourceExhibitionId: fixedID, etag: etag, runtimeManifestSha256: runtime, watermark: nil)
        XCTAssertEqual(legacy, "b48cbcd3783272fc1a00e084451ef8461d305a42bf02241b8e78a10877fe34c5")
        let draft = try NativeExhibitionDraft.new(title: "프로필을 보관하는 전시")
        let bounded = try nativePublicationPlanBinding(sourceExhibitionId: draft.id, etag: etag, runtimeManifestSha256: runtime, watermark: nil, publicationProfile: "bounded-local-v1")
        XCTAssertNotEqual(bounded, try binding(draft))
        let pending = try NativePublicationPending(source: draft, etag: etag, planBindingSha256: bounded)
        let cold = try JSONDecoder().decode(NativePublicationPending.self, from: JSONEncoder().encode(pending))
        XCTAssertEqual(cold.saveRequestId, pending.saveRequestId)
        XCTAssertEqual(cold.publishRequestId, pending.publishRequestId)
        XCTAssertEqual(cold.snapshot, pending.snapshot)
        try cold.requirePlanBinding(nativePublicationPlanBinding(sourceExhibitionId: cold.sourceExhibitionId, etag: cold.sourceEtag, runtimeManifestSha256: runtime, watermark: cold.snapshot.publicationWatermark(), publicationProfile: "bounded-local-v1"))
        XCTAssertThrowsError(try cold.requirePlanBinding(binding(draft)))
        XCTAssertThrowsError(try nativePublicationPlanBinding(sourceExhibitionId: draft.id, etag: etag, runtimeManifestSha256: runtime, watermark: nil, publicationProfile: "unrecognized"))
    }
    func testEffectiveRecipeIgnoresInactiveNoneFieldsButBindsRuntimeSourceAndSettings() throws {
        let draft = try NativeExhibitionDraft.new(title: "전시")
        let absent = try binding(draft)
        XCTAssertEqual(absent, try binding(draft, watermark: PublicationWatermark(mode: .none, text: "unused", position: .topLeft, size: 24, opacity: 0.6)))
        XCTAssertNotEqual(absent, try binding(draft, runtime: String(repeating: "d", count: 64)))
        XCTAssertNotEqual(absent, try binding(draft, etag: "\"studio-r3-" + String(repeating: "a", count: 64) + "\""))
        XCTAssertNotEqual(absent, try binding(NativeExhibitionDraft.new(title: "전시")))
        let overlay = PublicationWatermark(mode: .overlay, text: " 작가 🖼️ ")
        XCTAssertEqual(try binding(draft, watermark: overlay), try binding(draft, watermark: PublicationWatermark(mode: .overlay, text: "작가 🖼️")))
        XCTAssertNotEqual(absent, try binding(draft, watermark: overlay))
        XCTAssertNotEqual(try binding(draft, watermark: overlay), try binding(draft, watermark: PublicationWatermark(mode: .overlay, text: "작가 🖼️", size: 13)))
    }
    func testLegacyCompletedRemainsReadableButCannotSatisfyQualifiedPlan() throws {
        let draft = try NativeExhibitionDraft.new(title: "기존 저장본")
        let legacy = completed(try NativePublicationPending(source: draft, etag: etag))
        let bytes = try JSONEncoder().encode(legacy)
        XCTAssertNil((try JSONSerialization.jsonObject(with: bytes) as? [String: Any])?["planBindingSha256"])
        let reopened = try JSONDecoder().decode(NativePublicationCompleted.self, from: bytes)
        XCTAssertEqual(reopened.publication.publicationId, legacy.publication.publicationId)
        XCTAssertTrue(reopened.isReusable(sourceExhibitionId: draft.id, etag: etag, planBindingSha256: nil))
        XCTAssertFalse(reopened.isReusable(sourceExhibitionId: draft.id, etag: etag, planBindingSha256: try binding(draft)))
        let bound = completed(try NativePublicationPending(source: draft, etag: etag, planBindingSha256: binding(draft)))
        let cold = try JSONDecoder().decode(NativePublicationCompleted.self, from: JSONEncoder().encode(bound))
        XCTAssertTrue(cold.isReusable(sourceExhibitionId: draft.id, etag: etag, planBindingSha256: try binding(draft)))
        XCTAssertFalse(cold.isReusable(sourceExhibitionId: draft.id, etag: etag, planBindingSha256: nil))
    }
    func testUnresolvedPendingRetainsExactRequestsAndSnapshotOnBindingMismatch() throws {
        let draft = try NativeExhibitionDraft.new(title: "보관 요청")
        let pending = try NativePublicationPending(source: draft, etag: etag, planBindingSha256: binding(draft))
        let reopened = try JSONDecoder().decode(NativePublicationPending.self, from: JSONEncoder().encode(pending))
        try reopened.requirePlanBinding(binding(draft))
        XCTAssertThrowsError(try reopened.requirePlanBinding(binding(draft, runtime: String(repeating: "d", count: 64))))
        XCTAssertThrowsError(try reopened.requirePlanBinding(nil))
        XCTAssertEqual(reopened, pending)
        let legacy = try NativePublicationPending(source: draft, etag: etag)
        XCTAssertThrowsError(try legacy.requirePlanBinding(binding(draft)))
        XCTAssertThrowsError(try NativePublicationPending(source: draft, etag: etag, planBindingSha256: "invalid"))
    }
}
