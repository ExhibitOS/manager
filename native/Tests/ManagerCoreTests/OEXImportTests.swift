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

final class OEXImportTests: XCTestCase {
    func testSnapshotRequiresLocalOEXRegularFileAndPreservesBytes() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("sample.oex")
        let bytes = Data([0x50, 0x4b, 0x03, 0x04] + Array(repeating: 0, count: 20))
        try bytes.write(to: file)
        let snapshot = try await NativeOEXImportFile.load(file)
        XCTAssertEqual(snapshot.name, "sample.oex")
        XCTAssertEqual(snapshot.archive.data, bytes)
        let link = root.appendingPathComponent("link.oex")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: file)
        do { _ = try await NativeOEXImportFile.load(link); XCTFail("final symlink must be refused") } catch {}
        do { _ = try await NativeOEXImportFile.load(root.appendingPathComponent("sample.png")); XCTFail("other extension must be refused") } catch {}
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
