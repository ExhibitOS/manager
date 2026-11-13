// SPDX-License-Identifier: Apache-2.0
import XCTest
import CryptoKit
@testable import ManagerCore

final class NativeCurrentImageThumbnailTests: XCTestCase {
    private let artworkID = "11111111-1111-4111-8111-111111111111"
    private let sourceID = "22222222-2222-4222-8222-222222222222"
    private let thumbnailID = "33333333-3333-4333-8333-333333333333"
    private let jobID = "44444444-4444-4444-8444-444444444444"
    private let workspace = "55555555-5555-4555-8555-555555555555"
    private let principal = "66666666-6666-4666-8666-666666666666"
    private let sourceHash = String(repeating: "a", count: 64)
    private let outputHash = String(repeating: "b", count: 64)
    private func hash<T: Encodable>(_ value: T) throws -> String {
        let e = JSONEncoder(); e.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        return try SHA256.hash(data: e.encode(value)).map { String(format: "%02x", $0) }.joined()
    }
    private func artwork() -> NativeArtwork {
        var metadata = ArtworkMetadata(); metadata.rights.permissions.display = true; metadata.rights.permissions.export = true
        return NativeArtwork(id: artworkID, artistId: "77777777-7777-4777-8777-777777777777", revision: 3,
            approvedRevision: 3, approvedAssetId: sourceID, metadata: metadata,
            assets: [NativeArtwork.Asset(id: sourceID, mime: "image/png", state: "approved", sha256: sourceHash, bytes: 1000)], archived: false)
    }
    private func ready() throws -> DocumentValue {
        let source: DocumentValue = .object(["assetId": .string(sourceID), "sha256": .string(sourceHash), "bytes": .integer(1000), "mime": .string("image/png")])
        let thumbnail: DocumentValue = .object(["assetId": .string(thumbnailID), "sha256": .string(outputHash), "bytes": .integer(100), "mime": .string("image/png"), "width": .integer(256), "height": .integer(128)])
        let producer: DocumentValue = .object(["version": .integer(1), "recipeSHA256": .string("d13ad48b1ae8c29818002225fb5ebb4840907646810d90922a4ea3a5448f06c0"),
            "decoder": .string("sharp-0.35.4-vips-8.18.6-mozjpeg-0826579"),
            "source": .object(["mime": .string("image/png"), "sha256": .string(sourceHash), "bytes": .integer(1000), "width": .integer(512), "height": .integer(256), "iccSHA256": .null]),
            "full": .object(["sha256": .string(String(repeating:"c",count:64)), "bytes": .integer(900), "width": .integer(512), "height": .integer(256)])])
        var receipt: DocumentValue = .object(["version": .integer(1), "profile": .string(NativeCurrentImageThumbnailJob.profile),
            "jobId": .string(jobID), "artworkId": .string(artworkID), "artworkRevision": .integer(3),
            "workspaceId": .string(workspace), "principalId": .string(principal),
            "rightsSHA256": .string(try hash(artwork().metadata!.rights)), "source": source,
            "thumbnail": thumbnail, "producer": producer])
        receipt["sha256"] = .string(try hash(receipt))
        let selection: DocumentValue = .object(["expectedRevision": .integer(3), "sourceAssetId": .string(sourceID),
            "sourceSha256": .string(sourceHash), "thumbnailAssetId": .string(thumbnailID),
            "thumbnailSha256": .string(outputHash), "view": .object(["kind": .string("default")])])
        return .object(["id": .string(jobID), "state": .string("approved"), "progress": .integer(100), "attempts": .integer(1),
            "errorCode": .null, "assetId": .string(thumbnailID), "retryAt": .null, "originalAssetId": .null,
            "workerDrainVersion": .integer(1), "workerDrained": .bool(true),
            "thumbnailPreparation": .object(["profile": .string(NativeCurrentImageThumbnailJob.profile), "artworkRevision": .integer(3),
                "source": source, "status": .string("ready"), "workerDrained": .bool(true), "serverReceipt": receipt, "selectionCandidate": selection])])
    }
    private func decode(_ wire: DocumentValue) throws -> NativeCurrentImageThumbnailJob {
        try JSONDecoder().decode(NativeCurrentImageThumbnailJob.self, from: JSONEncoder().encode(wire))
    }
    private func resign(_ value: inout DocumentValue) throws {
        guard case .object(var fields) = value["thumbnailPreparation"]["serverReceipt"] else { XCTFail(); return }
        fields.removeValue(forKey: "sha256"); let body = DocumentValue.object(fields)
        value["thumbnailPreparation"]["serverReceipt"]["sha256"] = .string(try hash(body))
    }
    func testReadyBindsCurrentApprovedSourceAndOwners() throws {
        let job = try decode(ready())
        XCTAssertNoThrow(try job.check(artwork: artwork(), workspaceId: workspace, principalId: principal, jobId: jobID))
        let roundTrip = try JSONDecoder().decode(NativeCurrentImageThumbnailJob.self, from: JSONEncoder().encode(job))
        XCTAssertNoThrow(try roundTrip.check(artwork: artwork(), workspaceId: workspace, principalId: principal, jobId: jobID))
        XCTAssertThrowsError(try job.check(artwork: artwork(), workspaceId: workspace, principalId: "77777777-7777-4777-8777-777777777777", jobId: jobID))
        XCTAssertThrowsError(try job.check(artwork: artwork(), workspaceId: workspace, principalId: principal, jobId: "77777777-7777-4777-8777-777777777777"))
        let a = artwork()
        let stale = NativeArtwork(id:a.id, artistId:a.artistId, revision:4, approvedRevision:3, approvedAssetId:a.approvedAssetId, metadata:a.metadata, assets:a.assets, archived:false)
        XCTAssertThrowsError(try job.check(artwork: stale, workspaceId: workspace, principalId: principal, jobId: jobID))
    }
    func testClosedReceiptTamperingAndContradictoryCandidateFail() throws {
        var value = try ready(); value["thumbnailPreparation"]["serverReceipt"]["sha256"] = .string(String(repeating:"0",count:64))
        XCTAssertThrowsError(try decode(value))
        value = try ready(); value["thumbnailPreparation"]["serverReceipt"]["unexpected"] = .bool(true)
        XCTAssertThrowsError(try decode(value))
        value = try ready(); value["thumbnailPreparation"]["workerDrained"] = .bool(false)
        XCTAssertThrowsError(try decode(value))
        value = try ready(); value["thumbnailPreparation"]["selectionCandidate"]["thumbnailSha256"] = .string(sourceHash)
        let job = try decode(value)
        XCTAssertThrowsError(try job.check(artwork: artwork(), workspaceId: workspace, principalId: principal, jobId: jobID))
        value = try ready(); value["thumbnailPreparation"]["serverReceipt"]["thumbnail"]["width"] = .integer(257); try resign(&value)
        let tooLarge = try decode(value)
        XCTAssertThrowsError(try tooLarge.check(artwork: artwork(), workspaceId: workspace, principalId: principal, jobId: jobID))
        value = try ready(); value["thumbnailPreparation"]["serverReceipt"]["producer"]["source"]["sha256"] = .string(outputHash); try resign(&value)
        let wrongSource = try decode(value)
        XCTAssertThrowsError(try wrongSource.check(artwork: artwork(), workspaceId: workspace, principalId: principal, jobId: jobID))
        value = try ready(); value["thumbnailPreparation"]["serverReceipt"]["workspaceId"] = .string("77777777-7777-4777-8777-777777777777"); try resign(&value)
        let foreign = try decode(value)
        XCTAssertThrowsError(try foreign.check(artwork: artwork(), workspaceId: workspace, principalId: principal, jobId: jobID))
        value = try ready(); value["thumbnailPreparation"]["serverReceipt"]["rightsSHA256"] = .string(sourceHash); try resign(&value)
        let wrongRights = try decode(value)
        XCTAssertThrowsError(try wrongRights.check(artwork: artwork(), workspaceId: workspace, principalId: principal, jobId: jobID))
        value = try ready(); value["thumbnailPreparation"]["serverReceipt"]["thumbnail"]["assetId"] = .string(sourceID); try resign(&value)
        value["assetId"] = .string(sourceID); value["thumbnailPreparation"]["selectionCandidate"]["thumbnailAssetId"] = .string(sourceID)
        let sameAsset = try decode(value)
        XCTAssertThrowsError(try sameAsset.check(artwork: artwork(), workspaceId: workspace, principalId: principal, jobId: jobID))
    }
    func testPendingCannotSmuggleReadyAuthorityAndRequestIsClosed() throws {
        var value = try ready(); value["state"] = .string("queued"); value["progress"] = .integer(0)
        value["assetId"] = .null; value["workerDrained"] = .bool(false)
        value["thumbnailPreparation"]["status"] = .string("pending"); value["thumbnailPreparation"]["workerDrained"] = .bool(false)
        XCTAssertThrowsError(try decode(value))
        guard case .object(var p) = value["thumbnailPreparation"] else { XCTFail(); return }
        p.removeValue(forKey:"serverReceipt"); p.removeValue(forKey:"selectionCandidate")
        value["thumbnailPreparation"] = .object(p)
        let pending = try decode(value)
        XCTAssertNoThrow(try pending.check(artwork: artwork(), workspaceId: workspace, principalId: principal, jobId: jobID))
        value["state"] = .string("failed"); value["thumbnailPreparation"]["status"] = .string("failed")
        XCTAssertNoThrow(try decode(value))
        value["thumbnailPreparation"]["selectionCandidate"] = .object([:])
        XCTAssertThrowsError(try decode(value))
        let request = try NativeCurrentImageThumbnailRequest(requestId: jobID, expectedRevision: 3)
        let requestWire = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(request))
        guard case .object(let fields) = requestWire else { XCTFail(); return }
        XCTAssertEqual(Set(fields.keys), ["requestId","expectedRevision"])
        var forged = requestWire; forged["path"] = .string("/unexpected")
        XCTAssertThrowsError(try JSONDecoder().decode(NativeCurrentImageThumbnailRequest.self, from: JSONEncoder().encode(forged)))
    }
}
