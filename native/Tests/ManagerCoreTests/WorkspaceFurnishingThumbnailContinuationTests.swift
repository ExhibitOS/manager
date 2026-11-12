// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class WorkspaceFurnishingThumbnailContinuationTests: XCTestCase {
    private let png = "iVBORw0KGgoAAAANSUhEUgAAAQAAAAEACAYAAABccqhmAAABFUlEQVR4nO3BMQEAAADCoPVP7WsIoAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAeAMBPAABPO1TCQAAAABJRU5ErkJggg=="
    private struct Fixture {
        let owner: NativeDocumentState
        let furnishing: WorkspaceFurnishing
        let submission: WorkspaceFurnishingThumbnailSubmission
        let candidate: WorkspaceFurnishingThumbnailCandidate
    }
    private func value<T: Encodable>(_ input: T) throws -> DocumentValue {
        try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(input))
    }
    private func fixture() throws -> Fixture {
        let draft = try NativeExhibitionDraft.new()
        let owner = NativeDocumentState(format: 1, revision: 4, workspaceId: UUID().uuidString.lowercased(),
            tenantId: UUID().uuidString.lowercased(), principalId: UUID().uuidString.lowercased(), draft: draft,
            etag: "synthetic-capture-head", savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        let descriptor: DocumentValue = .object(["id": .string(UUID().uuidString.lowercased()), "version": .integer(1),
            "kind": .string("bench"), "name": .string("Synthetic"), "provenance": .string("Synthetic fixture"),
            "rights": try value(ArtworkMetadata.Rights()), "model": .object(["mime": .string("model/gltf-binary"),
                "bytes": .integer(128), "sha256": .string(String(repeating: "a", count: 64))])])
        let item = try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(descriptor))
        let frame = ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 1, tenantId: owner.tenantId,
            principalId: owner.principalId, draft: draft, selection: nil, mode: "translate", view: "isometric", disabled: false,
            workspaceId: owner.workspaceId, baseETag: owner.etag)
        let request = try value(ViewportFurnishingThumbnailRequest(frame: frame, furnishing: item))
        let bytes = try XCTUnwrap(Data(base64Encoded: png))
        let thumbnail: DocumentValue = .object(["mime": .string("image/png"), "width": .integer(256), "height": .integer(256),
            "bytes": .integer(Int64(bytes.count)), "sha256": .string(nativeImportSHA256(bytes))])
        var capturePNG = thumbnail; capturePNG["base64"] = .string(png)
        let capture: DocumentValue = .object(["workspaceId": .string(owner.workspaceId), "nativeRevision": .integer(4),
            "editVersion": draft.document["editVersion"], "baseETag": .string("synthetic-capture-head"), "request": request, "png": capturePNG])
        let submission = try WorkspaceFurnishingThumbnailSubmission(capture: capture, document: owner, furnishing: item)
        let source: DocumentValue = .object(["furnishingId": .string(item.id), "furnishingVersion": .integer(1),
            "modelSha256": .string(item.details.model.sha256), "modelBytes": .integer(128),
            "descriptorSha256": .string(String(repeating: "b", count: 64)), "rightsSha256": .string(String(repeating: "c", count: 64))])
        let candidateId = "58830074-8ca6-8af4-a506-f6976ad8ebb8"
        let receipt: DocumentValue = .object(["version": .integer(1), "purpose": .string("registered-furnishing-thumbnail"),
            "profile": .string("workspace-furnishing-thumbnail-v1"), "workspaceId": .string(owner.workspaceId),
            "candidateId": .string(candidateId), "requestId": request["requestId"], "source": source, "renderRequest": request, "png": thumbnail])
        let raw: DocumentValue = .object(["candidateId": .string(candidateId), "state": .string("ready"), "source": source,
            "thumbnail": thumbnail, "receipt": receipt, "receiptSha256": .string(String(repeating: "d", count: 64))])
        let candidate = try WorkspaceFurnishingThumbnailCandidate(value: raw, document: owner, furnishing: item, submission: submission)
        return Fixture(owner: owner, furnishing: item, submission: submission, candidate: candidate)
    }
    private func decode(_ value: DocumentValue) throws -> NativeFurnishingThumbnailState {
        try JSONDecoder().decode(NativeFurnishingThumbnailState.self, from: JSONEncoder().encode(value))
    }

    func testCaptureJournalRoundTripRetainsMetadataNonceAndPNGAcrossSceneEdits() throws {
        let f = try fixture(); var state = NativeFurnishingThumbnailState(document: f.owner)
        state.pending = try NativeFurnishingThumbnailPending(capture: f.submission)
        let raw = try value(state)
        guard case .object(let fields) = raw else { return XCTFail("Expected closed journal") }
        XCTAssertEqual(Set(fields.keys), ["format", "revision", "workspaceId", "tenantId", "principalId", "pending"])
        XCTAssertEqual(raw["pending"]["capture"], f.submission.captureMetadata)
        guard case .object(let metadata) = raw["pending"]["capture"]["png"] else { return XCTFail("Expected PNG metadata") }
        XCTAssertNil(metadata["base64"], "Persist bytes once, outside retained SDK metadata")
        XCTAssertEqual(raw["pending"]["pngBase64"], .string(png))
        let decoded = try decode(raw), pending = try XCTUnwrap(decoded.pending)
        XCTAssertEqual(pending.requestId, f.submission.request.requestId)
        XCTAssertEqual(pending, state.pending)
        var later = f.owner; later.revision += 10; later.etag = "later-saved-head"
        let restored = try pending.submission(in: later, furnishing: f.furnishing)
        XCTAssertEqual(restored.body, f.submission.body)
        XCTAssertEqual(restored.captureMetadata, f.submission.captureMetadata)
        XCTAssertEqual(restored.pngData, f.submission.pngData)
        XCTAssertEqual(decoded.revision, 0, "Exhibition revision changes do not create journal writes")
    }

    func testSelectionJournalPreservesDistinctMutationNonceAndOriginalCAS() throws {
        let f = try fixture(), selectionNonce = UUID().uuidString.lowercased()
        var state = NativeFurnishingThumbnailState(document: f.owner)
        state.pending = try NativeFurnishingThumbnailPending(candidate: f.candidate, requestId: selectionNonce, expectedSelectionVersion: 7)
        let pending = try XCTUnwrap(decode(value(state)).pending)
        XCTAssertEqual(pending.kind, .selection); XCTAssertEqual(pending.requestId, selectionNonce)
        XCTAssertNotEqual(pending.requestId, f.submission.request.requestId)
        XCTAssertEqual(pending.expectedSelectionVersion, 7)
        var later = f.owner; later.revision += 1
        XCTAssertEqual(try pending.candidate(in: later, furnishing: f.furnishing).value, f.candidate.value)
        XCTAssertThrowsError(try pending.submission(in: later, furnishing: f.furnishing))
        for version in [-1, 2_147_483_647] as [Int64] {
            XCTAssertThrowsError(try NativeFurnishingThumbnailPending(candidate: f.candidate, requestId: selectionNonce, expectedSelectionVersion: version))
        }
        XCTAssertThrowsError(try NativeFurnishingThumbnailPending(candidate: f.candidate, requestId: "invalid", expectedSelectionVersion: 7))
    }

    func testOwnerSwapCannotDecodeOrResumeEitherPendingKind() throws {
        let f = try fixture()
        let pendingKinds = [try NativeFurnishingThumbnailPending(capture: f.submission),
            try NativeFurnishingThumbnailPending(candidate: f.candidate, requestId: UUID().uuidString.lowercased(), expectedSelectionVersion: 0)]
        for pending in pendingKinds {
            var state = NativeFurnishingThumbnailState(document: f.owner); state.pending = pending
            for key in ["workspaceId", "tenantId", "principalId"] {
                var changed = try value(state); changed[key] = .string(UUID().uuidString.lowercased())
                XCTAssertThrowsError(try decode(changed), key)
            }
            let other = NativeDocumentState(format: 1, revision: f.owner.revision, workspaceId: f.owner.workspaceId,
                tenantId: f.owner.tenantId, principalId: UUID().uuidString.lowercased(), draft: f.owner.draft,
                etag: f.owner.etag, savedEditVersion: f.owner.savedEditVersion, pending: nil)
            XCTAssertThrowsError(try state.requireScope(other))
            XCTAssertThrowsError(try pending.requireScope(other))
        }
    }

    func testClosedJournalRejectsMissingPendingUnknownFieldsAndInvalidRevision() throws {
        let f = try fixture(), raw = try value(NativeFurnishingThumbnailState(document: f.owner))
        for (key, bad) in [("format", DocumentValue.integer(2)), ("revision", .integer(-1)),
                           ("revision", .number(0.5)), ("tenantId", .string("invalid")), ("unknown", .bool(true))] {
            var changed = raw; changed[key] = bad
            XCTAssertThrowsError(try decode(changed), key)
        }
        guard case .object(var fields) = raw else { return XCTFail("Expected journal") }
        fields.removeValue(forKey: "pending")
        XCTAssertThrowsError(try decode(.object(fields)))
        XCTAssertNil(try decode(raw).pending)
    }

    func testCorruptCaptureBytesMetadataAndNonceFailBeforeRehydration() throws {
        let f = try fixture(), pending = try NativeFurnishingThumbnailPending(capture: f.submission)
        var unknown = pending.value; unknown["unexpected"] = .bool(true)
        XCTAssertThrowsError(try NativeFurnishingThumbnailPending(value: unknown))
        var nonce = pending.value; nonce["capture"]["request"]["requestId"] = .string(UUID().uuidString.lowercased())
        XCTAssertThrowsError(try NativeFurnishingThumbnailPending(value: nonce))
        var bytes = pending.value; bytes["pngBase64"] = .string((f.submission.pngData + Data([0])).base64EncodedString())
        XCTAssertThrowsError(try NativeFurnishingThumbnailPending(value: bytes))
        var noncanonical = pending.value; noncanonical["pngBase64"] = .string(png + "\n")
        XCTAssertThrowsError(try NativeFurnishingThumbnailPending(value: noncanonical))
        var duplicated = pending.value; duplicated["capture"]["png"]["base64"] = .string(png)
        XCTAssertThrowsError(try NativeFurnishingThumbnailPending(value: duplicated))
        var provenance = pending.value; provenance["capture"]["nativeRevision"] = .integer(-1)
        let retained = try NativeFurnishingThumbnailPending(value: provenance)
        XCTAssertThrowsError(try retained.submission(in: f.owner, furnishing: f.furnishing), "Full SDK metadata is revalidated before use")
    }

    func testRestoreRequiresCurrentSourceAndDisplayRightsRatherThanJournalAuthority() throws {
        let f = try fixture(), pending = try NativeFurnishingThumbnailPending(capture: f.submission)
        var descriptor = try value(f.furnishing)
        descriptor["model"]["sha256"] = .string(String(repeating: "e", count: 64))
        let otherModel = try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(descriptor))
        XCTAssertThrowsError(try pending.submission(in: f.owner, furnishing: otherModel))
        descriptor = try value(f.furnishing); descriptor["rights"]["permissions"]["display"] = .bool(false)
        let denied = try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(descriptor))
        XCTAssertThrowsError(try pending.submission(in: f.owner, furnishing: denied))
        var selection = try NativeFurnishingThumbnailPending(candidate: f.candidate, requestId: UUID().uuidString.lowercased(), expectedSelectionVersion: 1).value
        selection["candidate"]["receipt"]["renderRequest"]["principalId"] = .string(UUID().uuidString.lowercased())
        let foreign = try NativeFurnishingThumbnailPending(value: selection)
        XCTAssertThrowsError(try foreign.candidate(in: f.owner, furnishing: f.furnishing))
    }
}
