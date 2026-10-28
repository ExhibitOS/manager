// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

private actor ModelDrainGate {
    private var entered = false
    private var entry: CheckedContinuation<Void, Never>?
    private var release: CheckedContinuation<Void, Never>?
    func hold() async { entered = true; entry?.resume(); entry = nil; await withCheckedContinuation { release = $0 } }
    func waitForEntry() async { if !entered { await withCheckedContinuation { entry = $0 } } }
    func finish() { release?.resume(); release = nil }
}

private actor ModelImportFixture: NativeModelDerivationTransport {
    let id = UUID().uuidString.lowercased()
    let fullID = UUID().uuidString.lowercased(), reducedID = UUID().uuidString.lowercased()
    let gate: ModelDrainGate?
    var input: NativeImportIntent?
    var calls: [String] = []
    var originalApproved = false
    var cancelResponseLost = false
    var corruptGeneration = false
    init(gate: ModelDrainGate? = nil) { self.gate = gate }
    func configure(approved: Bool = false, lost: Bool = false, corrupt: Bool = false) {
        originalApproved = approved; cancelResponseLost = lost; corruptGeneration = corrupt
    }
    func job(state: String, status: String, drained: Bool, ready: Bool = false) throws -> NativeImportJob {
        let intent = try XCTUnwrap(input), binding = try XCTUnwrap(intent.modelDerivation)
        var model: [String: Any] = ["operationID": binding.operationID, "generation": corruptGeneration ? UUID().uuidString.lowercased() : binding.generation,
            "profileId": binding.profileId, "status": status, "workerDrained": drained]
        var value: [String: Any] = ["id": id, "state": state, "progress": 100, "attempts": 0]
        if state == "approved" {
            value["assetId"] = fullID; model["fullAssetId"] = fullID
            model["full"] = ["assetId": fullID, "sha256": intent.sha256, "bytes": intent.bytes, "reusedExactBytes": true, "triangles": 150_000, "textureSize": 2048]
        }
        if ready {
            model["reducedAssetId"] = reducedID
            model["reduced"] = ["assetId": reducedID, "sha256": String(repeating: "f", count: 64), "bytes": 300, "triangles": 150_000, "textureSize": 512]
        }
        if status == "failed" { model["errorCode"] = "UNSUPPORTED_MODEL_TEXTURE" }
        value["modelDerivation"] = model
        return try JSONDecoder().decode(NativeImportJob.self, from: JSONSerialization.data(withJSONObject: value))
    }
    func createImport(_ input: NativeImportIntent) async throws -> NativeImportJob {
        calls.append("create"); self.input = input
        return try job(state: originalApproved ? "approved" : "queued", status: originalApproved ? "failed" : "pending", drained: originalApproved)
    }
    func uploadImport(_ id: String, bytes: Data) async throws { calls.append("upload") }
    func uploadDerivedImport(_ id: String, bytes: Data) async throws { XCTFail("GLB must not use JPEG derived-byte upload") }
    func importAction(_ id: String, action: NativeImportAction) async throws -> NativeImportJob {
        calls.append(action.rawValue)
        if cancelResponseLost { throw AuthoringFailure("CONNECTION_FAILED") }
        return try job(state: "cancelled", status: "pending", drained: false)
    }
    func importJob(_ id: String) async throws -> NativeImportJob {
        calls.append("poll"); await gate?.hold()
        return try job(state: "cancelled", status: "failed", drained: true)
    }
    func readyJob() throws -> NativeImportJob { try job(state: "approved", status: "ready", drained: true, ready: true) }
    func observed() -> [String] { calls }
}

@MainActor
final class NativeModelDerivationTests: XCTestCase {
    private func intent() -> NativeImportIntent {
        let operation = UUID().uuidString.lowercased()
        return NativeImportIntent(artworkId: UUID().uuidString.lowercased(), idempotencyKey: operation, mime: "model/gltf-binary",
            sha256: String(repeating: "a", count: 64), bytes: 1000, scaleMeters: 1, rights: ArtworkMetadata().rights,
            modelDerivation: NativeModelDerivationIntent(operationID: operation, generation: UUID().uuidString.lowercased(), artworkRevision: 1))
    }
    func testDurableIntentKeepsExactCanonicalBindingAndLegacyOmission() throws {
        XCTAssertEqual(nativeImportSHA256(NativeModelDerivationProfile.settingsBytes), NativeModelDerivationProfile.settingsSHA256)
        let value = intent(), encoded = try JSONEncoder().encode(value)
        XCTAssertEqual(try JSONDecoder().decode(NativeImportIntent.self, from: encoded), value)
        XCTAssertEqual(value.modelDerivation?.operationID, value.idempotencyKey)
        let model = try XCTUnwrap(value.modelDerivation)
        XCTAssertNotEqual(try model.cacheIdentity(sourceSHA256: value.sha256), try model.cacheIdentity(sourceSHA256: String(repeating: "b", count: 64)))
        let legacy = NativeImportIntent(artworkId: value.artworkId, mime: value.mime, sha256: value.sha256, bytes: value.bytes, scaleMeters: 1, rights: value.rights)
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(legacy)) as? [String: Any])
        XCTAssertNil(object["modelDerivation"]); XCTAssertNil(object["derived"])
        XCTAssertFalse(NativeModelDerivationProfile.supports(manifestSHA256: "d27fba8c86ba906dad85ce564458f3a71c9dfc567b4ec59682e023a5eff4de46", ready: true))
        let qualified = "b2ce49e9703cadf7d22b73ec5dfd93622489a07c34721f9bbe2b9f6df17ee28e"
        XCTAssertTrue(NativeModelDerivationProfile.supports(manifestSHA256: qualified, ready: true))
        XCTAssertFalse(NativeModelDerivationProfile.supports(manifestSHA256: qualified, ready: false))
        XCTAssertFalse(NativeModelDerivationProfile.supports(manifestSHA256: String(repeating: "f", count: 64), ready: true))
        XCTAssertTrue(PublicationWatermarkRuntime.supports(manifestSHA256: qualified, ready: true))
        XCTAssertFalse(PublicationWatermarkRuntime.supports(manifestSHA256: qualified, ready: false))
        XCTAssertTrue(PublicationWatermarkRuntime.supports(manifestSHA256: PublicationWatermarkRuntime.qualifiedManifestSHA256, ready: true))
    }
    func testOptionalDerivationFailureRemainsApprovedOriginal() async throws {
        let fixture = ModelImportFixture(), owner = NativeImportTransfer(), value = intent()
        await fixture.configure(approved: true)
        let job = try await owner.run(intent: value, jobID: nil, file: nil, transport: fixture, persistJob: { _ in }, isCurrent: { true })
        XCTAssertEqual(job.state, "approved"); XCTAssertEqual(job.modelDerivation?.status, "failed")
        XCTAssertEqual(job.assetId, job.modelDerivation?.full?.assetId); XCTAssertNil(job.modelDerivation?.reduced)
        XCTAssertFalse(owner.hasPendingModelWorker)
        let calls = await fixture.observed(); XCTAssertEqual(calls, ["create"])
        XCTAssertTrue(job.canRetry)
        XCTAssertFalse(NativeImportJob(id: job.id, state: job.state, progress: job.progress, attempts: 5, errorCode: job.errorCode,
            assetId: job.assetId, retryAt: nil, modelDerivation: job.modelDerivation).canRetry)
        func changedReceipt(missingFull: Bool) throws -> NativeImportJob {
            var object: [String: Any] = ["id": job.id, "state": "approved", "progress": 100, "attempts": 0,
                "assetId": missingFull ? job.assetId! : UUID().uuidString.lowercased()]
            var near: [String: Any] = ["operationID": value.modelDerivation!.operationID,
                "generation": value.modelDerivation!.generation, "profileId": NativeModelDerivationProfile.id,
                "status": "failed", "workerDrained": true, "errorCode": "UNSUPPORTED_MODEL_TEXTURE"]
            if !missingFull {
                near["fullAssetId"] = job.assetId!
                near["full"] = ["assetId": job.assetId!, "sha256": value.sha256, "bytes": value.bytes, "reusedExactBytes": true]
            }
            object["modelDerivation"] = near
            return try JSONDecoder().decode(NativeImportJob.self, from: JSONSerialization.data(withJSONObject: object))
        }
        for missingFull in [true, false] {
            let changed = try changedReceipt(missingFull: missingFull)
            XCTAssertFalse(changed.modelDerivation!.matches(value, job: changed))
            XCTAssertThrowsError(try owner.observe(changed, intent: value))
        }
    }
    func testCancelWaitsForRealWorkerDrainBeforeReplacement() async throws {
        let gate = ModelDrainGate(), fixture = ModelImportFixture(gate: gate), owner = NativeImportTransfer(), value = intent()
        _ = try await owner.run(intent: value, jobID: nil, file: nil, transport: fixture, persistJob: { _ in }, isCurrent: { true })
        var returned = false
        let cancel = Task { await owner.cancelAndWait(); returned = true }
        await gate.waitForEntry()
        XCTAssertFalse(returned); XCTAssertTrue(owner.hasPendingModelWorker)
        do {
            _ = try await owner.run(intent: intent(), jobID: nil, file: nil, transport: fixture, persistJob: { _ in }, isCurrent: { true })
            XCTFail("Replacement cannot overtake the owned worker")
        } catch let error as AuthoringFailure { XCTAssertEqual(error.code, "IMPORT_ACTIVE") }
        await gate.finish(); await cancel.value
        XCTAssertTrue(returned); XCTAssertFalse(owner.hasPendingModelWorker); XCTAssertNil(owner.modelDrainFailure)
    }
    func testLostCancelResponseStillRequiresFreshDrainedStatus() async throws {
        let gate = ModelDrainGate(), fixture = ModelImportFixture(gate: gate), owner = NativeImportTransfer(), value = intent()
        await fixture.configure(lost: true)
        _ = try await owner.run(intent: value, jobID: nil, file: nil, transport: fixture, persistJob: { _ in }, isCurrent: { true })
        let cancel = Task { await owner.cancelAndWait() }
        await gate.waitForEntry(); XCTAssertTrue(owner.hasPendingModelWorker)
        await gate.finish(); await cancel.value
        XCTAssertFalse(owner.hasPendingModelWorker)
        let calls = await fixture.observed(); XCTAssertEqual(calls, ["create", "cancel", "poll"])
    }
    func testMismatchedDrainRetainsDurableOwnerUntilOwnedRuntimeStops() async throws {
        let fixture = ModelImportFixture(), owner = NativeImportTransfer(), value = intent()
        _ = try await owner.run(intent: value, jobID: nil, file: nil, transport: fixture, persistJob: { _ in }, isCurrent: { true })
        await fixture.configure(corrupt: true)
        await owner.cancelAndWait()
        XCTAssertTrue(owner.hasPendingModelWorker); XCTAssertEqual(owner.modelDrainFailure, "IMPORT_RECEIPT_INVALID")
        // The service owner's no-children boundary is separate from HTTP cancel.
        await owner.ownedRuntimeDidStop(); XCTAssertFalse(owner.hasPendingModelWorker)
    }
    func testReadyReuseRequiresCompleteCurrentHashQualifiedInventory() async throws {
        let fixture = ModelImportFixture(), value = intent()
        _ = try await fixture.createImport(value)
        let job = try await fixture.readyJob(), model = try XCTUnwrap(job.modelDerivation)
        XCTAssertFalse(job.canRetry)
        func artwork(changedHash: Bool, revision: Int = 1) throws -> NativeArtwork {
            let metadata = try JSONSerialization.jsonObject(with: JSONEncoder().encode(ArtworkMetadata()))
            return try JSONDecoder().decode(NativeArtwork.self, from: JSONSerialization.data(withJSONObject: [
                "id": value.artworkId, "artistId": UUID().uuidString.lowercased(), "revision": revision, "metadata": metadata, "archived": false,
                "assets": [model.full, model.reduced].compactMap { $0 }.map { ["id": $0.assetId, "mime": "model/gltf-binary", "state": "approved", "sha256": changedHash ? String(repeating: "c", count: 64) : $0.sha256, "bytes": $0.bytes] as [String: Any] }
            ]))
        }
        XCTAssertTrue(model.currentOutputs(in: try artwork(changedHash: false), intent: value, job: job))
        XCTAssertFalse(model.currentOutputs(in: try artwork(changedHash: true), intent: value, job: job))
        XCTAssertFalse(model.currentOutputs(in: try artwork(changedHash: false, revision: 2), intent: value, job: job))
        XCTAssertFalse(model.matches(intent(), job: job))
    }
}
