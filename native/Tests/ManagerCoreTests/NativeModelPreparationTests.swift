// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class NativeModelPreparationTests: XCTestCase {
    func testFreshRequiredIntakeWaitsForNewUnapprovedArtworkRevision() throws {
        let artworkId = UUID().uuidString, artistId = UUID().uuidString
        let asset = NativeArtwork.Asset(id: UUID().uuidString, mime: "model/gltf-binary", state: "approved",
                                       sha256: String(repeating: "a", count: 64), bytes: 128)
        let approved = NativeArtwork(id: artworkId, artistId: artistId, revision: 1, approvedRevision: 1,
                                     approvedAssetId: asset.id, metadata: ArtworkMetadata(), assets: [asset], archived: false)
        XCTAssertThrowsError(try NativeModelPreparationIntent.requireFreshIntake(for: approved)) {
            XCTAssertEqual(($0 as? AuthoringFailure)?.code, "IMPORT_MODEL_NEW_REVISION_REQUIRED")
        }
        let revised = NativeArtwork(id: artworkId, artistId: artistId, revision: 2, approvedRevision: nil,
                                    approvedAssetId: nil, metadata: approved.metadata, assets: approved.assets, archived: false)
        XCTAssertNoThrow(try NativeModelPreparationIntent.requireFreshIntake(for: revised))
        XCTAssertEqual(revised.assets.map(\.id), [asset.id])
    }

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

    func testHayesOriginalMatchesOnlyTheQualifiedSourceArtwork() {
        let artworkId = UUID().uuidString, artistId = UUID().uuidString
        let exactHash = "694a9cf81dfbf12fd0841ae45240c5ae0f1d7cb129d0ee122be7cff86c4625bc"
        func artwork(_ hash: String) -> NativeArtwork {
            var metadata = ArtworkMetadata()
            metadata.provenance.sourceUnits = "unknown"
            metadata.displayReference = .init(version: 1, status: "provisional-uncalibrated", assetSha256: hash,
                rawBounds: .init(min: [0, 0, 0], max: [1, 1, 1]), referenceHeightMeters: 1,
                pose: .init(position: [0, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1]))
            return .init(id: artworkId, artistId: artistId, revision: 1, approvedRevision: nil,
                         approvedAssetId: nil, metadata: metadata, assets: [], archived: false)
        }
        XCTAssertTrue(NativeHayesOriginalIntent.matchesSourceArtwork(artwork(exactHash)))
        XCTAssertFalse(NativeHayesOriginalIntent.matchesSourceArtwork(artwork(String(repeating: "b", count: 64))))
    }

    func testOrdinaryJobActionsDoNotFollowASelectedArtworkAwayFromRetainedHayes() {
        let scopeId = UUID().uuidString, artwork = UUID().uuidString, other = UUID().uuidString
        let job = UUID().uuidString, request = UUID().uuidString
        let hash = NativeHayesOriginalIntent.sourceSHA256, bytes = NativeHayesOriginalIntent.sourceBytes
        var transfer = NativeUploadState(format: 1, revision: 1, workspaceId: scopeId, tenantId: scopeId,
                                         principalId: scopeId, intent: nil, jobId: job, fileName: "model.glb")
        transfer.intent = .init(artworkId: artwork, idempotencyKey: request, mime: "model/gltf-binary",
            sha256: hash, bytes: bytes, scaleMeters: 1, rights: .init(),
            hayesOriginal: .init(artworkRevision: 1, displayReferenceSha256: hash))
        XCTAssertFalse(transfer.permitsOrdinaryJobAction(job, selectedArtworkId: artwork))
        XCTAssertFalse(transfer.permitsOrdinaryJobAction(job, selectedArtworkId: other))
        transfer.intent = .init(artworkId: artwork, idempotencyKey: request, mime: "model/gltf-binary",
            sha256: hash, bytes: bytes, scaleMeters: 1, rights: .init())
        XCTAssertFalse(transfer.permitsOrdinaryJobAction(job, selectedArtworkId: other))
        XCTAssertFalse(transfer.permitsOrdinaryJobAction(UUID().uuidString, selectedArtworkId: artwork))
        XCTAssertTrue(transfer.permitsOrdinaryJobAction(job, selectedArtworkId: artwork))
    }

    func testInspectedOriginalBuildsOnlyAnAuthoredProvisionalReference() throws {
        let hash = String(repeating: "b", count: 64)
        let file = ImportFileSnapshot(name: "synthetic.glb", mime: "model/gltf-binary",
                                      sha256: hash, data: Data(repeating: 0, count: 64))
        var response: DocumentValue = .object([
            "version": .integer(1), "profileId": .string(NativeModelPreparationIntent.profile),
            "original": .object(["mime": .string(file.mime), "sha256": .string(hash), "bytes": .integer(64)]),
            "rawBounds": .object(["min": .vector([0, 0, 0]), "max": .vector([1, 2, 3])]),
            "triangles": .integer(4), "vertices": .integer(4), "sourceUnits": .string("unknown"),
            "qualification": .string("structure-only")
        ])
        let inspected = try JSONDecoder().decode(NativeModelOriginalInspection.self, from: JSONEncoder().encode(response))
        try inspected.check(file: file)
        let original = ArtworkMetadata()
        let metadata = try inspected.applying(to: original, height: 1.513)
        XCTAssertEqual(metadata.provenance.sourceUnits, "unknown")
        XCTAssertFalse(metadata.provenance.scaleApplied)
        XCTAssertEqual(metadata.dimensions, original.dimensions)
        XCTAssertEqual(metadata.displayReference?.assetSha256, hash)
        XCTAssertEqual(metadata.displayReference?.pose.scale, Array(repeating: 1.513 / 2, count: 3))
        XCTAssertThrowsError(try inspected.applying(to: original, height: 0))
        response["original"]["bytes"] = .integer(63)
        let mismatched = try JSONDecoder().decode(NativeModelOriginalInspection.self, from: JSONEncoder().encode(response))
        XCTAssertThrowsError(try mismatched.check(file: file))
    }

    func testUnknownUnitApprovalRequiresActualDisplayReferenceAndNextRevision() throws {
        let (bytes, _, original, request) = try readyFixture()
        let display = try XCTUnwrap(original.display)
        let originalId = try XCTUnwrap(original.original.assetId)
        var response = try JSONDecoder().decode(DocumentValue.self, from: bytes)
        let bounds = ArtworkMetadata.DisplayReference.Bounds(min: [0, 0, 0], max: [1, 1, 1])
        let reference = ArtworkMetadata.DisplayReference(version: 1, status: "provisional-uncalibrated",
            assetSha256: display.sha256, rawBounds: bounds, referenceHeightMeters: 1.5,
            pose: .init(position: [0, 0, 0], rotation: [0, 0, 0, 1], scale: [1.5, 1.5, 1.5]))
        let receiptKey = "serverReceipt"
        response["modelPreparation"][receiptKey]["producer"]["source"]["bounds"]["max"] = .vector(bounds.max)
        response["modelPreparation"][receiptKey]["producer"]["output"]["bounds"]["max"] = .vector(bounds.max)
        response["modelPreparation"][receiptKey]["displayReference"] = try JSONDecoder().decode(
            DocumentValue.self, from: JSONEncoder().encode(reference))
        let job = try JSONDecoder().decode(NativeImportJob.self, from: JSONEncoder().encode(response))
        let status = try XCTUnwrap(job.modelPreparation)
        XCTAssertTrue(status.matches(request, job: job))
        XCTAssertEqual(status.approvalRevision, status.intent.artworkRevision + 1)
        let assets: [NativeArtwork.Asset] = [
            .init(id: display.assetId, mime: display.mime, state: "approved", sha256: display.sha256, bytes: Int64(display.bytes)),
            .init(id: originalId, mime: status.original.mime, state: "quarantine", sha256: status.original.sha256,
                  bytes: Int64(status.original.bytes))
        ]
        var metadata = ArtworkMetadata()
        metadata.provenance.sourceUnits = "unknown"
        metadata.rights = request.rights
        let artist = UUID().uuidString
        func artwork(_ revision: Int, approved: Bool, reference: ArtworkMetadata.DisplayReference) -> NativeArtwork {
            var snapshot = metadata
            snapshot.displayReference = reference
            return .init(id: request.artworkId, artistId: artist, revision: revision, approvedRevision: approved ? revision : nil,
                         approvedAssetId: approved ? display.assetId : nil, metadata: snapshot, assets: assets, archived: false)
        }
        let sourceReference = reference.binding(to: status.original.sha256)
        let source = artwork(status.intent.artworkRevision, approved: false, reference: sourceReference)
        let rebound = artwork(status.approvalRevision, approved: true, reference: reference)
        XCTAssertTrue(status.permitsReview(of: source, request: request, job: job))
        XCTAssertFalse(status.permitsClearingTransfer(artwork: source, request: request, job: job))
        XCTAssertTrue(status.permitsClearingTransfer(artwork: rebound, request: request, job: job))
        XCTAssertFalse(status.permitsClearingTransfer(
            artwork: artwork(status.approvalRevision, approved: true, reference: sourceReference), request: request, job: job))
        response["modelPreparation"][receiptKey]["producer"]["output"]["bounds"]["max"] = .vector([1, 1, 2])
        let changed = try JSONDecoder().decode(NativeImportJob.self, from: JSONEncoder().encode(response))
        XCTAssertEqual(changed.modelPreparation?.matches(request, job: changed), false)
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
