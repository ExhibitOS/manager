// SPDX-License-Identifier: Apache-2.0
import XCTest
import Foundation
@testable import ManagerCore
@testable import ManagerApp

@MainActor
final class FurnishingThumbnailReviewLifecycleTests: XCTestCase {
    private func fixture() throws -> (NativeDocumentState, WorkspaceFurnishing, WorkspaceFurnishingThumbnailSubmission) {
        let draft = try NativeExhibitionDraft.new()
        let owner = NativeDocumentState(format: 1, revision: 4, workspaceId: UUID().uuidString.lowercased(),
            tenantId: UUID().uuidString.lowercased(), principalId: UUID().uuidString.lowercased(), draft: draft,
            etag: "synthetic-etag", savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        let encoder = JSONEncoder()
        let rights = try JSONDecoder().decode(DocumentValue.self, from: encoder.encode(ArtworkMetadata.Rights()))
        let descriptor: DocumentValue = .object(["id": .string(UUID().uuidString.lowercased()), "version": .integer(1),
            "kind": .string("bench"), "name": .string("Synthetic"), "provenance": .string("Synthetic fixture"), "rights": rights,
            "model": .object(["mime": .string("model/gltf-binary"), "bytes": .integer(128), "sha256": .string(String(repeating: "a", count: 64))])])
        let item = try JSONDecoder().decode(WorkspaceFurnishing.self, from: encoder.encode(descriptor))
        let frame = ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 1, tenantId: owner.tenantId,
            principalId: owner.principalId, draft: draft, selection: nil, mode: "translate", view: "isometric", disabled: false,
            workspaceId: owner.workspaceId, baseETag: owner.etag)
        let request = try JSONDecoder().decode(DocumentValue.self, from: encoder.encode(ViewportFurnishingThumbnailRequest(frame: frame, furnishing: item)))
        let encoded = "iVBORw0KGgoAAAANSUhEUgAAAQAAAAEACAYAAABccqhmAAABFUlEQVR4nO3BMQEAAADCoPVP7WsIoAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAeAMBPAABPO1TCQAAAABJRU5ErkJggg=="
        let bytes = try XCTUnwrap(Data(base64Encoded: encoded))
        let capture: DocumentValue = .object(["workspaceId": .string(owner.workspaceId), "nativeRevision": .integer(4),
            "editVersion": draft.document["editVersion"], "baseETag": .string("synthetic-etag"), "request": request,
            "png": .object(["mime": .string("image/png"), "width": .integer(256), "height": .integer(256),
                "bytes": .integer(Int64(bytes.count)), "sha256": .string(NativeAutomationSession.digest(bytes)), "base64": .string(encoded)])])
        return (owner, item, try WorkspaceFurnishingThumbnailSubmission(capture: capture, document: owner, furnishing: item))
    }
    private func api(_ owner: NativeDocumentState) throws -> AuthoringAPI {
        // Deliberately unconnected: this exercises retained intent on refusal
        // without a live server, profile, renderer or HTTP request.
        try AuthoringAPI(port: 18091, tenantId: owner.tenantId)
    }
    func testSheetReopenRetainsOriginalCaptureAndRebindDoesNotRenderAgain() async throws {
        let (owner, item, submission) = try fixture(); var captures = 0
        let model = FurnishingThumbnailReviewModel(name: item.details.name, furnishingId: item.id, owner: owner, api: try api(owner),
            document: { owner }, capture: { captures += 1; return submission }, refreshed: {})
        let sessions = FurnishingThumbnailReviewSessions(), key = FurnishingThumbnailReviewSessions.Key(owner: owner, furnishingId: item.id)
        XCTAssertTrue(sessions.retain(model, for: key))
        await model.prepare(); XCTAssertTrue(model.hasUnresolvedIntent); XCTAssertEqual(captures, 1)
        model.closeDisplay()
        let reopened = try XCTUnwrap(sessions.existing(key)); XCTAssertTrue(reopened === model)
        var newer = owner; newer.revision += 1
        XCTAssertTrue(reopened.rebind(name: item.details.name, api: try api(owner), document: { newer },
            capture: { captures += 1; return submission }, refreshed: {}))
        await reopened.prepare()
        XCTAssertEqual(captures, 1); XCTAssertTrue(reopened.hasUnresolvedIntent)
    }
    func testOtherOwnerCannotBorrowOrReplaceRetainedIntent() async throws {
        let (owner, item, submission) = try fixture()
        let model = FurnishingThumbnailReviewModel(name: item.details.name, furnishingId: item.id, owner: owner, api: try api(owner),
            document: { owner }, capture: { submission }, refreshed: {})
        let sessions = FurnishingThumbnailReviewSessions(), key = FurnishingThumbnailReviewSessions.Key(owner: owner, furnishingId: item.id)
        XCTAssertTrue(sessions.retain(model, for: key)); await model.prepare()
        let other = NativeDocumentState(format: owner.format, revision: owner.revision, workspaceId: owner.workspaceId,
            tenantId: owner.tenantId, principalId: UUID().uuidString.lowercased(), draft: owner.draft,
            etag: owner.etag, savedEditVersion: owner.savedEditVersion, pending: nil)
        XCTAssertNil(sessions.existing(.init(owner: other, furnishingId: item.id)))
        XCTAssertFalse(model.rebind(name: item.details.name, api: try api(other), document: { other }, capture: { submission }, refreshed: {}))
        let replacement = FurnishingThumbnailReviewModel(name: item.details.name, furnishingId: item.id, owner: owner, api: try api(owner),
            document: { owner }, capture: { submission }, refreshed: {})
        XCTAssertFalse(sessions.retain(replacement, for: key)); XCTAssertTrue(sessions.existing(key) === model)
        XCTAssertTrue(model.hasUnresolvedIntent)
    }
}
