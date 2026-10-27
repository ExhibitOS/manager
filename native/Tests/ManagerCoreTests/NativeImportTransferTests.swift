// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

private actor ImportTransportFixture: NativeImportTransport {
    let id = UUID().uuidString.lowercased()
    var calls: [String] = []
    var uploaded: Data?
    var completionCode: String?
    var createdState = "uploading"
    var createHook: (@Sendable () async -> Void)?
    var uploadHook: (@Sendable () async -> Void)?
    func configure(code: String? = nil, state: String = "uploading",
                   create: (@Sendable () async -> Void)? = nil,
                   upload: (@Sendable () async -> Void)? = nil) {
        completionCode = code; createdState = state; createHook = create; uploadHook = upload
    }
    func job(_ state: String) throws -> NativeImportJob {
        try JSONDecoder().decode(NativeImportJob.self, from: JSONSerialization.data(withJSONObject: [
            "id": id, "state": state, "progress": 10, "attempts": 0
        ]))
    }
    func createImport(_ input: NativeImportIntent) async throws -> NativeImportJob {
        calls.append("create:\(input.idempotencyKey)")
        await createHook?()
        return try job(createdState)
    }
    func uploadImport(_ id: String, bytes: Data) async throws {
        calls.append("upload"); uploaded = bytes
        await uploadHook?()
    }
    func importAction(_ id: String, action: NativeImportAction) async throws -> NativeImportJob {
        calls.append(action.rawValue)
        if let completionCode { throw AuthoringFailure(completionCode, status: 409) }
        return try job("queued")
    }
    func observed() -> ([String], Data?) { (calls, uploaded) }
}

private actor ImportDrainGate {
    private var entered = false
    private var entry: CheckedContinuation<Void, Never>?
    private var release: CheckedContinuation<Void, Never>?
    func hold() async {
        entered = true; entry?.resume(); entry = nil
        await withCheckedContinuation { release = $0 }
    }
    func waitForEntry() async {
        if !entered { await withCheckedContinuation { entry = $0 } }
    }
    func finish() { release?.resume(); release = nil }
}

@MainActor
final class NativeImportTransferTests: XCTestCase {
    private func intent(_ file: ImportFileSnapshot? = nil) -> NativeImportIntent {
        NativeImportIntent(artworkId: UUID().uuidString.lowercased(), mime: file?.mime ?? "model/gltf-binary",
            sha256: file?.sha256 ?? String(repeating: "a", count: 64), bytes: file?.data.count ?? 3,
            scaleMeters: 1, rights: ArtworkMetadata().rights)
    }
    private func file() async throws -> (URL, ImportFileSnapshot) {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("\(UUID()).glb")
        try Data("abc".utf8).write(to: url)
        return (url, try await ImportFileSnapshot.load(url))
    }
    func testResumeUsesSameIntentAndDurableServerOriginalWithoutLocalFile() async throws {
        let transport = ImportTransportFixture(), transfer = NativeImportTransfer(), input = intent()
        var persisted: String?
        let result = try await transfer.run(intent: input, jobID: nil, file: nil, transport: transport,
            persistJob: { persisted = $0.id }, isCurrent: { true })
        XCTAssertEqual(result.state, "queued"); XCTAssertNil(result.assetId)
        XCTAssertEqual(persisted, result.id)
        let (calls, uploaded) = await transport.observed()
        XCTAssertEqual(calls, ["create:\(input.idempotencyKey)", "complete"]); XCTAssertNil(uploaded)
        XCTAssertNil(transfer.operationID)
    }
    func testMissingOriginalRequestsExactFileAndIntegrityFailureIsNotHidden() async throws {
        for code in ["UPLOAD_INCOMPLETE", "UPLOAD_INTEGRITY"] {
            let transport = ImportTransportFixture(), input = intent()
            await transport.configure(code: code)
            var persisted = false
            do {
                _ = try await NativeImportTransfer().run(intent: input, jobID: nil, file: nil, transport: transport,
                    persistJob: { _ in persisted = true }, isCurrent: { true })
                XCTFail("Missing/corrupt original must not be admitted")
            } catch let error as AuthoringFailure {
                XCTAssertEqual(error.code, code == "UPLOAD_INCOMPLETE" ? "IMPORT_FILE_REQUIRED" : code)
            }
            XCTAssertTrue(persisted)
            let (calls, uploaded) = await transport.observed()
            XCTAssertEqual(calls.count, 2); XCTAssertNil(uploaded)
        }
    }
    func testOriginalSnapshotAndIntentArePersistedBeforeAnyUpload() async throws {
        let (url, snapshot) = try await file(); defer { try? FileManager.default.removeItem(at: url) }
        try Data("changed source".utf8).write(to: url)
        let input = intent(snapshot), transport = ImportTransportFixture()
        var intentSaved = false, jobSaved = false
        _ = try await NativeImportTransfer().run(intent: input, jobID: nil, file: snapshot, transport: transport,
            persistIntent: { intentSaved = true }, persistJob: { _ in
                XCTAssertTrue(intentSaved)
                let (calls, bytes) = await transport.observed()
                XCTAssertEqual(calls, ["create:\(input.idempotencyKey)"]); XCTAssertNil(bytes)
                jobSaved = true
            }, isCurrent: { true })
        let (calls, uploaded) = await transport.observed()
        XCTAssertTrue(jobSaved); XCTAssertEqual(uploaded, Data("abc".utf8))
        XCTAssertEqual(calls, ["create:\(input.idempotencyKey)", "upload", "complete"])
        XCTAssertEqual(try Data(contentsOf: url), Data("changed source".utf8))
    }
    func testPersistenceFailureOrWrongFileCannotSendBytes() async throws {
        let (url, snapshot) = try await file(); defer { try? FileManager.default.removeItem(at: url) }
        for wrongFile in [false, true] {
            let transport = ImportTransportFixture()
            do {
                _ = try await NativeImportTransfer().run(intent: wrongFile ? intent() : intent(snapshot), jobID: nil,
                    file: snapshot, transport: transport, persistIntent: { throw AuthoringFailure("STORE_FAILED") },
                    persistJob: { _ in XCTFail("No job should exist") }, isCurrent: { true })
                XCTFail("Must refuse")
            } catch let error as AuthoringFailure {
                XCTAssertEqual(error.code, wrongFile ? "IMPORT_INTENT_CHANGED" : "STORE_FAILED")
            }
            let (calls, _) = await transport.observed(); XCTAssertEqual(calls, [])
        }
    }
    func testStaleGenerationAndWrongJobCannotContinueOrPersist() async throws {
        let input = intent(), transport = ImportTransportFixture()
        var current = true
        await transport.configure(create: { @MainActor in current = false })
        do {
            _ = try await NativeImportTransfer().run(intent: input, jobID: nil, file: nil, transport: transport,
                persistJob: { _ in XCTFail("Stale result must not persist") }, isCurrent: { current })
            XCTFail("Stale generation must refuse")
        } catch let error as AuthoringFailure { XCTAssertEqual(error.code, "IMPORT_CONTEXT_CHANGED") }
        let second = ImportTransportFixture()
        do {
            _ = try await NativeImportTransfer().run(intent: input, jobID: UUID().uuidString, file: nil, transport: second,
                persistJob: { _ in XCTFail("Foreign job must not persist") }, isCurrent: { true })
            XCTFail("Different job receipt must refuse")
        } catch let error as AuthoringFailure { XCTAssertEqual(error.code, "IMPORT_RECEIPT_INVALID") }
        let (calls, _) = await transport.observed(); XCTAssertEqual(calls.count, 1)
        let (otherCalls, _) = await second.observed(); XCTAssertEqual(otherCalls.count, 1)
    }
    func testExistingFailedCancelledOrQueuedJobDoesNotStartAnotherAttempt() async throws {
        for state in ["failed", "cancelled", "queued", "processing"] {
            let transport = ImportTransportFixture(), input = intent()
            await transport.configure(state: state)
            let result = try await NativeImportTransfer().run(intent: input, jobID: nil, file: nil,
                transport: transport, persistJob: { _ in }, isCurrent: { true })
            XCTAssertEqual(result.state, state); XCTAssertNil(result.assetId)
            let (calls, _) = await transport.observed(); XCTAssertEqual(calls.count, 1)
        }
        let invalid = ImportTransportFixture()
        await invalid.configure(state: "approved") // a status alone cannot prove an asset exists
        do {
            _ = try await NativeImportTransfer().run(intent: intent(), jobID: nil, file: nil,
                transport: invalid, persistJob: { _ in XCTFail("Invalid approval must not persist") }, isCurrent: { true })
            XCTFail("Approved job needs a valid asset ID")
        } catch let error as AuthoringFailure { XCTAssertEqual(error.code, "IMPORT_RECEIPT_INVALID") }
    }
    func testCancellationWaitsForActualUploadDrainAndNeverCompletesPartialOutput() async throws {
        let (url, snapshot) = try await file(); defer { try? FileManager.default.removeItem(at: url) }
        let transport = ImportTransportFixture(), gate = ImportDrainGate(), transfer = NativeImportTransfer()
        await transport.configure(upload: { await gate.hold() })
        let running = Task { try await transfer.run(intent: intent(snapshot), jobID: nil, file: snapshot,
            transport: transport, persistJob: { _ in }, isCurrent: { true }) }
        await gate.waitForEntry()
        var drainStarted = false, drainReturned = false
        let draining = Task { drainStarted = true; await transfer.cancelAndWait(); drainReturned = true }
        // Awaiting a MainActor turn ensures the drain request ran. The gate is
        // deliberately noncooperative until released, modelling in-flight I/O.
        while !drainStarted { await Task.yield() }
        XCTAssertFalse(drainReturned); XCTAssertNotNil(transfer.operationID)
        do {
            _ = try await transfer.run(intent: intent(), jobID: nil, file: nil, transport: transport,
                persistJob: { _ in }, isCurrent: { true })
            XCTFail("Do not replace an undrained operation")
        } catch let error as AuthoringFailure { XCTAssertEqual(error.code, "IMPORT_ACTIVE") }
        await gate.finish(); await draining.value
        do { _ = try await running.value; XCTFail("Cancelled upload must not complete") }
        catch is CancellationError {}
        XCTAssertTrue(drainReturned); XCTAssertNil(transfer.operationID)
        let (calls, _) = await transport.observed(); XCTAssertFalse(calls.contains("complete"))
    }
}
