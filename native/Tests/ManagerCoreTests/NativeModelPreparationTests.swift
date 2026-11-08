// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class NativeModelPreparationTests: XCTestCase {
    private func readyFixture() throws -> (Data, NativeImportJob, NativeModelPreparationStatus, NativeImportIntent) {
        guard let path = ProcessInfo.processInfo.environment["EXHIBITOS_REQUIRED_MODEL_READY_FIXTURE"] else {
            throw XCTSkip("Set the synthetic Platform 045 fixture path for this paired contract check")
        }
        let bytes = try Data(contentsOf: URL(fileURLWithPath: path))
        let ready = try JSONDecoder().decode(NativeImportJob.self, from: bytes)
        let status = try XCTUnwrap(ready.modelPreparation)
        let receipt = try XCTUnwrap(status.serverReceipt)
        let request = NativeImportIntent(artworkId: receipt.artworkId, idempotencyKey: status.intent.operationID,
            mime: status.original.mime, sha256: status.original.sha256, bytes: status.original.bytes,
            scaleMeters: 1, rights: .init(), modelPreparation: status.intent)
        return (bytes, ready, status, request)
    }

    func testRequiredReadyWireAndTamperedReceipt() throws {
        let (bytes, ready, status, request) = try readyFixture()
        XCTAssertTrue(status.matches(request, job: ready))
        XCTAssertTrue(ready.confirmsIntakeDrain(for: request))
        let restored = try JSONDecoder().decode(NativeImportIntent.self, from: JSONEncoder().encode(request))
        XCTAssertEqual(restored, request)

        var response = try JSONDecoder().decode(DocumentValue.self, from: bytes)
        response["modelPreparation"]["serverReceipt"]["generation"] = .string("another-generation")
        let tampered = try JSONDecoder().decode(NativeImportJob.self, from: JSONEncoder().encode(response))
        XCTAssertFalse(tampered.confirmsIntakeDrain(for: request))
        response = try JSONDecoder().decode(DocumentValue.self, from: bytes)
        response["modelPreparation"]["workerDrained"] = .bool(false)
        let draining = try JSONDecoder().decode(NativeImportJob.self, from: JSONEncoder().encode(response))
        XCTAssertFalse(draining.confirmsIntakeDrain(for: request))
    }

    func testRequiredTransferRemainsUntilExactApprovalOrDrainedCancellation() throws {
        let (bytes, job, status, request) = try readyFixture()
        let display = try XCTUnwrap(status.display)
        let originalId = try XCTUnwrap(status.original.assetId)
        let assets: [NativeArtwork.Asset] = [
            .init(id: display.assetId, mime: display.mime, state: "approved", sha256: display.sha256, bytes: Int64(display.bytes)),
            .init(id: originalId, mime: status.original.mime, state: "quarantine", sha256: status.original.sha256,
                  bytes: Int64(status.original.bytes))
        ]
        func artwork(approvedRevision: Int? = nil, approvedAsset: String? = nil, rightsChanged: Bool = false) -> NativeArtwork {
            var metadata = ArtworkMetadata()
            metadata.rights = request.rights
            if rightsChanged { metadata.rights.holder = "Different rights holder" }
            return .init(id: request.artworkId, artistId: UUID().uuidString, revision: status.intent.artworkRevision,
                         approvedRevision: approvedRevision, approvedAssetId: approvedAsset, metadata: metadata,
                         assets: assets, archived: false)
        }
        let revision = status.intent.artworkRevision
        let approved = artwork(approvedRevision: revision, approvedAsset: display.assetId)
        let scopeId = UUID().uuidString.lowercased()
        var transfer = NativeUploadState(format: 1, revision: 1, workspaceId: scopeId, tenantId: scopeId,
                                         principalId: scopeId, intent: request, jobId: job.id, fileName: "fixture.glb")
        XCTAssertTrue(status.permitsReview(of: artwork(), request: request, job: job))
        XCTAssertFalse(status.permitsClearingTransfer(artwork: artwork(), request: request, job: job))
        XCTAssertTrue(transfer.holdsRequiredModelMetadata(for: request.artworkId.uppercased()))
        XCTAssertFalse(transfer.holdsRequiredModelMetadata(for: UUID().uuidString))
        XCTAssertFalse(transfer.holdsRequiredModelMetadata(for: nil))
        XCTAssertFalse(status.permitsClearingTransfer(
            artwork: artwork(approvedRevision: revision + 1, approvedAsset: display.assetId), request: request, job: job))
        XCTAssertFalse(status.permitsClearingTransfer(
            artwork: artwork(approvedRevision: revision, approvedAsset: originalId), request: request, job: job))
        XCTAssertFalse(status.permitsClearingTransfer(
            artwork: artwork(approvedRevision: revision, approvedAsset: display.assetId, rightsChanged: true), request: request, job: job))
        XCTAssertTrue(status.permitsClearingTransfer(artwork: approved, request: request, job: job))
        XCTAssertTrue(transfer.holdsRequiredModelMetadata(for: request.artworkId))

        var response = try JSONDecoder().decode(DocumentValue.self, from: bytes)
        response["modelPreparation"]["serverReceipt"]["generation"] = .string("another-generation")
        let tampered = try JSONDecoder().decode(NativeImportJob.self, from: JSONEncoder().encode(response))
        XCTAssertEqual(tampered.modelPreparation?.permitsClearingTransfer(artwork: approved, request: request, job: tampered), false)

        response["state"] = .string("cancelled")
        response["assetId"] = .null
        response["modelPreparation"]["status"] = .string("cancelled")
        response["modelPreparation"]["original"]["assetId"] = .null
        response["modelPreparation"]["display"] = .null
        response["modelPreparation"]["serverReceipt"] = .null
        for drained in [false, true] {
            response["modelPreparation"]["workerDrained"] = .bool(drained)
            let cancelled = try JSONDecoder().decode(NativeImportJob.self, from: JSONEncoder().encode(response))
            let preparation = try XCTUnwrap(cancelled.modelPreparation)
            XCTAssertEqual(preparation.permitsClearingTransfer(artwork: nil, request: request, job: cancelled), drained)
        }
        transfer.intent = nil; transfer.jobId = nil; transfer.fileName = nil
        XCTAssertFalse(transfer.holdsRequiredModelMetadata(for: request.artworkId))
    }
}
