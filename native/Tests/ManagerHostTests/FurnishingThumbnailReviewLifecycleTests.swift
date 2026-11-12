// SPDX-License-Identifier: Apache-2.0
import XCTest
import Foundation
@testable import ManagerCore
@testable import ManagerApp

@MainActor
final class FurnishingThumbnailReviewLifecycleTests: XCTestCase {
    private func fixture() throws -> (NativeDocumentState, WorkspaceFurnishing, WorkspaceFurnishingThumbnailSubmission) {
        let draft = try NativeExhibitionDraft.new()
        let owner = NativeDocumentState(format: 1, revision: 4, workspaceId: UUID().uuidString.lowercased(),
            tenantId: UUID().uuidString.lowercased(), principalId: UUID().uuidString.lowercased(), draft: draft,
            etag: "synthetic-etag", savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        let encoder = JSONEncoder()
        let rights = try JSONDecoder().decode(DocumentValue.self, from: encoder.encode(ArtworkMetadata.Rights()))
        let descriptor: DocumentValue = .object(["id": .string(UUID().uuidString.lowercased()), "version": .integer(1),
            "kind": .string("bench"), "name": .string("Synthetic"), "provenance": .string("Synthetic fixture"), "rights": rights,
            "model": .object(["mime": .string("model/gltf-binary"), "bytes": .integer(128), "sha256": .string(String(repeating: "a", count: 64))])])
        let item = try JSONDecoder().decode(WorkspaceFurnishing.self, from: encoder.encode(descriptor))
        let frame = ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 1, tenantId: owner.tenantId,
            principalId: owner.principalId, draft: draft, selection: nil, mode: "translate", view: "isometric", disabled: false,
            workspaceId: owner.workspaceId, baseETag: owner.etag)
        let request = try JSONDecoder().decode(DocumentValue.self, from: encoder.encode(ViewportFurnishingThumbnailRequest(frame: frame, furnishing: item)))
        let encoded = "iVBORw0KGgoAAAANSUhEUgAAAQAAAAEACAYAAABccqhmAAABFUlEQVR4nO3BMQEAAADCoPVP7WsIoAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAeAMBPAABPO1TCQAAAABJRU5ErkJggg=="
        let bytes = try XCTUnwrap(Data(base64Encoded: encoded))
        let capture: DocumentValue = .object(["workspaceId": .string(owner.workspaceId), "nativeRevision": .integer(4),
            "editVersion": draft.document["editVersion"], "baseETag": .string("synthetic-etag"), "request": request,
            "png": .object(["mime": .string("image/png"), "width": .integer(256), "height": .integer(256),
                "bytes": .integer(Int64(bytes.count)), "sha256": .string(NativeAutomationSession.digest(bytes)), "base64": .string(encoded)])])
        return (owner, item, try WorkspaceFurnishingThumbnailSubmission(capture: capture, document: owner, furnishing: item))
    }
    private func api(_ owner: NativeDocumentState) throws -> AuthoringAPI {
        // Deliberately unconnected: this exercises retained intent on refusal
        // without a live server, profile, renderer or HTTP request.
        try AuthoringAPI(port: 18091, tenantId: owner.tenantId)
    }
    func testSheetReopenRetainsOriginalCaptureAndRebindDoesNotRenderAgain() async throws {
        let (owner, item, submission) = try fixture(); var captures = 0
        let journal = try MemoryJournal(owner)
        let model = FurnishingThumbnailReviewModel(name: item.details.name, furnishingId: item.id, owner: owner, api: try api(owner),
            document: { owner }, capture: { captures += 1; return submission }, refreshed: {},
            readCheckpoint: { try journal.read() }, writeCheckpoint: { try journal.write($0) })
        let sessions = FurnishingThumbnailReviewSessions(), key = FurnishingThumbnailReviewSessions.Key(owner: owner, furnishingId: item.id)
        XCTAssertTrue(sessions.retain(model, for: key))
        await model.prepare(); XCTAssertTrue(model.hasUnresolvedIntent); XCTAssertEqual(captures, 1)
        model.closeDisplay()
        let reopened = try XCTUnwrap(sessions.existing(key)); XCTAssertTrue(reopened === model)
        var newer = owner; newer.revision += 1
        XCTAssertTrue(reopened.rebind(name: item.details.name, api: try api(owner), document: { newer },
            capture: { captures += 1; return submission }, refreshed: {},
            readCheckpoint: { try journal.read() }, writeCheckpoint: { try journal.write($0) }))
        await reopened.prepare()
        XCTAssertEqual(captures, 1); XCTAssertTrue(reopened.hasUnresolvedIntent)
    }
    func testOtherOwnerCannotBorrowOrReplaceRetainedIntent() async throws {
        let (owner, item, submission) = try fixture()
        let journal = try MemoryJournal(owner)
        let model = FurnishingThumbnailReviewModel(name: item.details.name, furnishingId: item.id, owner: owner, api: try api(owner),
            document: { owner }, capture: { submission }, refreshed: {},
            readCheckpoint: { try journal.read() }, writeCheckpoint: { try journal.write($0) })
        let sessions = FurnishingThumbnailReviewSessions(), key = FurnishingThumbnailReviewSessions.Key(owner: owner, furnishingId: item.id)
        XCTAssertTrue(sessions.retain(model, for: key)); await model.prepare()
        let other = NativeDocumentState(format: owner.format, revision: owner.revision, workspaceId: owner.workspaceId,
            tenantId: owner.tenantId, principalId: UUID().uuidString.lowercased(), draft: owner.draft,
            etag: owner.etag, savedEditVersion: owner.savedEditVersion, pending: nil)
        XCTAssertNil(sessions.existing(.init(owner: other, furnishingId: item.id)))
        XCTAssertFalse(model.rebind(name: item.details.name, api: try api(other), document: { other }, capture: { submission }, refreshed: {},
            readCheckpoint: { try journal.read() }, writeCheckpoint: { try journal.write($0) }))
        let replacement = FurnishingThumbnailReviewModel(name: item.details.name, furnishingId: item.id, owner: owner, api: try api(owner),
            document: { owner }, capture: { submission }, refreshed: {},
            readCheckpoint: { try journal.read() }, writeCheckpoint: { try journal.write($0) })
        XCTAssertFalse(sessions.retain(replacement, for: key)); XCTAssertTrue(sessions.existing(key) === model)
        XCTAssertTrue(model.hasUnresolvedIntent)
    }

    /// Encoded bytes stand in for a new process's durable owner store, never a
    /// model-owned object. Writes enforce a separate journal revision CAS.
    private final class MemoryJournal {
        private var bytes: Data
        var writeAttempts = 0
        var writes = 0
        var failWrite = false
        init(_ owner: NativeDocumentState) throws { bytes = try JSONEncoder().encode(NativeFurnishingThumbnailState(document: owner)) }
        func read() throws -> NativeFurnishingThumbnailState { try JSONDecoder().decode(NativeFurnishingThumbnailState.self, from: bytes) }
        func write(_ proposed: NativeFurnishingThumbnailState) throws -> NativeFurnishingThumbnailState {
            writeAttempts += 1
            if failWrite { throw AuthoringFailure("SYNTHETIC_JOURNAL_WRITE_FAILED") }
            let before = try read()
            guard proposed.revision == before.revision,
                  [proposed.workspaceId, proposed.tenantId, proposed.principalId] == [before.workspaceId, before.tenantId, before.principalId] else {
                throw AuthoringFailure("NATIVE_FURNISHING_THUMBNAIL_STATE_CHANGED")
            }
            var raw = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(proposed))
            raw["revision"] = .integer(Int64(before.revision) + 1)
            let acknowledged = try JSONDecoder().decode(NativeFurnishingThumbnailState.self, from: JSONEncoder().encode(raw))
            bytes = try JSONEncoder().encode(acknowledged); writes += 1
            return acknowledged
        }
    }
    private func connectedAPI(_ owner: NativeDocumentState, item: WorkspaceFurnishing,
                              submission: WorkspaceFurnishingThumbnailSubmission) async throws -> AuthoringAPI {
        let source: DocumentValue = .object(["furnishingId": .string(item.id), "furnishingVersion": .integer(1),
            "modelSha256": .string(item.details.model.sha256), "modelBytes": .integer(Int64(item.details.model.bytes)),
            "descriptorSha256": .string(String(repeating: "b", count: 64)), "rightsSha256": .string(String(repeating: "c", count: 64))])
        let session = ["tenantId": owner.tenantId, "userId": owner.principalId, "role": "admin", "csrfToken": "synthetic-csrf",
            "expiresAt": "2099-01-01T00:00:00Z", "workspaceFurnishingProfile": "workspace-furnishing-intake-v1",
            "workspaceFurnishingStoolProfile": "workspace-furnishing-stool-v1", "workspaceFurnishingRoomVisualProfile": "workspace-furnishing-room-visual-v1",
            "workspaceFurnishingThumbnailProfile": "workspace-furnishing-thumbnail-v1"]
        LifecycleThumbnailProtocol.configure(session: try JSONSerialization.data(withJSONObject: session), descriptor: try JSONEncoder().encode(item),
            page: try JSONEncoder().encode(DocumentValue.object(["source": source, "items": .array([]), "nextCursor": .null])),
            selection: try JSONEncoder().encode(DocumentValue.object(["source": source, "selectionVersion": .integer(0), "selected": .null])))
        let api = try AuthoringAPI(port: 18091, tenantId: owner.tenantId,
            nativeRuntimeManifestSHA256: "2c29a8ffe2a56395dd1cdc1b0acd93e4a1623e8aac8b0e10aca30369e60b4626", protocolClasses: [LifecycleThumbnailProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: owner.tenantId, principalId: owner.principalId,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        return api
    }
    func testNewModelRestoresPersistedCaptureAndReplaysOriginalNonceWithoutRendering() async throws {
        let (owner, item, submission) = try fixture(); let journal = try MemoryJournal(owner)
        var initialRenders = 0, restoredRenders = 0
        let originalAPI = try await connectedAPI(owner, item: item, submission: submission)
        var first: FurnishingThumbnailReviewModel? = FurnishingThumbnailReviewModel(name: item.details.name, furnishingId: item.id,
            owner: owner, api: originalAPI, document: { owner }, capture: { initialRenders += 1; return submission }, refreshed: {},
            readCheckpoint: { try journal.read() }, writeCheckpoint: { try journal.write($0) })
        await first?.prepare()
        let originalPending = try XCTUnwrap(journal.read().pending)
        XCTAssertEqual(originalPending.requestId, submission.request.requestId)
        XCTAssertEqual(originalPending.value["capture"], submission.captureMetadata)
        XCTAssertEqual(journal.writes, 1); XCTAssertEqual(initialRenders, 1)
        let firstPost = try XCTUnwrap(LifecycleThumbnailProtocol.requests().first(where: { $0.method == "POST" })?.body)
        XCTAssertEqual(try JSONDecoder().decode(DocumentValue.self, from: firstPost), submission.value)
        weak var released = first; first = nil
        XCTAssertNil(released, "The restored model must not inherit the original in-memory intent")
        var newer = owner; newer.revision += 1
        let restoredAPI = try await connectedAPI(owner, item: item, submission: submission)
        let restored = FurnishingThumbnailReviewModel(name: item.details.name, furnishingId: item.id,
            owner: newer, api: restoredAPI, document: { newer }, capture: { restoredRenders += 1; return submission }, refreshed: {},
            readCheckpoint: { try journal.read() }, writeCheckpoint: { try journal.write($0) })
        await restored.load()
        XCTAssertTrue(restored.hasUnresolvedIntent)
        await restored.prepare()
        let replay = try XCTUnwrap(LifecycleThumbnailProtocol.requests().first(where: { $0.method == "POST" })?.body)
        XCTAssertEqual(replay, firstPost)
        XCTAssertEqual(try journal.read().pending, originalPending)
        XCTAssertEqual(journal.writes, 1, "Existing durable intent is reused without another journal mutation")
        XCTAssertEqual(initialRenders, 1); XCTAssertEqual(restoredRenders, 0)
        XCTAssertTrue(restored.hasUnresolvedIntent, "503 retains the exact intent; it is not a known failure receipt")
    }
    func testJournalPersistenceFailurePreventsHTTPDispatch() async throws {
        let (owner, item, submission) = try fixture(); let journal = try MemoryJournal(owner); journal.failWrite = true
        let api = try await connectedAPI(owner, item: item, submission: submission)
        let before = LifecycleThumbnailProtocol.requests().count; var renders = 0
        let model = FurnishingThumbnailReviewModel(name: item.details.name, furnishingId: item.id, owner: owner, api: api,
            document: { owner }, capture: { renders += 1; return submission }, refreshed: {},
            readCheckpoint: { try journal.read() }, writeCheckpoint: { try journal.write($0) })
        await model.prepare()
        XCTAssertEqual(journal.writeAttempts, 1); XCTAssertEqual(journal.writes, 0)
        XCTAssertNil(try journal.read().pending); XCTAssertEqual(try journal.read().revision, 0)
        XCTAssertEqual(LifecycleThumbnailProtocol.requests().count, before)
        XCTAssertEqual(renders, 1); XCTAssertTrue(model.hasUnresolvedIntent)
    }

}


private final class LifecycleThumbnailProtocol: URLProtocol, @unchecked Sendable {
    struct Observation: Sendable { let method: String; let body: Data? }
    private struct Plan { let session: Data, descriptor: Data, page: Data, selection: Data }
    private static let lock = NSLock()
    nonisolated(unsafe) private static var plan: Plan?
    nonisolated(unsafe) private static var observed: [Observation] = []
    static func configure(session: Data, descriptor: Data, page: Data, selection: Data) {
        lock.lock(); defer { lock.unlock() }
        plan = Plan(session: session, descriptor: descriptor, page: page, selection: selection); observed = []
    }
    static func requests() -> [Observation] { lock.lock(); defer { lock.unlock() }; return observed }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        var body = request.httpBody
        if body == nil, let stream = request.httpBodyStream {
            stream.open(); defer { stream.close() }; var bytes = Data(), buffer = [UInt8](repeating: 0, count: 4096)
            while true { let n = stream.read(&buffer, maxLength: buffer.count); if n <= 0 { break }; bytes.append(contentsOf: buffer.prefix(n)) }
            body = bytes
        }
        Self.lock.lock()
        let plan = Self.plan; Self.observed.append(.init(method: request.httpMethod ?? "GET", body: body))
        Self.lock.unlock()
        guard let plan, let url = request.url else { return }
        let path = url.path; let status: Int, data: Data
        if path == "/api/v1/auth/session" { status = 200; data = plan.session }
        else if path.hasSuffix("/versions/1") { status = 200; data = plan.descriptor }
        else if path.hasSuffix("/thumbnail-candidates"), request.httpMethod == "POST" {
            status = 503; data = Data("{\"code\":\"FURNISHING_THUMBNAIL_UNAVAILABLE\"}".utf8)
        } else if path.hasSuffix("/thumbnail-candidates") { status = 200; data = plan.page }
        else if path.hasSuffix("/thumbnail-selection") { status = 200; data = plan.selection }
        else { status = 404; data = Data("{\"code\":\"NOT_FOUND\"}".utf8) }
        let response = HTTPURLResponse(url: url, statusCode: status, httpVersion: "HTTP/1.1", headerFields: ["Content-Type": "application/json", "Content-Length": String(data.count)])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
