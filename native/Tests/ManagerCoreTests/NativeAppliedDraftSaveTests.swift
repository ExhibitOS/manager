// SPDX-License-Identifier: Apache-2.0
import Foundation
import CryptoKit
import XCTest
@testable import ManagerCore

@MainActor
final class NativeAppliedDraftSaveTests: XCTestCase {
    private let requestId = "44444444-4444-4444-8444-444444444444"
    private let manifest = String(repeating: "c", count: 64)

    private func fixture(originalBody: Bool = false) throws -> (NativeDocumentState, NativeDocumentState, DocumentValue) {
        let before = try NativeExhibitionDraft.new(title: "before")
        var after = before, candidate = before.candidate
        candidate["title"] = .string("selected applied change")
        try after.replaceCandidate(candidate)
        let source = NativeDocumentState(format: 1, revision: 40,
            workspaceId: "11111111-1111-4111-8111-111111111111", tenantId: "22222222-2222-4222-8222-222222222222",
            principalId: "33333333-3333-4333-8333-333333333333", draft: before,
            etag: "\"studio-r12-" + String(repeating: "a", count: 64) + "\"", savedEditVersion: 1, pending: nil)
        var pending = try NativeAppliedDraftSave.stage(source, draft: after, requestId: requestId)
        pending.revision += 1 // the existing helper performs this single CAS increment
        var anchor: DocumentValue = .object(["version": .integer(1), "requestId": .string(requestId),
            "workspaceId": .string(source.workspaceId), "tenantId": .string(source.tenantId), "principalId": .string(source.principalId),
            "exhibitionId": .string(before.id), "runtimeManifestSHA256": .string(manifest), "etag": .string(source.etag!),
            "sourceNativeRevision": .integer(40), "sourceEditVersion": .integer(1), "candidate": after.candidate])
        if originalBody { anchor["originalDraft"] = after.document }
        return (source, pending, anchor)
    }

    private func connect(_ source: NativeDocumentState, pending: NativeDocumentState,
                         dropCommitReply: Bool = false) async throws -> AuthoringAPI {
        AppliedSaveProtocol.configure(source, after: pending.draft!, requestId: requestId, dropCommitReply: dropCommitReply)
        let api = try AuthoringAPI(port: 18091, tenantId: source.tenantId, nativeRuntimeManifestSHA256: manifest,
            protocolClasses: [AppliedSaveProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: source.tenantId, principalId: source.principalId,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        return api
    }

    func testAppliedBeforeSaveUsesRetainedBodyAndOriginalIdentityWithoutReapplying() async throws {
        let (source, pending, input) = try fixture(), anchor = try NativeAppliedDraftSave.Anchor(input)
        let api = try await connect(source, pending: pending)
        XCTAssertEqual(try anchor.state(pending, manifest: manifest), .pending)
        let saved = try await NativeAppliedDraftSave.replayOriginal(anchor, current: pending, manifest: manifest, api: api)
        XCTAssertEqual(saved.draft, pending.pending?.draft)
        XCTAssertEqual(AppliedSaveProtocol.counts().commits, 1)
        XCTAssertEqual(AppliedSaveProtocol.counts().puts, 1)
        XCTAssertNotNil(pending.pending) // replay does not mutate or reapply the local draft
        let summary = try NativeDocumentInspection.summary(pending)
        XCTAssertEqual(summary["applyOperationAttribution"]["requestId"], .string(requestId))
        XCTAssertEqual(summary["liveRuntimeOwnership"], .string("unknown"))
    }

    func testCommittedSaveReplyLostRetriesOriginalReceiptAndCleanRetryDoesNotCommitAgain() async throws {
        let (source, pending, input) = try fixture(originalBody: true), anchor = try NativeAppliedDraftSave.Anchor(input)
        let api = try await connect(source, pending: pending, dropCommitReply: true)
        do { _ = try await NativeAppliedDraftSave.replayOriginal(anchor, current: pending, manifest: manifest, api: api); XCTFail("Injected lost commit reply") }
        catch { }
        XCTAssertEqual(AppliedSaveProtocol.counts().commits, 1)
        XCTAssertEqual(pending.pending?.requestId, requestId)
        let receipt = try await NativeAppliedDraftSave.replayOriginal(anchor, current: pending, manifest: manifest, api: api)
        var clean = pending
        clean.revision += 1; clean.pending = nil; clean.etag = receipt.etag; clean.savedEditVersion = 2
        XCTAssertEqual(try anchor.state(clean, manifest: manifest), .saved)
        let again = try await NativeAppliedDraftSave.replayOriginal(anchor, current: clean, manifest: manifest, api: api)
        XCTAssertEqual(again.etag, receipt.etag)
        XCTAssertEqual(clean.revision, 42)
        XCTAssertEqual(AppliedSaveProtocol.counts().commits, 1)
        XCTAssertEqual(AppliedSaveProtocol.counts().puts, 3)
        // A changed current If-Match would hit REQUEST_CONFLICT in this fake
        // receipt server. All three PUTs were required to preserve the old one.
    }

    func testUnrelatedOwnerRevisionDraftRuntimeAndHeadConflictsNeverPut() async throws {
        let (source, pending, input) = try fixture(), anchor = try NativeAppliedDraftSave.Anchor(input)
        let api = try await connect(source, pending: pending)
        var changed = pending; changed.revision += 1
        var unrelated = pending, draft = pending.draft!, candidate = draft.candidate
        candidate["title"] = .string("unrelated"); try draft.replaceCandidate(candidate); unrelated.draft = draft
        let foreign = NativeDocumentState(format: 1, revision: pending.revision,
            workspaceId: UUID().uuidString.lowercased(), tenantId: pending.tenantId, principalId: pending.principalId,
            draft: pending.draft, etag: pending.etag, savedEditVersion: pending.savedEditVersion, pending: pending.pending)
        for state in [changed, unrelated, foreign] {
            do { _ = try await NativeAppliedDraftSave.replayOriginal(anchor, current: state, manifest: manifest, api: api); XCTFail("Conflict must preserve intent") }
            catch { }
        }
        XCTAssertThrowsError(try anchor.state(pending, manifest: String(repeating: "d", count: 64)))
        AppliedSaveProtocol.replaceHeadWithUnrelatedEdit()
        do { _ = try await NativeAppliedDraftSave.replayOriginal(anchor, current: pending, manifest: manifest, api: api); XCTFail("Foreign server head") }
        catch { }
        XCTAssertEqual(AppliedSaveProtocol.counts().puts, 0)
        XCTAssertEqual(pending.pending?.requestId, requestId)
    }

    func testHistoricalDirtyAndCleanWithoutOriginalBodyAreNotAttributable() throws {
        let (_, pending, input) = try fixture(), anchor = try NativeAppliedDraftSave.Anchor(input)
        var unknown = pending; unknown.pending = nil
        XCTAssertThrowsError(try anchor.state(unknown, manifest: manifest))
        unknown.revision = 42; unknown.savedEditVersion = 2
        unknown.etag = "\"studio-r13-" + String(repeating: "b", count: 64) + "\""
        XCTAssertThrowsError(try anchor.state(unknown, manifest: manifest))
        XCTAssertEqual(try NativeDocumentInspection.summary(unknown)["applyOperationAttribution"], .string("unknown"))
        var malformed = input; malformed["unknown"] = .bool(true)
        XCTAssertThrowsError(try NativeAppliedDraftSave.Anchor(malformed))
        malformed = input; malformed["originalDraft"] = .null
        XCTAssertThrowsError(try NativeAppliedDraftSave.Anchor(malformed))
    }

    func testRetainedCallerIntentUsesExistingPrivatePinnedReader() throws {
        let (_, _, input) = try fixture()
        let root = FileManager.default.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: root) }
        let path = root.appendingPathComponent("intent.json"), bytes = try JSONEncoder().encode(input)
        try bytes.write(to: path)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: path.path)
        let hash = SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
        XCTAssertEqual(try NativeAutomationSession.sharedBenchReply(root: root, path: path.path, expectedSHA256: hash), input)
        XCTAssertThrowsError(try NativeAutomationSession.sharedBenchReply(root: root, path: path.path, expectedSHA256: String(repeating: "f", count: 64)))
    }

    func testSameSessionAnchorOnlyUsesExactRetainedOptInApplyResponse() throws {
        let (_, pending, _) = try fixture()
        let response = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(pending))
        let original: DocumentValue = .object(["operation": .string("fixture-apply"), "requestId": .string(requestId),
            "workspaceId": .string(pending.workspaceId), "tenantId": .string(pending.tenantId),
            "principalId": .string(pending.principalId), "expectedRevision": .integer(40),
            "expectedEditVersion": .integer(1), "retainSaveIntent": .bool(true)])
        let anchor = try NativeAppliedDraftSave.Anchor.sameSession(request: original, response: response, manifest: manifest)
        XCTAssertEqual(anchor.originalDraft, pending.draft)
        for (key, value) in [("operation", DocumentValue.string("fixture-prepare")),
                             ("retainSaveIntent", .bool(false)), ("workspaceId", .string(UUID().uuidString.lowercased()))] {
            var wrong = original; wrong[key] = value
            XCTAssertThrowsError(try NativeAppliedDraftSave.Anchor.sameSession(request: wrong, response: response, manifest: manifest))
        }
        var unretained = response; unretained["pending"] = .null
        XCTAssertThrowsError(try NativeAppliedDraftSave.Anchor.sameSession(request: original, response: unretained, manifest: manifest))
    }

    func testCleanReplayWithoutOriginalReceiptCannotCreateNewRevision() async throws {
        let (source, pending, input) = try fixture(originalBody: true), anchor = try NativeAppliedDraftSave.Anchor(input)
        let api = try await connect(source, pending: pending)
        AppliedSaveProtocol.markSavedWithoutReceipt()
        var clean = pending; clean.revision = 42; clean.pending = nil; clean.savedEditVersion = 2
        clean.etag = "\"studio-r13-" + String(repeating: "b", count: 64) + "\""
        do { _ = try await NativeAppliedDraftSave.replayOriginal(anchor, current: clean, manifest: manifest, api: api); XCTFail("Old If-Match must prevent a new Save") }
        catch { }
        XCTAssertEqual(AppliedSaveProtocol.counts().commits, 0)
        XCTAssertEqual(clean.revision, 42)
    }
}

private final class AppliedSaveProtocol: URLProtocol, @unchecked Sendable {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var source: NativeDocumentState?
    nonisolated(unsafe) private static var after: NativeExhibitionDraft?
    nonisolated(unsafe) private static var requestId = ""
    nonisolated(unsafe) private static var receipt: DocumentValue?
    nonisolated(unsafe) private static var head: DocumentValue = .null
    nonisolated(unsafe) private static var puts = 0, commits = 0
    nonisolated(unsafe) private static var dropCommitReply = false
    static func configure(_ state: NativeDocumentState, after: NativeExhibitionDraft, requestId: String, dropCommitReply: Bool) {
        lock.lock(); defer { lock.unlock() }
        source = state; self.after = after; self.requestId = requestId
        head = .object(["revision": .integer(12), "etag": .string(state.etag!), "draft": state.draft!.document])
        receipt = nil; puts = 0; commits = 0; self.dropCommitReply = dropCommitReply
    }
    static func counts() -> (puts: Int, commits: Int) { lock.lock(); defer { lock.unlock() }; return (puts, commits) }
    static func replaceHeadWithUnrelatedEdit() {
        lock.lock(); defer { lock.unlock() }
        head["etag"] = .string("\"studio-r14-" + String(repeating: "d", count: 64) + "\"")
        head["revision"] = .integer(14)
    }
    static func markSavedWithoutReceipt() {
        lock.lock(); defer { lock.unlock() }
        head = .object(["revision": .integer(13), "etag": .string("\"studio-r13-" + String(repeating: "b", count: 64) + "\""), "draft": after!.document])
    }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func stopLoading() {}
    override func startLoading() {
        do {
            let (value, status) = try Self.reply(request)
            var headers = ["Content-Type": "application/json", "Cache-Control": "no-store"]
            if let etag = value["etag"].string { headers["ETag"] = etag }
            let response = HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: "HTTP/1.1", headerFields: headers)!
            client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
            client?.urlProtocol(self, didLoad: try JSONEncoder().encode(value))
            client?.urlProtocolDidFinishLoading(self)
        } catch { client?.urlProtocol(self, didFailWithError: error) }
    }
    private static func reply(_ request: URLRequest) throws -> (DocumentValue, Int) {
        lock.lock(); defer { lock.unlock() }
        let source = try XCTUnwrap(source)
        if request.url?.path == "/api/v1/auth/session" {
            return (.object(["tenantId": .string(source.tenantId), "userId": .string(source.principalId),
                "role": .string("admin"), "csrfToken": .string("synthetic-csrf"), "expiresAt": .string("2099-01-01T00:00:00Z")]), 200)
        }
        if request.httpMethod != "PUT" { return (head, 200) }
        puts += 1
        var body = request.httpBody
        if body == nil, let stream = request.httpBodyStream {
            stream.open(); defer { stream.close() }
            var bytes = Data(), buffer = [UInt8](repeating: 0, count: 4096)
            while true { let n = stream.read(&buffer, maxLength: buffer.count); if n <= 0 { break }; bytes.append(contentsOf: buffer.prefix(n)) }
            body = bytes
        }
        let input = try JSONDecoder().decode(DocumentValue.self, from: XCTUnwrap(body))
        guard input["requestId"] == .string(requestId), input["draft"] == after!.document,
              request.value(forHTTPHeaderField: "If-Match") == source.etag else {
            return (.object(["error": .object(["code": .string("REQUEST_CONFLICT")])]), 409)
        }
        if let receipt { return (receipt, 200) }
        guard head["etag"] == .string(source.etag!) else {
            return (.object(["error": .object(["code": .string("REMOTE_CONFLICT")])]), 412)
        }
        commits += 1
        head = .object(["revision": .integer(13), "etag": .string("\"studio-r13-" + String(repeating: "b", count: 64) + "\""), "draft": after!.document])
        receipt = head
        if dropCommitReply { dropCommitReply = false; throw URLError(.networkConnectionLost) }
        return (head, 200)
    }
}
