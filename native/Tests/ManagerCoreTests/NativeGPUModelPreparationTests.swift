// SPDX-License-Identifier: Apache-2.0
import XCTest
import CryptoKit
@testable import ManagerCore

final class NativeGPUModelPreparationTests: XCTestCase {
    let asset = "aaaaaaaa-0000-4000-8000-000000000001"
    let output = "bbbbbbbb-0000-4000-8000-000000000002"
    let artwork = "cccccccc-0000-4000-8000-000000000003"
    let job = "dddddddd-0000-4000-8000-000000000004"
    let hash = String(repeating: "a", count: 64)
    func request() throws -> NativeGPUModelPreparationRequest {
        try .init(revision: 2, assetId: asset, sha256: hash, bytes: 1024, idempotencyKey: "op-1", generation: "gen-1")
    }
    func source() -> DocumentValue {
        .object(["assetId": .string(asset), "mime": .string("model/gltf-binary"), "sha256": .string(hash), "bytes": .integer(1024), "approvalSHA256": .string(hash), "rightsSHA256": .string(hash)])
    }
    func candidate() -> DocumentValue {
        .object(["assetId": .string(output), "mime": .string("model/gltf-binary"), "sha256": .string(hash), "bytes": .integer(8192)])
    }
    func pending() -> DocumentValue {
        .object(["id": .string(job), "state": .string("queued"), "progress": .integer(0), "attempts": .integer(0), "errorCode": .null, "retryAt": .string("2026-11-11T10:00:00Z"), "gpuPreparation": .object(["version": .integer(1), "purpose": .string(NativeGPUModelPreparation.purpose), "profile": .string(NativeGPUModelPreparation.profile), "recipeSHA256": .string(NativeGPUModelPreparation.recipe), "operationID": .string("op-1"), "generation": .string("gen-1"), "artworkRevision": .integer(2), "status": .string("pending"), "workerDrained": .bool(false), "source": source()])])
    }
    func ready() throws -> (DocumentValue, String) {
        let roles: [DocumentValue] = ["baseColor", "normal", "occlusion"].enumerated().map { i, role in
            .object(["role": .string(role), "textureIndex": .integer(Int64(i)), "sourceImageIndex": .integer(Int64(i)), "sourceSHA256": .string(hash), "sourceBytes": .integer(50), "outputPngImageIndex": .integer(Int64(i)), "outputPngSHA256": .string(hash), "outputPngBytes": .integer(50), "width": .integer(512), "height": .integer(512), "colorSpace": .string(i == 0 ? "srgb" : "linear")])
        }
        let producer: DocumentValue = .object(["version": .integer(1), "tool": .object(["name": .string("toktx"), "version": .string("4.4.2"), "executableSHA256": .string(hash), "librarySHA256": .string(hash), "recipeSHA256": .string(NativeGPUModelPreparation.recipe)]), "geometry": .object(["algorithm": .string("glb-source-preservation-v1"), "sourceSHA256": .string(hash), "outputSHA256": .string(hash), "triangles": .integer(20000), "vertices": .integer(30000)]), "samplers": .object(["sourceSHA256": .string(hash), "outputSHA256": .string(hash), "semantics": .string("source-preserved-implicit-repeat-mipmapped")]), "roles": .array(roles), "ktx": .object(["imageIndex": .integer(3), "sha256": .string(hash), "bytes": .integer(900), "width": .integer(512), "height": .integer(512), "levels": .integer(10), "format": .string("uastc"), "transfer": .string("srgb"), "opaque": .bool(true), "levelDimensions": .array([512,256,128,64,32,16,8,4,2,1].map { .integer(Int64($0)) })])])
        var receipt: [String: DocumentValue] = ["version": .integer(1), "purpose": .string(NativeGPUModelPreparation.purpose), "profile": .string(NativeGPUModelPreparation.profile), "recipeSHA256": .string(NativeGPUModelPreparation.recipe), "jobId": .string(job), "artworkId": .string(artwork), "artworkRevision": .integer(2), "operationID": .string("op-1"), "generation": .string("gen-1"), "source": source(), "candidate": candidate(), "producer": producer]
        let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        let digest = SHA256.hash(data: try encoder.encode(DocumentValue.object(receipt))).map { String(format: "%02x", $0) }.joined()
        receipt["sha256"] = .string(digest)
        var result = pending(); result["state"] = .string("approved"); result["progress"] = .integer(100); result["assetId"] = .string(output)
        result["gpuPreparation"]["status"] = .string("ready"); result["gpuPreparation"]["workerDrained"] = .bool(true)
        result["gpuPreparation"]["candidate"] = candidate(); result["gpuPreparation"]["serverReceipt"] = .object(receipt)
        return (result, digest)
    }
    func testPendingAndMixedFamilyDenial() throws {
        let req = try request(); XCTAssertNoThrow(try NativeGPUModelPreparationJob(pending(), artworkId: artwork, request: req))
        var bad = pending(); bad["modelPreparation"] = .object([:]); XCTAssertThrowsError(try NativeGPUModelPreparationJob(bad, artworkId: artwork, request: req))
        bad = pending(); bad["gpuPreparation"]["purpose"] = .string("other"); XCTAssertThrowsError(try NativeGPUModelPreparationJob(bad, artworkId: artwork, request: req))
        bad = pending(); bad["retryAt"] = .null; XCTAssertThrowsError(try NativeGPUModelPreparationJob(bad, artworkId: artwork, request: req))
        bad = pending(); bad["state"] = .string("failed"); bad["gpuPreparation"]["status"] = .string("failed"); bad["gpuPreparation"]["candidate"] = candidate()
        XCTAssertThrowsError(try NativeGPUModelPreparationJob(bad, artworkId: artwork, request: req))
    }
    func testReadyReceiptMustBeBoundDrainedAndExact() throws {
        let (value, receipt) = try ready(), req = try request()
        XCTAssertNoThrow(try NativeGPUModelPreparationJob(value, artworkId: artwork, request: req, verifiedReceiptSHA256: receipt))
        var bad = value; bad["gpuPreparation"]["workerDrained"] = .bool(false); XCTAssertThrowsError(try NativeGPUModelPreparationJob(bad, artworkId: artwork, request: req, verifiedReceiptSHA256: receipt))
        bad = value; bad["gpuPreparation"]["serverReceipt"]["producer"]["tool"]["version"] = .string("4.0"); XCTAssertThrowsError(try NativeGPUModelPreparationJob(bad, artworkId: artwork, request: req, verifiedReceiptSHA256: receipt))
        XCTAssertThrowsError(try NativeGPUModelPreparationJob(value, artworkId: artwork, request: req, verifiedReceiptSHA256: hash))
    }
    func testPriorSelectionIsNotCurrentCandidate() throws {
        let (value, receipt) = try ready(), req = try request()
        let bound = try NativeGPUModelPreparationJob(value, artworkId: artwork, request: req, verifiedReceiptSHA256: receipt)
        XCTAssertTrue(bound.matchesCurrentSourceAuthority(approvalSHA256: hash, rightsSHA256: hash))
        XCTAssertFalse(bound.matchesCurrentSourceAuthority(approvalSHA256: hash, rightsSHA256: String(repeating: "b", count: 64)))
        var selection: DocumentValue = .object(["version": .integer(1), "profile": .string(NativeGPUModelPreparation.profile), "jobId": .string(job), "sourceAssetId": .string(asset), "candidateAssetId": .string(output), "candidateSHA256": .string(hash), "receiptSHA256": .string(receipt), "approvedRevision": .integer(3)])
        XCTAssertTrue(try NativeGPUModelSelection(selection).matchesCandidate(job: bound, request: req))
        let otherRequest = try NativeGPUModelPreparationRequest(revision: 2, assetId: asset, sha256: hash, bytes: 1024, idempotencyKey: "other-op", generation: "gen-1")
        XCTAssertFalse(try NativeGPUModelSelection(selection).matchesCandidate(job: bound, request: otherRequest))
        let n = value["gpuPreparation"]
        let otherArtwork = UUID().uuidString.lowercased()
        let review: DocumentValue = .object(["artworkId": .string(otherArtwork), "revision": .integer(2), "jobId": .string(job), "receiptSHA256": .string(receipt), "source": n["source"], "candidate": n["candidate"], "serverReceipt": n["serverReceipt"], "selection": .null])
        XCTAssertThrowsError(try NativeGPUModelReview(review, artworkId: otherArtwork, request: req, job: bound))
        let result: DocumentValue = .object(["artworkId": .string(otherArtwork), "revision": .integer(3), "jobId": .string(job), "receiptSHA256": .string(receipt), "selection": selection])
        XCTAssertThrowsError(try NativeGPUModelApprovalResult(result, artworkId: otherArtwork, job: bound, request: req))
        selection["jobId"] = .string(UUID().uuidString.lowercased())
        XCTAssertFalse(try NativeGPUModelSelection(selection).matchesCandidate(job: bound, request: req))
    }
    func testRequestLimitsAndScopeBinding() throws {
        XCTAssertThrowsError(try NativeGPUModelPreparationRequest(revision: 0, assetId: asset, sha256: hash, bytes: 10, idempotencyKey: "", generation: "x"))
        for suffix in ["\n", "\r\n", "\u{2028}"] {
            XCTAssertThrowsError(try NativeGPUModelPreparationRequest(revision: 2, assetId: asset, sha256: hash + suffix, bytes: 1024, idempotencyKey: "op-1", generation: "gen-1"))
            XCTAssertThrowsError(try NativeGPUModelPreparationRequest(revision: 2, assetId: asset, sha256: hash, bytes: 1024, idempotencyKey: "op-1" + suffix, generation: "gen-1"))
        }
        let req = try request(), w = UUID().uuidString.lowercased(), t = UUID().uuidString.lowercased(), p = UUID().uuidString.lowercased()
        let transfer = try NativeGPUModelPreparationTransfer(workspaceId: w, tenantId: t, principalId: p, artworkId: artwork, request: req)
        XCTAssertTrue(transfer.matches(workspaceId: w, tenantId: t, principalId: p, artworkId: artwork, request: req, jobId: nil))
        XCTAssertFalse(transfer.matches(workspaceId: w, tenantId: UUID().uuidString, principalId: p, artworkId: artwork, request: req, jobId: nil))
        let encoded = try JSONEncoder().encode(req)
        var object = try XCTUnwrap(JSONSerialization.jsonObject(with: encoded) as? [String: Any])
        object["profile"] = "other"
        XCTAssertThrowsError(try JSONDecoder().decode(NativeGPUModelPreparationRequest.self, from: JSONSerialization.data(withJSONObject: object)))
        object["profile"] = NativeGPUModelPreparation.profile; object["unexpected"] = true
        XCTAssertThrowsError(try JSONDecoder().decode(NativeGPUModelPreparationRequest.self, from: JSONSerialization.data(withJSONObject: object)))
    }
}
