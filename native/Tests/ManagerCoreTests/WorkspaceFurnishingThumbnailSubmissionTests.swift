// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class WorkspaceFurnishingThumbnailSubmissionTests: XCTestCase {
    // Synthetic 256x256 PNG, shared with the renderer's existing source tests.
    private let png = "iVBORw0KGgoAAAANSUhEUgAAAQAAAAEACAYAAABccqhmAAABFUlEQVR4nO3BMQEAAADCoPVP7WsIoAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAeAMBPAABPO1TCQAAAABJRU5ErkJggg=="

    private func value<T: Encodable>(_ input: T) throws -> DocumentValue {
        try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(input))
    }
    private func furnishing(id: String = "11111111-1111-4111-8111-111111111111", bytes: Int = 128,
                            hash: String = String(repeating: "a", count: 64),
                            rights: ArtworkMetadata.Rights = .init()) throws -> WorkspaceFurnishing {
        let descriptor: DocumentValue = .object(["id": .string(id), "version": .integer(1), "kind": .string("bench"),
            "name": .string("Synthetic"), "provenance": .string("Synthetic source"), "rights": try value(rights),
            "model": .object(["mime": .string("model/gltf-binary"), "bytes": .integer(Int64(bytes)), "sha256": .string(hash)])])
        return try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(descriptor))
    }
    private func fixture() throws -> (DocumentValue, NativeDocumentState, WorkspaceFurnishing, Data) {
        let draft = try NativeExhibitionDraft.new()
        let document = NativeDocumentState(format: 1, revision: 4, workspaceId: UUID().uuidString.lowercased(),
            tenantId: UUID().uuidString.lowercased(), principalId: UUID().uuidString.lowercased(), draft: draft,
            etag: "capture-etag", savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        let item = try furnishing()
        let frame = ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 1,
            tenantId: document.tenantId, principalId: document.principalId, draft: draft,
            selection: nil, mode: "translate", view: "isometric", disabled: false,
            workspaceId: document.workspaceId, baseETag: document.etag)
        let request = try ViewportFurnishingThumbnailRequest(frame: frame, furnishing: item)
        let bytes = try XCTUnwrap(Data(base64Encoded: png))
        let capture: DocumentValue = .object(["workspaceId": .string(document.workspaceId), "nativeRevision": .integer(4),
            "editVersion": draft.document["editVersion"], "baseETag": .string("capture-etag"), "request": try value(request),
            "png": .object(["mime": .string("image/png"), "width": .integer(256), "height": .integer(256),
                "bytes": .integer(Int64(bytes.count)), "sha256": .string(nativeImportSHA256(bytes)), "base64": .string(png)])])
        return (capture, document, item, bytes)
    }
    private func replacingPNG(_ capture: DocumentValue, with bytes: Data) -> DocumentValue {
        var result = capture
        result["png"]["bytes"] = .integer(Int64(bytes.count)); result["png"]["sha256"] = .string(nativeImportSHA256(bytes))
        result["png"]["base64"] = .string(bytes.base64EncodedString()); return result
    }

    func testExactBodyBindsImmutableSourceAndPNGWithoutAuthorityFields() throws {
        let (capture, document, furnishing, bytes) = try fixture()
        let submission = try WorkspaceFurnishingThumbnailSubmission(capture: capture, document: document, furnishing: furnishing)
        guard case .object(let fields) = submission.value else { return XCTFail("Expected POST object") }
        XCTAssertEqual(Set(fields.keys), ["kind", "requestId", "renderRequest", "pngSha256", "pngBytes", "pngBase64"])
        XCTAssertEqual(submission.value["kind"], .string("workspace-furnishing-thumbnail-v1"))
        XCTAssertEqual(submission.value["requestId"], capture["request"]["requestId"])
        XCTAssertEqual(submission.value["renderRequest"], capture["request"])
        XCTAssertEqual(submission.request.furnishingId, furnishing.id)
        XCTAssertEqual(submission.request.modelSha256, furnishing.details.model.sha256)
        XCTAssertEqual(submission.workspaceId, document.workspaceId)
        XCTAssertEqual(submission.pngData, bytes); XCTAssertEqual(submission.pngBytes, bytes.count)
        XCTAssertEqual(submission.pngSha256, nativeImportSHA256(bytes))
        XCTAssertEqual(submission.value["pngBytes"], .integer(Int64(bytes.count)))
        XCTAssertEqual(try JSONDecoder().decode(DocumentValue.self, from: submission.body), submission.value)
    }

    func testRetainedMetadataNeedsExplicitMatchingBytes() throws {
        let (capture, document, furnishing, bytes) = try fixture()
        var retained = capture
        guard case .object(var png) = retained["png"] else { return XCTFail("Expected PNG metadata") }
        png.removeValue(forKey: "base64"); retained["png"] = .object(png)
        let original = try WorkspaceFurnishingThumbnailSubmission(capture: capture, document: document, furnishing: furnishing)
        let separate = try WorkspaceFurnishingThumbnailSubmission(capture: retained, pngData: bytes, document: document, furnishing: furnishing)
        XCTAssertEqual(original.body, separate.body)
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSubmission(capture: retained, document: document, furnishing: furnishing))
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSubmission(capture: capture, pngData: Data([0]), document: document, furnishing: furnishing))
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSubmission(capture: retained, pngData: bytes + Data([0]), document: document, furnishing: furnishing))
    }

    func testRejectsWrongOwnerModelAndRendererBindings() throws {
        let (capture, document, furnishing, _) = try fixture()
        let mutations: [(String, DocumentValue)] = [("tenantId", .string(UUID().uuidString.lowercased())),
            ("principalId", .string(UUID().uuidString.lowercased())), ("furnishingId", .string(UUID().uuidString.lowercased())),
            ("furnishingVersion", .integer(2)), ("modelSha256", .string(String(repeating: "b", count: 64))),
            ("modelBytes", .integer(129)), ("version", .integer(2)), ("sequence", .integer(0)),
            ("epoch", .string("invalid")), ("requestId", .string("invalid")),
            ("recipeId", .string("other")), ("toolId", .string("other"))]
        for (key, value) in mutations {
            var changed = capture; changed["request"][key] = value
            XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSubmission(capture: changed, document: document, furnishing: furnishing), key)
        }
        var changed = capture; changed["workspaceId"] = .string(UUID().uuidString.lowercased())
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSubmission(capture: changed, document: document, furnishing: furnishing))
    }

    func testRejectsUnknownKeysMissingBindingsAndMalformedProvenance() throws {
        let (capture, document, furnishing, _) = try fixture()
        var extra = capture; extra["path"] = .string("/untrusted/thumbnail.png")
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSubmission(capture: extra, document: document, furnishing: furnishing))
        extra = capture; extra["request"]["extra"] = .bool(true)
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSubmission(capture: extra, document: document, furnishing: furnishing))
        extra = capture; extra["png"]["path"] = .string("/untrusted/thumbnail.png")
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSubmission(capture: extra, document: document, furnishing: furnishing))
        var missing = capture
        guard case .object(var fields) = missing["request"] else { return XCTFail("Expected request") }
        fields.removeValue(forKey: "toolId"); missing["request"] = .object(fields)
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSubmission(capture: missing, document: document, furnishing: furnishing))
        for (key, value) in [("nativeRevision", DocumentValue.integer(-1)), ("editVersion", .string("94")), ("baseETag", .null)] {
            var changed = capture; changed[key] = value
            XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSubmission(capture: changed, document: document, furnishing: furnishing), key)
        }
    }

    func testRejectsTamperedInvalidOversizeAndMislabeledPNG() throws {
        let (capture, document, furnishing, bytes) = try fixture()
        var damaged = bytes; damaged[damaged.count - 1] ^= 1
        for bad in [damaged, Data(repeating: 0, count: 40), Data(repeating: 0, count: 512 * 1024 + 1), bytes + Data([0])] {
            // A correct supplied hash cannot make invalid PNG content valid.
            XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSubmission(capture: replacingPNG(capture, with: bad), document: document, furnishing: furnishing))
        }
        for (key, value) in [("sha256", DocumentValue.string(String(repeating: "b", count: 64))),
                              ("bytes", .integer(1)), ("width", .integer(255)), ("height", .integer(128)),
                              ("mime", .string("image/jpeg")), ("base64", .string(png + "\n"))] {
            var changed = capture; changed["png"][key] = value
            XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSubmission(capture: changed, document: document, furnishing: furnishing), key)
        }
    }

    func testUnrelatedSceneEditsDoNotInvalidateIsolatedSourceCapture() throws {
        let (capture, document, furnishing, _) = try fixture()
        let before = try WorkspaceFurnishingThumbnailSubmission(capture: capture, document: document, furnishing: furnishing)
        var changed = document
        var draft = try XCTUnwrap(changed.draft), candidate = draft.candidate
        candidate["title"] = .string("Unrelated scene edit"); try draft.replaceCandidate(candidate)
        changed.draft = draft; changed.revision += 10; changed.etag = "later-etag"
        XCTAssertTrue(changed.dirty)
        let reused = try WorkspaceFurnishingThumbnailSubmission(capture: capture, document: changed, furnishing: furnishing)
        XCTAssertEqual(reused.body, before.body)
        XCTAssertEqual(capture["baseETag"], .string("capture-etag"))
    }

    func testCurrentRightsAndSeparateSourceIdentityRemainRequired() throws {
        let (capture, document, original, bytes) = try fixture()
        var rights = ArtworkMetadata.Rights(); rights.permissions.display = false
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSubmission(capture: capture, document: document, furnishing: furnishing(rights: rights)))
        rights.permissions.display = true; rights.expiresAt = "2000-01-01T00:00:00Z"
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSubmission(capture: capture, document: document, furnishing: furnishing(rights: rights)))
        rights.expiresAt = nil; rights.validFrom = "2999-01-01T00:00:00Z"
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSubmission(capture: capture, document: document, furnishing: furnishing(rights: rights)))
        let different = try furnishing(id: "22222222-2222-4222-8222-222222222222", bytes: 256, hash: String(repeating: "b", count: 64))
        XCTAssertThrowsError(try WorkspaceFurnishingThumbnailSubmission(capture: capture, document: document, furnishing: different))
        var otherCapture = capture
        otherCapture["request"]["furnishingId"] = .string(different.id)
        otherCapture["request"]["modelBytes"] = .integer(Int64(different.details.model.bytes))
        otherCapture["request"]["modelSha256"] = .string(different.details.model.sha256)
        otherCapture["request"]["requestId"] = .string(UUID().uuidString.lowercased())
        let first = try WorkspaceFurnishingThumbnailSubmission(capture: capture, document: document, furnishing: original)
        let second = try WorkspaceFurnishingThumbnailSubmission(capture: otherCapture, document: document, furnishing: different)
        XCTAssertEqual(second.pngData, bytes); XCTAssertEqual(first.pngSha256, second.pngSha256)
        XCTAssertNotEqual(first.request.furnishingId, second.request.furnishingId)
        XCTAssertNotEqual(first.body, second.body) // PNG identity never substitutes for source identity.
    }
}
