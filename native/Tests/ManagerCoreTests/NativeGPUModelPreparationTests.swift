// SPDX-License-Identifier: Apache-2.0
import XCTest
import CryptoKit
@testable import ManagerCore

final class NativeGPUModelPreparationTests: XCTestCase {
    let asset = "aaaaaaaa-0000-4000-8000-000000000001"
    let output = "bbbbbbbb-0000-4000-8000-000000000002"
    let artwork = "cccccccc-0000-4000-8000-000000000003"
    let job = "dddddddd-0000-4000-8000-000000000004"
    let fixtureDigest = String(repeating: "a", count: 64)
    func request() throws -> NativeGPUModelPreparationRequest {
        try .init(revision: 2, assetId: asset, sha256: fixtureDigest, bytes: 1024, idempotencyKey: "op-1", generation: "gen-1")
    }
    func source() -> DocumentValue {
        .object(["assetId": .string(asset), "mime": .string("model/gltf-binary"), "sha256": .string(fixtureDigest), "bytes": .integer(1024), "approvalSHA256": .string(fixtureDigest), "rightsSHA256": .string(fixtureDigest)])
    }
    func candidate() -> DocumentValue {
        .object(["assetId": .string(output), "mime": .string("model/gltf-binary"), "sha256": .string(fixtureDigest), "bytes": .integer(8192)])
    }
    func pending() -> DocumentValue {
        .object(["id": .string(job), "state": .string("queued"), "progress": .integer(0), "attempts": .integer(0), "errorCode": .null, "retryAt": .string("2026-11-11T10:00:00Z"), "gpuPreparation": .object(["version": .integer(1), "purpose": .string(NativeGPUModelPreparation.purpose), "profile": .string(NativeGPUModelPreparation.profile), "recipeSHA256": .string(NativeGPUModelPreparation.recipe), "operationID": .string("op-1"), "generation": .string("gen-1"), "artworkRevision": .integer(2), "status": .string("pending"), "workerDrained": .bool(false), "source": source()])])
    }
    func ready() throws -> (DocumentValue, String) {
        let roles: [DocumentValue] = ["baseColor", "normal", "occlusion"].enumerated().map { i, role in
            .object(["role": .string(role), "textureIndex": .integer(Int64(i)), "sourceImageIndex": .integer(Int64(i)), "sourceSHA256": .string(fixtureDigest), "sourceBytes": .integer(50), "outputPngImageIndex": .integer(Int64(i)), "outputPngSHA256": .string(fixtureDigest), "outputPngBytes": .integer(50), "width": .integer(512), "height": .integer(512), "colorSpace": .string(i == 0 ? "srgb" : "linear")])
        }
        let producer: DocumentValue = .object(["version": .integer(1), "tool": .object(["name": .string("toktx"), "version": .string("4.4.2"), "executableSHA256": .string(NativeGPUModelPreparation.encoderSHA256), "librarySHA256": .string(NativeGPUModelPreparation.encoderLibrarySHA256), "recipeSHA256": .string(NativeGPUModelPreparation.recipe)]), "geometry": .object(["algorithm": .string("glb-source-preservation-v1"), "sourceSHA256": .string(fixtureDigest), "outputSHA256": .string(fixtureDigest), "triangles": .integer(20000), "vertices": .integer(30000)]), "samplers": .object(["sourceSHA256": .string(fixtureDigest), "outputSHA256": .string(fixtureDigest), "semantics": .string("source-preserved-implicit-repeat-mipmapped")]), "roles": .array(roles), "ktx": .object(["imageIndex": .integer(3), "sha256": .string(fixtureDigest), "bytes": .integer(900), "width": .integer(512), "height": .integer(512), "levels": .integer(10), "format": .string("uastc"), "transfer": .string("srgb"), "opaque": .bool(true), "levelDimensions": .array([512,256,128,64,32,16,8,4,2,1].map { .integer(Int64($0)) })])])
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
        XCTAssertThrowsError(try NativeGPUModelPreparationJob(value, artworkId: artwork, request: req, verifiedReceiptSHA256: fixtureDigest))
        var producer = value["gpuPreparation"]["serverReceipt"]["producer"]
        XCTAssertTrue(NativeGPUModelPreparation.producer(producer))
        producer["tool"]["executableSHA256"] = .string(fixtureDigest)
        XCTAssertFalse(NativeGPUModelPreparation.producer(producer))
        producer = value["gpuPreparation"]["serverReceipt"]["producer"]
        producer["ktx"]["imageIndex"] = .integer(0)
        XCTAssertFalse(NativeGPUModelPreparation.producer(producer))
    }
    func testHistoricalVendorRecipeIsRejected() throws {
        let req = try request()
        var queued = pending()
        queued["gpuPreparation"]["recipeSHA256"] = .string(NativeGPUModelPreparation.historicalVendorRecipe)
        XCTAssertThrowsError(try NativeGPUModelPreparationJob(queued, artworkId: artwork, request: req))
        func refreshedReceiptDigest(_ value: DocumentValue) throws -> (DocumentValue, String) {
            var result = value
            guard case .object(var fields) = value["gpuPreparation"]["serverReceipt"] else { throw NativeGPUModelPreparation.invalid() }
            fields.removeValue(forKey: "sha256")
            let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
            let digest = SHA256.hash(data: try encoder.encode(DocumentValue.object(fields))).map { String(format: "%02x", $0) }.joined()
            result["gpuPreparation"]["serverReceipt"]["sha256"] = .string(digest)
            return (result, digest)
        }
        let (value, _) = try ready()
        var oldReceipt = value
        oldReceipt["gpuPreparation"]["serverReceipt"]["recipeSHA256"] = .string(NativeGPUModelPreparation.historicalVendorRecipe)
        let (oldReceiptValue, oldReceiptDigest) = try refreshedReceiptDigest(oldReceipt)
        XCTAssertThrowsError(try NativeGPUModelPreparationJob(oldReceiptValue, artworkId: artwork, request: req, verifiedReceiptSHA256: oldReceiptDigest))
        var oldTool = value
        oldTool["gpuPreparation"]["serverReceipt"]["producer"]["tool"]["recipeSHA256"] = .string(NativeGPUModelPreparation.historicalVendorRecipe)
        let (oldToolValue, oldToolDigest) = try refreshedReceiptDigest(oldTool)
        XCTAssertThrowsError(try NativeGPUModelPreparationJob(oldToolValue, artworkId: artwork, request: req, verifiedReceiptSHA256: oldToolDigest))
    }
    func testPriorSelectionIsNotCurrentCandidate() throws {
        let (value, receipt) = try ready(), req = try request()
        let bound = try NativeGPUModelPreparationJob(value, artworkId: artwork, request: req, verifiedReceiptSHA256: receipt)
        XCTAssertTrue(bound.matchesCurrentSourceAuthority(approvalSHA256: fixtureDigest, rightsSHA256: fixtureDigest))
        XCTAssertFalse(bound.matchesCurrentSourceAuthority(approvalSHA256: fixtureDigest, rightsSHA256: String(repeating: "b", count: 64)))
        var selection: DocumentValue = .object(["version": .integer(1), "profile": .string(NativeGPUModelPreparation.profile), "jobId": .string(job), "sourceAssetId": .string(asset), "candidateAssetId": .string(output), "candidateSHA256": .string(fixtureDigest), "receiptSHA256": .string(receipt), "approvedRevision": .integer(3)])
        XCTAssertTrue(try NativeGPUModelSelection(selection).matchesCandidate(job: bound, request: req))
        let otherRequest = try NativeGPUModelPreparationRequest(revision: 2, assetId: asset, sha256: fixtureDigest, bytes: 1024, idempotencyKey: "other-op", generation: "gen-1")
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

    func testCommittedApprovalReplayRequiresExactSelection() throws {
        let (value, receipt) = try ready(), req = try request()
        let bound = try NativeGPUModelPreparationJob(value, artworkId: artwork, request: req, verifiedReceiptSHA256: receipt)
        let n = value["gpuPreparation"]
        let selection: DocumentValue = .object(["version": .integer(1), "profile": .string(NativeGPUModelPreparation.profile), "jobId": .string(job), "sourceAssetId": .string(asset), "candidateAssetId": .string(output), "candidateSHA256": .string(fixtureDigest), "receiptSHA256": .string(receipt), "approvedRevision": .integer(3)])
        var review: DocumentValue = .object(["artworkId": .string(artwork), "revision": .integer(3), "jobId": .string(job), "receiptSHA256": .string(receipt), "source": n["source"], "candidate": n["candidate"], "serverReceipt": n["serverReceipt"], "selection": selection])
        let valid = try NativeGPUModelReview(review, artworkId: artwork, request: req, job: bound)
        XCTAssertEqual(try NativeGPUModelApprovalRequest(review: valid, request: req).revision, 2)
        review["selection"]["jobId"] = .string(UUID().uuidString.lowercased())
        XCTAssertThrowsError(try NativeGPUModelReview(review, artworkId: artwork, request: req, job: bound))
        review["selection"] = .null
        XCTAssertThrowsError(try NativeGPUModelReview(review, artworkId: artwork, request: req, job: bound))
        review["revision"] = .integer(4); review["selection"] = selection
        XCTAssertThrowsError(try NativeGPUModelReview(review, artworkId: artwork, request: req, job: bound))
    }

    func testTransferClearRequiresMatchingTerminalAuthority() throws {
        let req = try request(), owner = UUID().uuidString.lowercased()
        let transfer = try NativeGPUModelPreparationTransfer(workspaceId: owner, tenantId: owner, principalId: owner, artworkId: artwork, request: req, jobId: job)
        var cancelled = pending()
        cancelled["state"] = .string("cancelled"); cancelled["gpuPreparation"]["status"] = .string("cancelled")
        let undrained = try NativeGPUModelPreparationJob(cancelled, artworkId: artwork, request: req)
        XCTAssertFalse(transfer.permitsClearingCancelled(undrained))
        cancelled["gpuPreparation"]["workerDrained"] = .bool(true)
        let drained = try NativeGPUModelPreparationJob(cancelled, artworkId: artwork, request: req)
        XCTAssertTrue(transfer.permitsClearingCancelled(drained))
        cancelled["id"] = .string(UUID().uuidString.lowercased())
        let otherJob = try NativeGPUModelPreparationJob(cancelled, artworkId: artwork, request: req)
        XCTAssertFalse(transfer.permitsClearingCancelled(otherJob))
        let (value, receipt) = try ready()
        let readyJob = try NativeGPUModelPreparationJob(value, artworkId: artwork, request: req, verifiedReceiptSHA256: receipt)
        XCTAssertFalse(transfer.permitsClearingSelected(readyJob, selection: nil))
        let selected = try NativeGPUModelSelection(.object(["version": .integer(1), "profile": .string(NativeGPUModelPreparation.profile), "jobId": .string(job), "sourceAssetId": .string(asset), "candidateAssetId": .string(output), "candidateSHA256": .string(fixtureDigest), "receiptSHA256": .string(receipt), "approvedRevision": .integer(3)]))
        XCTAssertTrue(transfer.permitsClearingSelected(readyJob, selection: selected))
    }

    func testPreviewReadyInvalidationAndDrainCannotResurrect() throws {
        let (jobValue, receipt) = try ready(), req = try request(), owner = UUID().uuidString.lowercased()
        let job = try NativeGPUModelPreparationJob(jobValue, artworkId: artwork, request: req, verifiedReceiptSHA256: receipt)
        let transfer = try NativeGPUModelPreparationTransfer(workspaceId: owner, tenantId: owner, principalId: owner, artworkId: artwork, request: req, jobId: self.job)
        let n = jobValue["gpuPreparation"]
        let reviewValue: DocumentValue = .object(["artworkId": .string(artwork), "revision": .integer(2), "jobId": .string(self.job), "receiptSHA256": .string(receipt), "source": n["source"], "candidate": n["candidate"], "serverReceipt": n["serverReceipt"], "selection": .null])
        let review = try NativeGPUModelReview(reviewValue, artworkId: artwork, request: req, job: job)
        let request = try NativeGPUCandidatePreviewRequest(transfer: transfer, job: job, review: review, epoch: owner, sequence: 1, requestId: UUID().uuidString.lowercased())
        guard case .object(var reply) = request.value else { return XCTFail("missing request") }
        reply.removeValue(forKey: "phase")
        reply["capability"] = .string(NativeGPUCandidatePreviewRequest.capability); reply["reviewRevision"] = .integer(2)
        reply["rendered"] = .bool(true); reply["drawGeneration"] = .integer(1); reply["targetFormat"] = .string("bc7")
        let ready = DocumentValue.object(reply)
        var lifecycle = NativeGPUCandidatePreviewLifecycle(request: request)
        XCTAssertTrue(lifecycle.acceptReady(ready)); XCTAssertTrue(lifecycle.rendered)
        let v = request.value
        var ended: DocumentValue = .object(["version": .integer(1), "epoch": v["epoch"], "sequence": v["sequence"], "type": .string("gpu-candidate-preview-ended"), "body": .object(["requestId": v["requestId"], "workspaceId": v["workspaceId"], "tenantId": v["tenantId"], "principalId": v["principalId"], "artworkId": v["artworkId"], "artworkRevision": v["artworkRevision"], "jobId": v["jobId"], "generation": v["generation"], "receiptSHA256": v["receiptSHA256"], "candidateSHA256": v["candidate"]["sha256"], "reason": .string("hidden"), "drained": .bool(false)])])
        XCTAssertTrue(lifecycle.acceptEnded(ended)); XCTAssertFalse(lifecycle.rendered); XCTAssertFalse(lifecycle.drained)
        XCTAssertFalse(lifecycle.acceptReady(ready))
        ended["body"]["drained"] = .bool(true)
        XCTAssertTrue(lifecycle.acceptEnded(ended)); XCTAssertTrue(lifecycle.drained)
        ended["body"]["drained"] = .bool(false)
        XCTAssertTrue(lifecycle.acceptEnded(ended)); XCTAssertTrue(lifecycle.drained)
        ended["body"]["jobId"] = .string(UUID().uuidString.lowercased())
        XCTAssertFalse(lifecycle.acceptEnded(ended))
    }

    func testPreviewReadyAndCancelRequireExactIdentityAndFormat() throws {
        let (jobValue, receipt) = try ready(), req = try request(), owner = UUID().uuidString.lowercased()
        let job = try NativeGPUModelPreparationJob(jobValue, artworkId: artwork, request: req, verifiedReceiptSHA256: receipt)
        let transfer = try NativeGPUModelPreparationTransfer(workspaceId: owner, tenantId: owner, principalId: owner, artworkId: artwork, request: req, jobId: self.job)
        let n = jobValue["gpuPreparation"]
        let review = try NativeGPUModelReview(.object(["artworkId": .string(artwork), "revision": .integer(2), "jobId": .string(self.job), "receiptSHA256": .string(receipt), "source": n["source"], "candidate": n["candidate"], "serverReceipt": n["serverReceipt"], "selection": .null]), artworkId: artwork, request: req, job: job)
        let request = try NativeGPUCandidatePreviewRequest(transfer: transfer, job: job, review: review, epoch: owner, sequence: 3, requestId: UUID().uuidString.lowercased())
        guard case .object(var result) = request.value, case .object(var cancel) = request.cancel else { return XCTFail("invalid fixture") }
        result.removeValue(forKey: "phase"); result["capability"] = .string(NativeGPUCandidatePreviewRequest.capability)
        result["reviewRevision"] = .integer(2); result["rendered"] = .bool(true); result["drawGeneration"] = .integer(1); result["targetFormat"] = .string("astc-4x4")
        XCTAssertTrue(request.acceptsReady(.object(result)))
        result["targetFormat"] = .string("png"); XCTAssertFalse(request.acceptsReady(.object(result)))
        result["targetFormat"] = .string("bc7"); result["requestId"] = .string(UUID().uuidString.lowercased()); XCTAssertFalse(request.acceptsReady(.object(result)))
        cancel.removeValue(forKey: "phase"); cancel["cancelled"] = .bool(true); cancel["drained"] = .bool(true)
        XCTAssertTrue(request.acceptsCancelled(.object(cancel)))
        cancel["drained"] = .bool(false); XCTAssertFalse(request.acceptsCancelled(.object(cancel)))
    }

    func testPrivatePreviewPolicyIsBoundToOneCandidateAndExactAssets() throws {
        let (jobValue, receipt) = try ready(), req = try request(), owner = UUID().uuidString.lowercased()
        let job = try NativeGPUModelPreparationJob(jobValue, artworkId: artwork, request: req, verifiedReceiptSHA256: receipt)
        let transfer = try NativeGPUModelPreparationTransfer(workspaceId: owner, tenantId: owner, principalId: owner, artworkId: artwork, request: req, jobId: self.job)
        let n = jobValue["gpuPreparation"]
        let review = try NativeGPUModelReview(.object(["artworkId": .string(artwork), "revision": .integer(2), "jobId": .string(self.job), "receiptSHA256": .string(receipt), "source": n["source"], "candidate": n["candidate"], "serverReceipt": n["serverReceipt"], "selection": .null]), artworkId: artwork, request: req, job: job)
        let preview = try NativeGPUCandidatePreviewRequest(transfer: transfer, job: job, review: review, epoch: owner, sequence: 1, requestId: UUID().uuidString.lowercased())
        let origin = "http://127.0.0.1:13200"
        let rules = try NativeGPUCandidatePreviewPolicy.encodedRules(origin: origin, request: preview, verifiedAssets: ["assets/index-123.js"])
        let items = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(rules.utf8)) as? [[String: Any]])
        let filters = try items.dropFirst().map { try XCTUnwrap(($0["trigger"] as? [String: Any])?["url-filter"] as? String) }
        func allows(_ path: String) -> Bool { filters.contains { (origin + path).range(of: $0, options: .regularExpression) != nil } }
        let prefix = "/api/v1/tenants/\(owner)/cms/artworks/\(artwork)/gpu-preparations/\(self.job)"
        let model = prefix + "/model?receiptSHA256=\(receipt)&candidateSHA256=\(fixtureDigest)"
        XCTAssertTrue(allows(model)); XCTAssertTrue(allows(prefix + "/review")); XCTAssertTrue(allows("/assets/index-123.js"))
        XCTAssertTrue(allows("/basis/basis_transcoder.wasm"))
        XCTAssertFalse(allows(model + "&extra=1")); XCTAssertFalse(allows(prefix + "/approve")); XCTAssertFalse(allows("/assets/other.js"))
        XCTAssertFalse(allows(prefix.replacingOccurrences(of: self.job, with: UUID().uuidString.lowercased()) + "/review"))
        XCTAssertThrowsError(try NativeGPUCandidatePreviewPolicy.encodedRules(origin: origin, request: preview, verifiedAssets: ["assets/../other.js"]))
    }

    func testPreviewResourcesRequireExactOwnedManifestAndClosure() throws {
        let manifest = String(repeating: "a", count: 64)
        let fixed = ["native-viewport.html", "basis/basis_transcoder.js", "basis/basis_transcoder.wasm", "basis/exhibitos-basis-worker.js", "assets/index-123.js"]
        func decode(_ paths: [String], _ digest: String = manifest) throws -> NativeGPUPreviewResources {
            try JSONDecoder().decode(NativeGPUPreviewResources.self, from: JSONSerialization.data(withJSONObject: ["format": 1, "payloadManifestSha256": digest, "paths": paths]))
        }
        XCTAssertEqual(try decode(fixed).checked(expectedManifestSHA256: manifest), ["assets/index-123.js"])
        XCTAssertThrowsError(try decode(fixed).checked(expectedManifestSHA256: String(repeating: "b", count: 64)))
        XCTAssertThrowsError(try decode(fixed + [fixed[0]]).checked(expectedManifestSHA256: manifest))
        XCTAssertThrowsError(try decode(Array(fixed.dropFirst())).checked(expectedManifestSHA256: manifest))
        XCTAssertThrowsError(try decode(fixed + ["api/v1/auth/session"]).checked(expectedManifestSHA256: manifest))
    }
    func testRequestLimitsAndScopeBinding() throws {
        XCTAssertThrowsError(try NativeGPUModelPreparationRequest(revision: 0, assetId: asset, sha256: fixtureDigest, bytes: 10, idempotencyKey: "", generation: "x"))
        for suffix in ["\n", "\r\n", "\u{2028}"] {
            XCTAssertThrowsError(try NativeGPUModelPreparationRequest(revision: 2, assetId: asset, sha256: fixtureDigest + suffix, bytes: 1024, idempotencyKey: "op-1", generation: "gen-1"))
            XCTAssertThrowsError(try NativeGPUModelPreparationRequest(revision: 2, assetId: asset, sha256: fixtureDigest, bytes: 1024, idempotencyKey: "op-1" + suffix, generation: "gen-1"))
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
