// SPDX-License-Identifier: Apache-2.0
import XCTest
import CryptoKit
@testable import ManagerCore

final class NativeGPUModelPreparationTests: XCTestCase {
    func testRuntimeQualificationRequiresExactVerifiedManifestAndReady() {
        let manifest = "3ee986e7d6601240db6b16f311235bb3a3d43cd2fef00688a740a6c9e91cd377"
        XCTAssertTrue(AuthoringAPI.supportsGPUModelPreparationRuntime(manifestSHA256: manifest, ready: true))
        XCTAssertFalse(AuthoringAPI.supportsGPUModelPreparationRuntime(manifestSHA256: manifest, ready: false))
        XCTAssertFalse(AuthoringAPI.supportsGPUModelPreparationRuntime(manifestSHA256: nil, ready: true))
        XCTAssertFalse(AuthoringAPI.supportsGPUModelPreparationRuntime(manifestSHA256: String(repeating: "a", count: 64), ready: true))
        XCTAssertFalse(AuthoringAPI.supportsGPUModelPreparationRuntime(manifestSHA256: manifest.uppercased(), ready: true))
    }
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
        let encoded = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(request.value))
        XCTAssertEqual(encoded["source"], review.value["source"])
        XCTAssertEqual(encoded["candidate"], review.value["candidate"])
        XCTAssertEqual(encoded["source"]["assetId"], .string(asset))
        XCTAssertEqual(encoded["candidate"]["assetId"], .string(output))
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

    func testApprovalAuthorityRejectsStaleRevokedAndDuplicateDispatch() throws {
        let (jobValue, receipt) = try ready(), req = try request(), owner = UUID().uuidString.lowercased()
        let job = try NativeGPUModelPreparationJob(jobValue, artworkId: artwork, request: req, verifiedReceiptSHA256: receipt)
        let transfer = try NativeGPUModelPreparationTransfer(workspaceId: owner, tenantId: owner, principalId: owner, artworkId: artwork, request: req, jobId: self.job)
        let n = jobValue["gpuPreparation"]
        let review = try NativeGPUModelReview(.object(["artworkId": .string(artwork), "revision": .integer(2), "jobId": .string(self.job), "receiptSHA256": .string(receipt), "source": n["source"], "candidate": n["candidate"], "serverReceipt": n["serverReceipt"], "selection": .null]), artworkId: artwork, request: req, job: job)
        let request = try NativeGPUCandidatePreviewRequest(transfer: transfer, job: job, review: review, epoch: owner, sequence: 1, requestId: UUID().uuidString.lowercased())
        var session = AuthoringSession(userId: owner, tenantId: owner, role: "admin", csrfToken: "synthetic", expiresAt: "2099-01-01T00:00:00Z")
        session.nativeGpuModelPreparationProfile = NativeGPUModelPreparation.profile
        var lifecycle = NativeGPUCandidatePreviewLifecycle(request: request)
        XCTAssertThrowsError(try NativeGPUPreviewApprovalAuthority(lifecycle: lifecycle, transfer: transfer, manifest: fixtureDigest, session: session))
        guard case .object(var fields) = request.value else { return XCTFail("missing request") }
        fields.removeValue(forKey: "phase"); fields["capability"] = .string(NativeGPUCandidatePreviewRequest.capability)
        fields["reviewRevision"] = .integer(2); fields["rendered"] = .bool(true); fields["drawGeneration"] = .integer(1); fields["targetFormat"] = .string("bc7")
        XCTAssertTrue(lifecycle.acceptReady(.object(fields)))
        let authority = try NativeGPUPreviewApprovalAuthority(lifecycle: lifecycle, transfer: transfer, manifest: fixtureDigest, session: session)
        XCTAssertTrue(authority.claim()); XCTAssertFalse(authority.claim())
        XCTAssertTrue(authority.matches(transfer: transfer, session: session, manifest: fixtureDigest, review: review))
        let changed = AuthoringSession(userId: owner, tenantId: owner, role: "admin", csrfToken: "changed", expiresAt: session.expiresAt)
        XCTAssertFalse(authority.matches(transfer: transfer, session: changed, manifest: fixtureDigest, review: review))
        XCTAssertFalse(authority.matches(transfer: transfer, session: session, manifest: String(repeating: "b", count: 64), review: review))
        let different = try NativeGPUModelPreparationTransfer(workspaceId: owner, tenantId: owner, principalId: owner, artworkId: artwork, request: req, jobId: UUID().uuidString.lowercased())
        XCTAssertFalse(authority.matches(transfer: different, session: session, manifest: fixtureDigest, review: review))
        authority.revoke(); XCTAssertFalse(authority.beginDispatch()); XCTAssertFalse(authority.wasDispatched)
        let once = try NativeGPUPreviewApprovalAuthority(lifecycle: lifecycle, transfer: transfer, manifest: fixtureDigest, session: session)
        XCTAssertTrue(once.claim()); XCTAssertTrue(once.beginDispatch()); XCTAssertTrue(once.wasDispatched)
        once.revoke(); XCTAssertFalse(once.beginDispatch()); XCTAssertTrue(once.wasDispatched)
        once.finish(); XCTAssertFalse(once.claim())
    }

    func testUnknownApprovalReconcilesOnlyExactServerSelection() async throws {
        let (jobValue, receipt) = try ready(), req = try request(), owner = UUID().uuidString.lowercased()
        let session = try JSONSerialization.data(withJSONObject: ["tenantId": owner, "userId": owner, "role": "admin", "csrfToken": "synthetic", "expiresAt": "2099-01-01T00:00:00Z"])
        let transfer = try NativeGPUModelPreparationTransfer(workspaceId: owner, tenantId: owner, principalId: owner, artworkId: artwork, request: req, jobId: job)
        let selection: DocumentValue = .object(["version": .integer(1), "profile": .string(NativeGPUModelPreparation.profile), "jobId": .string(job), "sourceAssetId": .string(asset), "candidateAssetId": .string(output), "candidateSHA256": .string(fixtureDigest), "receiptSHA256": .string(receipt), "approvedRevision": .integer(3)])
        let n = jobValue["gpuPreparation"]
        let review: DocumentValue = .object(["artworkId": .string(artwork), "revision": .integer(3), "jobId": .string(job), "receiptSHA256": .string(receipt), "source": n["source"], "candidate": n["candidate"], "serverReceipt": n["serverReceipt"], "selection": selection])
        let selected: DocumentValue = .object(["artworkId": .string(artwork), "revision": .integer(3), "selection": selection])
        let source = NativeArtwork(id: artwork, artistId: owner, revision: 3, approvedRevision: 3, approvedAssetId: asset, metadata: nil,
            assets: [.init(id: asset, mime: "model/gltf-binary", state: "approved", sha256: fixtureDigest, bytes: 1024)], archived: false)
        let jobData = try JSONEncoder().encode(jobValue), reviewData = try JSONEncoder().encode(review)
        let selectedData = try JSONEncoder().encode(selected), artworkData = try JSONEncoder().encode(source)
        let jobId = job, artId = artwork
        GPUApprovalMockProtocol.configure { request in
            switch request.url?.path {
            case "/api/v1/auth/session": return (200, session)
            case "/api/v1/tenants/\(owner)/imports/\(jobId)": return (200, jobData)
            case "/api/v1/tenants/\(owner)/cms/artworks/\(artId)/gpu-preparations/\(jobId)/review": return (200, reviewData)
            case "/api/v1/tenants/\(owner)/cms/artworks/\(artId)/gpu-preparations/selection": return (200, selectedData)
            case "/api/v1/tenants/\(owner)/cms/artworks/\(artId)": return (200, artworkData)
            default: return (500, Data("{}".utf8))
            }
        }
        defer { GPUApprovalMockProtocol.configure(nil) }
        let api = try AuthoringAPI(port: 18091, tenantId: owner, protocolClasses: [GPUApprovalMockProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: owner, principalId: owner, origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        let reconciled = try await api.reconcileGPUModelApproval(transfer: transfer)
        XCTAssertTrue(reconciled)
        XCTAssertFalse(GPUApprovalMockProtocol.methods().contains("POST"))
        var other = selected; other["selection"]["jobId"] = .string(UUID().uuidString.lowercased())
        let otherData = try JSONEncoder().encode(other)
        GPUApprovalMockProtocol.configure { request in
            switch request.url?.path {
            case "/api/v1/tenants/\(owner)/imports/\(jobId)": return (200, jobData)
            case "/api/v1/tenants/\(owner)/cms/artworks/\(artId)/gpu-preparations/\(jobId)/review": return (200, reviewData)
            case "/api/v1/tenants/\(owner)/cms/artworks/\(artId)/gpu-preparations/selection": return (200, otherData)
            default: return (500, Data("{}".utf8))
            }
        }
        let unrelated = try await api.reconcileGPUModelApproval(transfer: transfer)
        XCTAssertFalse(unrelated)
        XCTAssertFalse(GPUApprovalMockProtocol.methods().contains("POST"))
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

private final class GPUApprovalMockProtocol: URLProtocol, @unchecked Sendable {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var handler: (@Sendable (URLRequest) -> (Int, Data))?
    nonisolated(unsafe) private static var seen: [String] = []
    static func configure(_ value: (@Sendable (URLRequest) -> (Int, Data))?) {
        lock.lock(); defer { lock.unlock() }; handler = value; seen = []
    }
    static func methods() -> [String] { lock.lock(); defer { lock.unlock() }; return seen }
    private static func response(_ request: URLRequest) -> (Int, Data)? {
        lock.lock(); defer { lock.unlock() }; seen.append(request.httpMethod ?? ""); return handler?(request)
    }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        guard let url = request.url, let (status, data) = Self.response(request),
              let response = HTTPURLResponse(url: url, statusCode: status, httpVersion: "HTTP/1.1", headerFields: ["Content-Type": "application/json"]) else {
            client?.urlProtocol(self, didFailWithError: URLError(.badServerResponse)); return
        }
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
