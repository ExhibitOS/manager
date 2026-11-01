// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

private actor OEXMockTransport: NativeOEXImportTransport {
    var ids: [String] = []
    var shouldLoseFirstReply = true
    let jobId = UUID().uuidString.lowercased()
    func createOEXImport(requestId: String, bytes: Int, sha256: String) async throws -> NativeOEXImportJob {
        ids.append(requestId)
        if shouldLoseFirstReply { shouldLoseFirstReply = false; throw AuthoringFailure("REQUEST_FAILED") }
        return NativeOEXImportJob(id: jobId, state: "uploading", errorCode: nil, attempts: 0, result: nil, cleanupPending: false)
    }
    func oexImportJob(_ id: String) async throws -> NativeOEXImportJob {
        XCTAssertEqual(id, jobId)
        return NativeOEXImportJob(id: jobId, state: "queued", errorCode: nil, attempts: 0, result: nil, cleanupPending: false)
    }
    func uploadOEXImport(_ id: String, archive: NativeOEXArchive) async throws { XCTAssertEqual(id, jobId) }
    func oexImportAction(_ id: String, action: String) async throws -> NativeOEXImportJob {
        XCTAssertEqual(id, jobId); XCTAssertEqual(action, "complete")
        return NativeOEXImportJob(id: jobId, state: "queued", errorCode: nil, attempts: 0, result: nil, cleanupPending: false)
    }
    func observedIDs() -> [String] { ids }
}

private actor BoundedOEXMock: NativeBoundedOEXTransport {
    enum Behavior { case loseCreate, created, cancelAfterCreate }
    let behavior: Behavior
    let jobId = UUID().uuidString.lowercased()
    private var requests = 0
    private var uploads = 0
    init(_ behavior: Behavior) { self.behavior = behavior }
    func createBoundedOEXImport(requestId: String, archive: NativeOEXArchive) async throws -> NativeBoundedOEXJob {
        requests += 1
        if case .loseCreate = behavior { throw AuthoringFailure("REQUEST_FAILED") }
        if case .cancelAfterCreate = behavior { withUnsafeCurrentTask { $0?.cancel() } }
        return NativeBoundedOEXJob(id: jobId, state: "uploading")
    }
    func uploadBoundedOEXImport(_ id: String, archive: NativeOEXArchive) async throws { uploads += 1 }
    func queueBoundedOEXImport(_ id: String) async throws -> NativeBoundedOEXJob { NativeBoundedOEXJob(id: jobId, state: "queued") }
    func observed() -> (Int, Int) { (requests, uploads) }
}

final class OEXImportTests: XCTestCase {
    @MainActor
    func testSelectedCreateFailureAndCancellationPreserveIntentWithoutRetry() async throws {
        let id = UUID().uuidString.lowercased()
        let empty = NativeOEXImportState(format: 1, revision: 0, workspaceId: id, tenantId: id, principalId: id, intent: nil, jobId: nil, fileName: nil)
        let archive = try NativeOEXArchive(data: Data([0x50, 0x4b, 0x03, 0x04] + Array(repeating: 0, count: 20)))
        let file = NativeOEXImportFile(name: "synthetic.oex", archive: archive)
        var durable = empty
        let lost = BoundedOEXMock(.loseCreate)
        let persist: @MainActor (NativeOEXImportState) async throws -> NativeOEXImportState = { value in
            var next = value; next.revision += 1; durable = next; return next
        }
        do {
            _ = try await NativeOEXImportTransfer.runBoundedOnce(state: empty, file: file, api: lost, persist: persist, select: {}, isCurrent: { true })
            XCTFail("lost creation reply must be uncertain")
        } catch let error as NativeBoundedOEXUncertain { XCTAssertNil(error.jobId); XCTAssertEqual(error.requestId, durable.intent?.requestId) }
        XCTAssertNotNil(durable.intent); XCTAssertNil(durable.jobId)
        do {
            _ = try await NativeOEXImportTransfer.runBoundedOnce(state: durable, file: file, api: lost, persist: persist, select: {}, isCurrent: { true })
            XCTFail("durable uncertain intent must not create a second job")
        } catch {}
        let lostCalls = await lost.observed(); XCTAssertEqual(lostCalls.0, 1); XCTAssertEqual(lostCalls.1, 0)

        var retained = empty
        let created = BoundedOEXMock(.created)
        let createdId = await created.jobId
        var writes = 0
        let failSecond: @MainActor (NativeOEXImportState) async throws -> NativeOEXImportState = { value in
            writes += 1; if writes == 2 { throw AuthoringFailure("STATE_UNAVAILABLE") }
            var next = value; next.revision += 1; retained = next; return next
        }
        do {
            _ = try await NativeOEXImportTransfer.runBoundedOnce(state: empty, file: file, api: created, persist: failSecond, select: {}, isCurrent: { true })
            XCTFail("failed job journal write must remain uncertain")
        } catch let error as NativeBoundedOEXUncertain { XCTAssertEqual(error.jobId, createdId); XCTAssertEqual(error.requestId, retained.intent?.requestId) }
        let createdCalls = await created.observed(); XCTAssertEqual(createdCalls.0, 1); XCTAssertEqual(createdCalls.1, 0)
        XCTAssertNotNil(retained.intent); XCTAssertNil(retained.jobId)

        var cancelledState = empty
        let cancelled = BoundedOEXMock(.created)
        let task = Task { @MainActor in
            try await NativeOEXImportTransfer.runBoundedOnce(state: empty, file: file, api: cancelled, persist: { value in
                var next = value; next.revision += 1; cancelledState = next; return next
            }, select: { withUnsafeCurrentTask { $0?.cancel() } }, isCurrent: { true })
        }
        do { _ = try await task.value; XCTFail("cancelled selected operation must stop before create") } catch is CancellationError {}
        XCTAssertNotNil(cancelledState.intent); XCTAssertNil(cancelledState.jobId)
        let cancelledCalls = await cancelled.observed(); XCTAssertEqual(cancelledCalls.0, 0); XCTAssertEqual(cancelledCalls.1, 0)

        var afterResponse = empty
        let responseCancelled = BoundedOEXMock(.cancelAfterCreate)
        let responseId = await responseCancelled.jobId
        let afterTask = Task { @MainActor in
            try await NativeOEXImportTransfer.runBoundedOnce(state: empty, file: file, api: responseCancelled, persist: { value in
                try Task.checkCancellation()
                var next = value; next.revision += 1; afterResponse = next; return next
            }, select: {}, isCurrent: { true })
        }
        do { _ = try await afterTask.value; XCTFail("cancelled operation must stop after recording returned job") } catch is CancellationError {}
        XCTAssertNotNil(afterResponse.intent); XCTAssertEqual(afterResponse.jobId, responseId)
        let afterCalls = await responseCancelled.observed(); XCTAssertEqual(afterCalls.0, 1); XCTAssertEqual(afterCalls.1, 0)
    }
    func testBoundedChunksPreserveOrderAndRejectChangedSnapshot() throws {
        let limit = 16 * 1024 * 1024
        var bytes = Data(repeating: 0x31, count: limit + 7)
        bytes.replaceSubrange(0..<4, with: [0x50, 0x4b, 0x03, 0x04])
        let archive = try NativeOEXArchive(data: bytes)
        XCTAssertEqual(try archive.boundedChunk(index: 0), bytes.prefix(limit))
        XCTAssertEqual(try archive.boundedChunk(index: 1), bytes.suffix(7))
        XCTAssertThrowsError(try archive.boundedChunk(index: 2))
        let handle = try FileHandle(forWritingTo: archive.fileURL)
        try handle.seekToEnd(); try handle.write(contentsOf: Data([0x32])); try handle.close()
        XCTAssertThrowsError(try archive.boundedChunk(index: 0))
    }
    func testSnapshotRequiresLocalOEXRegularFileAndPreservesBytes() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("sample.oex")
        let bytes = Data([0x50, 0x4b, 0x03, 0x04] + Array(repeating: 0, count: 20))
        try bytes.write(to: file)
        let snapshot = try await NativeOEXImportFile.load(file)
        XCTAssertEqual(snapshot.name, "sample.oex")
        XCTAssertEqual(try Data(contentsOf: snapshot.archive.fileURL), bytes)
        try Data(repeating: 9, count: bytes.count).write(to: file)
        XCTAssertEqual(try Data(contentsOf: snapshot.archive.fileURL), bytes, "the selected snapshot is independent of subsequent source edits")
        XCTAssertEqual(snapshot.archive.count, bytes.count)
        let link = root.appendingPathComponent("link.oex")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: file)
        do { _ = try await NativeOEXImportFile.load(link); XCTFail("final symlink must be refused") } catch {}
        do { _ = try await NativeOEXImportFile.load(root.appendingPathComponent("sample.png")); XCTFail("other extension must be refused") } catch {}
    }
    func testDurableIntentSupportsOnlyExactProfileAndPreservesLegacyShape() throws {
        let id = UUID().uuidString.lowercased(), hash = String(repeating: "a", count: 64)
        let legacy = NativeOEXImportIntent(requestId: id, sha256: hash, bytes: 64 * 1024 * 1024)
        let json = try JSONSerialization.jsonObject(with: JSONEncoder().encode(legacy)) as! [String: Any]
        XCTAssertEqual(Set(json.keys), Set(["requestId", "sha256", "bytes"]))
        XCTAssertTrue(try JSONDecoder().decode(NativeOEXImportIntent.self, from: JSONEncoder().encode(legacy)).hasSupportedProfile)
        XCTAssertTrue(NativeOEXImportIntent(requestId: id, sha256: hash, bytes: 512 * 1024 * 1024, profile: "bounded-file-v1", oexFormat: "1.0.0-draft.9").hasSupportedProfile)
        XCTAssertFalse(NativeOEXImportIntent(requestId: id, sha256: hash, bytes: 64 * 1024 * 1024 + 1).hasSupportedProfile)
        XCTAssertFalse(NativeOEXImportIntent(requestId: id, sha256: hash, bytes: 1, profile: "bounded-file-v1").hasSupportedProfile)
        XCTAssertFalse(NativeOEXImportIntent(requestId: id, sha256: hash, bytes: 1, profile: "bounded-file-v1", oexFormat: "1.0.0-draft.8").hasSupportedProfile)
        XCTAssertFalse(NativeOEXImportIntent(requestId: id, sha256: hash, bytes: 512 * 1024 * 1024 + 1, profile: "bounded-file-v1", oexFormat: "1.0.0-draft.9").hasSupportedProfile)
    }
    func testJobRequiresBoundedStateAndValidCompletedExhibition() throws {
        let decoder = JSONDecoder()
        func job(_ json: String) throws -> NativeOEXImportJob { try decoder.decode(NativeOEXImportJob.self, from: Data(json.utf8)) }
        let id = UUID().uuidString.lowercased(), exhibition = UUID().uuidString.lowercased()
        XCTAssertNoThrow(try job(#"{"id":"\#(id)","state":"complete","attempts":1,"result":{"exhibitionId":"\#(exhibition)"},"cleanupPending":true}"#).checked())
        XCTAssertThrowsError(try job(#"{"id":"\#(id)","state":"complete","attempts":1,"result":null,"cleanupPending":false}"#).checked())
        XCTAssertThrowsError(try job(#"{"id":"\#(id)","state":"queued","attempts":1,"result":{"exhibitionId":"\#(exhibition)"},"cleanupPending":false}"#).checked())
        XCTAssertThrowsError(try job(#"{"id":"\#(id)","state":"unexpected","attempts":1,"result":null,"cleanupPending":false}"#).checked())
    }
    @MainActor
    func testLargeProfileJournalCannotStartUnnegotiatedLegacyTransfer() async throws {
        let id = UUID().uuidString.lowercased()
        let archive = try NativeOEXArchive(data: Data([0x50, 0x4b, 0x03, 0x04] + Array(repeating: 0, count: 20)))
        let intent = NativeOEXImportIntent(requestId: id, sha256: archive.sha256, bytes: archive.count, profile: "bounded-file-v1", oexFormat: "1.0.0-draft.9")
        let state = NativeOEXImportState(format: 1, revision: 0, workspaceId: id, tenantId: id, principalId: id, intent: intent, jobId: nil, fileName: "sample.oex")
        let transport = OEXMockTransport()
        do {
            _ = try await NativeOEXImportTransfer.run(state: state, file: NativeOEXImportFile(name: "sample.oex", archive: archive), transport: transport, persist: { $0 }, isCurrent: { true })
            XCTFail("profile must not use legacy wire")
        } catch let error as AuthoringFailure { XCTAssertEqual(error.code, "OEX_IMPORT_PROFILE_UNAVAILABLE") }
        let requests = await transport.observedIDs(); XCTAssertTrue(requests.isEmpty)
    }
    @MainActor
    func testLostCreateReplyRetainsSameDurableRequestAndFileBinding() async throws {
        let id = UUID().uuidString.lowercased()
        var saved = NativeOEXImportState(format: 1, revision: 0, workspaceId: id, tenantId: id, principalId: id, intent: nil, jobId: nil, fileName: nil)
        let archive = try NativeOEXArchive(data: Data([0x50, 0x4b, 0x03, 0x04] + Array(repeating: 0, count: 20)))
        let file = NativeOEXImportFile(name: "same.oex", archive: archive), transport = OEXMockTransport()
        let persist: @MainActor (NativeOEXImportState) async throws -> NativeOEXImportState = { value in var next = value; next.revision += 1; saved = next; return next }
        do { _ = try await NativeOEXImportTransfer.run(state: saved, file: file, transport: transport, persist: persist, isCurrent: { true }); XCTFail("first response must be lost") } catch {}
        let originalRequest = try XCTUnwrap(saved.intent?.requestId)
        let result = try await NativeOEXImportTransfer.run(state: saved, file: file, transport: transport, persist: persist, isCurrent: { true })
        XCTAssertEqual(result.1.state, "queued"); XCTAssertNotNil(result.0.jobId)
        let ids = await transport.observedIDs(); XCTAssertEqual(ids, [originalRequest, originalRequest])
        let recovered = try await NativeOEXImportTransfer.run(state: result.0, file: nil, transport: transport, persist: persist, isCurrent: { true })
        XCTAssertEqual(recovered.1.state, "queued")
        let afterRecovery = await transport.observedIDs(); XCTAssertEqual(afterRecovery, ids, "a durable job must be observed, not recreated")
        let changed = try NativeOEXArchive(data: Data([0x50, 0x4b, 0x03, 0x04] + Array(repeating: 1, count: 20)))
        do { _ = try await NativeOEXImportTransfer.run(state: saved, file: NativeOEXImportFile(name: "different.oex", archive: changed), transport: transport, persist: persist, isCurrent: { true }); XCTFail("changed file must be refused") } catch {}
    }
}
