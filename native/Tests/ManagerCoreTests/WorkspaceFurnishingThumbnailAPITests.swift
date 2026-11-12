// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class WorkspaceFurnishingThumbnailAPITests: XCTestCase {
    private let manifest = "1a0333bcb3f880878348911c6f7e3dfde81044506b3b887609740e4dc06c1738"
    private let png = "iVBORw0KGgoAAAANSUhEUgAAAQAAAAEACAYAAABccqhmAAABFUlEQVR4nO3BMQEAAADCoPVP7WsIoAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAeAMBPAABPO1TCQAAAABJRU5ErkJggg=="
    private struct Fixture: Sendable {
        let document: NativeDocumentState, furnishing: WorkspaceFurnishing, submission: WorkspaceFurnishingThumbnailSubmission
        let descriptor: Data, candidate: DocumentValue, bytes: Data
        var id: String { furnishing.id }
        func selection(_ version: Int64, selected: Bool) -> DocumentValue {
            .object(["source": candidate["source"], "selectionVersion": .integer(version), "selected": selected ? .object([
                "candidateId": candidate["candidateId"], "receiptSha256": candidate["receiptSha256"], "png": candidate["thumbnail"]]) : .null])
        }
    }
    private func value<T: Encodable>(_ input: T) throws -> DocumentValue {
        try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(input))
    }
    private func fixture() throws -> Fixture {
        let draft = try NativeExhibitionDraft.new()
        let doc = NativeDocumentState(format: 1, revision: 4, workspaceId: UUID().uuidString.lowercased(), tenantId: UUID().uuidString.lowercased(),
            principalId: UUID().uuidString.lowercased(), draft: draft, etag: "synthetic-etag", savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        let descriptor: DocumentValue = .object(["id": .string(UUID().uuidString.lowercased()), "version": .integer(1), "kind": .string("bench"),
            "name": .string("Synthetic"), "provenance": .string("Synthetic fixture"), "rights": try value(ArtworkMetadata.Rights()),
            "model": .object(["mime": .string("model/gltf-binary"), "bytes": .integer(128), "sha256": .string(String(repeating: "a", count: 64))])])
        let encoded = try JSONEncoder().encode(descriptor), item = try JSONDecoder().decode(WorkspaceFurnishing.self, from: encoded)
        let frame = ViewportFrame(epoch: UUID().uuidString.lowercased(), sequence: 1, tenantId: doc.tenantId, principalId: doc.principalId,
            draft: draft, selection: nil, mode: "translate", view: "isometric", disabled: false, workspaceId: doc.workspaceId, baseETag: doc.etag)
        let render = try value(ViewportFurnishingThumbnailRequest(frame: frame, furnishing: item)), bytes = try XCTUnwrap(Data(base64Encoded: png))
        let thumbnail: DocumentValue = .object(["mime": .string("image/png"), "width": .integer(256), "height": .integer(256),
            "bytes": .integer(Int64(bytes.count)), "sha256": .string(nativeImportSHA256(bytes))])
        var capturePNG = thumbnail; capturePNG["base64"] = .string(png)
        let capture: DocumentValue = .object(["workspaceId": .string(doc.workspaceId), "nativeRevision": .integer(4), "editVersion": draft.document["editVersion"],
            "baseETag": .string("synthetic-etag"), "request": render, "png": capturePNG])
        let submission = try WorkspaceFurnishingThumbnailSubmission(capture: capture, document: doc, furnishing: item)
        let source: DocumentValue = .object(["furnishingId": .string(item.id), "furnishingVersion": .integer(1), "modelBytes": .integer(128),
            "modelSha256": .string(item.details.model.sha256), "descriptorSha256": .string(String(repeating: "b", count: 64)), "rightsSha256": .string(String(repeating: "c", count: 64))])
        let id = "58830074-8ca6-8af4-a506-f6976ad8ebb8"
        let receipt: DocumentValue = .object(["version": .integer(1), "purpose": .string("registered-furnishing-thumbnail"), "profile": .string(WorkspaceFurnishingThumbnailSubmission.kind),
            "workspaceId": .string(doc.workspaceId), "candidateId": .string(id), "requestId": render["requestId"], "source": source, "renderRequest": render, "png": thumbnail])
        let candidate: DocumentValue = .object(["candidateId": .string(id), "state": .string("ready"), "source": source, "thumbnail": thumbnail,
            "receipt": receipt, "receiptSha256": .string(String(repeating: "d", count: 64))])
        return Fixture(document: doc, furnishing: item, submission: submission, descriptor: encoded, candidate: candidate, bytes: bytes)
    }
    private func plan(_ f: Fixture, marker: String? = WorkspaceFurnishingThumbnailSubmission.kind) throws -> ThumbnailAPIProtocol.Plan {
        var session: [String: String] = ["tenantId": f.document.tenantId, "userId": f.document.principalId, "role": "admin", "csrfToken": "synthetic-csrf", "expiresAt": "2099-01-01T00:00:00Z",
            "workspaceFurnishingProfile": "workspace-furnishing-intake-v1", "workspaceFurnishingStoolProfile": "workspace-furnishing-stool-v1", "workspaceFurnishingRoomVisualProfile": "workspace-furnishing-room-visual-v1"]
        session["workspaceFurnishingThumbnailProfile"] = marker
        let page: DocumentValue = .object(["source": f.candidate["source"], "items": .array([f.candidate]), "nextCursor": .null])
        return try ThumbnailAPIProtocol.Plan(session: JSONSerialization.data(withJSONObject: session), descriptors: [f.descriptor], candidate: JSONEncoder().encode(f.candidate),
            page: JSONEncoder().encode(page), selections: [JSONEncoder().encode(f.selection(0, selected: false))], recorded: JSONEncoder().encode(f.selection(1, selected: true)), image: f.bytes)
    }
    private func connect(_ f: Fixture) async throws -> AuthoringAPI {
        let api = try AuthoringAPI(port: 18091, tenantId: f.document.tenantId, nativeRuntimeManifestSHA256: manifest, protocolClasses: [ThumbnailAPIProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: f.document.tenantId, principalId: f.document.principalId,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        return api
    }
    private func requireNativeHeaders() {
        for request in ThumbnailAPIProtocol.requests().filter({ $0.path.contains("/thumbnail-") }) {
            XCTAssertEqual(request.origin, "http://127.0.0.1:18091", request.path)
            XCTAssertEqual(request.csrf, "synthetic-csrf", request.path)
        }
    }

    func testMarkerAndOwnerDenialPrecedeThumbnailHTTP() async throws {
        let f = try fixture()
        for marker in [nil, "unknown"] as [String?] {
            ThumbnailAPIProtocol.configure(try plan(f, marker: marker))
            let api = try await connect(f)
            do { _ = try await api.workspaceFurnishingThumbnailSelection(f.id, version: 1, in: f.document); XCTFail("missing marker admitted") } catch { }
            XCTAssertEqual(ThumbnailAPIProtocol.requests().map(\.path), ["/api/v1/auth/session"])
        }
        ThumbnailAPIProtocol.configure(try plan(f)); let api = try await connect(f)
        var raw = try value(f.document); raw["principalId"] = .string(UUID().uuidString.lowercased())
        let other = try JSONDecoder().decode(NativeDocumentState.self, from: JSONEncoder().encode(raw))
        do { _ = try await api.prepareWorkspaceFurnishingThumbnail(f.submission, in: other); XCTFail("wrong owner admitted") } catch { }
        XCTAssertEqual(ThumbnailAPIProtocol.requests().count, 1)
    }

    func testSubmitHistoricalCaptureAndReadReadyCandidateWithoutSelecting() async throws {
        let f = try fixture(); ThumbnailAPIProtocol.configure(try plan(f)); let api = try await connect(f)
        var edited = f.document; edited.revision += 10; edited.etag = "unrelated-head"; edited.savedEditVersion = nil
        let created = try await api.prepareWorkspaceFurnishingThumbnail(f.submission, in: edited)
        let page = try await api.workspaceFurnishingThumbnailCandidates(f.id, version: 1, in: edited)
        XCTAssertEqual(page.items, [created]); XCTAssertNil(page.nextCursor)
        let read = try await api.workspaceFurnishingThumbnailCandidate(f.id, version: 1, candidateId: created.candidateId, in: edited)
        XCTAssertEqual(read, created)
        let selected = try await api.selectedWorkspaceFurnishingThumbnail(f.id, version: 1, in: edited)
        XCTAssertNil(selected)
        let posts = ThumbnailAPIProtocol.requests().filter { $0.method == "POST" }
        XCTAssertEqual(posts.count, 1); XCTAssertEqual(posts.first?.body, f.submission.body)
        XCTAssertFalse(ThumbnailAPIProtocol.requests().contains { $0.path.hasSuffix("/image") })
        requireNativeHeaders()
    }

    func testHistoricalSelectionPOSTRetainsCallerCASAndReturnsLatestGETSeparately() async throws {
        let f = try fixture(); var p = try plan(f)
        p.selections = [try JSONEncoder().encode(f.selection(9, selected: false))]
        ThumbnailAPIProtocol.configure(p); let api = try await connect(f)
        let candidate = try WorkspaceFurnishingThumbnailCandidate(value: f.candidate, document: f.document, furnishing: f.furnishing)
        let requestId = UUID().uuidString.lowercased()
        let result = try await api.selectWorkspaceFurnishingThumbnail(candidate, expectedSelectionVersion: 0, requestId: requestId, in: f.document)
        XCTAssertEqual(result.recorded.selectionVersion, 1); XCTAssertTrue(result.recorded.matches(candidate))
        XCTAssertEqual(result.current.selectionVersion, 9); XCTAssertNil(result.current.selected)
        let calls = ThumbnailAPIProtocol.requests().filter { $0.path.hasSuffix("/thumbnail-selection") }
        XCTAssertEqual(calls.map(\.method), ["POST", "GET"])
        let posted = try JSONDecoder().decode(DocumentValue.self, from: XCTUnwrap(calls.first?.body))
        XCTAssertEqual(posted, .object(["requestId": .string(requestId), "candidateId": .string(candidate.candidateId),
            "receiptSha256": .string(candidate.receiptSha256), "expectedSelectionVersion": .integer(0)]))
        requireNativeHeaders()
    }

    func testSelectionAcknowledgementMustMatchRequestedCASVersion() async throws {
        let f = try fixture(); var p = try plan(f)
        p.recorded = try JSONEncoder().encode(f.selection(2, selected: true))
        ThumbnailAPIProtocol.configure(p); let api = try await connect(f)
        let candidate = try WorkspaceFurnishingThumbnailCandidate(value: f.candidate, document: f.document, furnishing: f.furnishing)
        do {
            _ = try await api.selectWorkspaceFurnishingThumbnail(candidate, expectedSelectionVersion: 0,
                requestId: UUID().uuidString.lowercased(), in: f.document)
            XCTFail("Unrelated recorded selection version acknowledged")
        } catch let error as AuthoringFailure { XCTAssertEqual(error.code, "CATALOG_PREVIEW_INVALID") }
        let calls = ThumbnailAPIProtocol.requests().filter { $0.path.hasSuffix("/thumbnail-selection") }
        XCTAssertEqual(calls.map(\.method), ["POST"])
        ThumbnailAPIProtocol.configure(try plan(f)); let other = try await connect(f)
        do {
            _ = try await other.selectWorkspaceFurnishingThumbnail(candidate, expectedSelectionVersion: 2_147_483_647,
                requestId: UUID().uuidString.lowercased(), in: f.document)
            XCTFail("Out-of-contract selection version admitted")
        } catch { }
        XCTAssertEqual(ThumbnailAPIProtocol.requests().count, 1)
    }

    func testOnlyDefinitivePOSTRejectionIsDistinguishedFromPostCommitReadbackFailure() async throws {
        let f = try fixture()
        let candidate = try WorkspaceFurnishingThumbnailCandidate(value: f.candidate, document: f.document, furnishing: f.furnishing)
        let failure = Data("{\"code\":\"FURNISHING_THUMBNAIL_CONFLICT\"}".utf8)
        var p = try plan(f); p.selectionPostStatus = 409; p.recorded = failure
        ThumbnailAPIProtocol.configure(p); let api = try await connect(f); let nonce = UUID().uuidString.lowercased()
        do {
            _ = try await api.selectWorkspaceFurnishingThumbnail(candidate, expectedSelectionVersion: 0, requestId: nonce, in: f.document)
            XCTFail("Rejected POST acknowledged")
        } catch let rejected as WorkspaceFurnishingThumbnailSelectionRejection {
            XCTAssertEqual(rejected.requestId, nonce); XCTAssertEqual(rejected.failure.status, 409)
        }
        p = try plan(f); p.selectionGetStatus = 409; p.selections = [failure]
        ThumbnailAPIProtocol.configure(p); let other = try await connect(f)
        do {
            _ = try await other.selectWorkspaceFurnishingThumbnail(candidate, expectedSelectionVersion: 0, requestId: nonce, in: f.document)
            XCTFail("Post-commit readback failure accepted")
        } catch is WorkspaceFurnishingThumbnailSelectionRejection { XCTFail("A later GET cannot prove POST did not commit") }
        catch let error as AuthoringFailure { XCTAssertEqual(error.status, 409) }
        XCTAssertEqual(ThumbnailAPIProtocol.requests().filter { $0.path.hasSuffix("/thumbnail-selection") }.map(\.method), ["POST", "GET"])
    }

    func testReadyCandidateImageReviewDoesNotSelect() async throws {
        let f = try fixture(); ThumbnailAPIProtocol.configure(try plan(f)); let api = try await connect(f)
        let candidate = try WorkspaceFurnishingThumbnailCandidate(value: f.candidate, document: f.document, furnishing: f.furnishing)
        let data = try await api.workspaceFurnishingThumbnailImage(candidate, in: f.document)
        XCTAssertEqual(data, f.bytes)
        XCTAssertFalse(ThumbnailAPIProtocol.requests().contains { $0.method == "POST" || $0.path.hasSuffix("/thumbnail-selection") })
        requireNativeHeaders()
    }

    func testSelectedImageUsesNativeHeadersWithoutDigestHeaderAndRejectsSelectionRace() async throws {
        let f = try fixture(); var p = try plan(f)
        p.selections = [try JSONEncoder().encode(f.selection(1, selected: true))]
        ThumbnailAPIProtocol.configure(p); var api = try await connect(f)
        let fetched = try await api.selectedWorkspaceFurnishingThumbnail(f.id, version: 1, in: f.document)
        let image = try XCTUnwrap(fetched)
        XCTAssertEqual(image.data, f.bytes); XCTAssertTrue(image.selection.matches(image.candidate)); requireNativeHeaders()
        p.selections.append(try JSONEncoder().encode(f.selection(2, selected: false)))
        ThumbnailAPIProtocol.configure(p); api = try await connect(f)
        do { _ = try await api.selectedWorkspaceFurnishingThumbnail(f.id, version: 1, in: f.document); XCTFail("stale card returned") }
        catch { XCTAssertEqual((error as? AuthoringFailure)?.code, "FURNISHING_THUMBNAIL_CONFLICT") }
    }

    func testStrictPageCandidateBindingsAndFreshDescriptorRights() async throws {
        let f = try fixture(); var p = try plan(f)
        let duplicate: DocumentValue = .object(["source": f.candidate["source"], "items": .array([f.candidate, f.candidate]), "nextCursor": .null])
        p.page = try JSONEncoder().encode(duplicate)
        ThumbnailAPIProtocol.configure(p); var api = try await connect(f)
        do { _ = try await api.workspaceFurnishingThumbnailCandidates(f.id, version: 1, in: f.document); XCTFail("duplicate IDs admitted") } catch { }
        var wrong = f.candidate; wrong["receipt"]["renderRequest"]["principalId"] = .string(UUID().uuidString.lowercased())
        p = try plan(f); p.candidate = try JSONEncoder().encode(wrong)
        ThumbnailAPIProtocol.configure(p); api = try await connect(f)
        do { _ = try await api.workspaceFurnishingThumbnailCandidate(f.id, version: 1, candidateId: f.candidate["candidateId"].string!, in: f.document); XCTFail("wrong actor admitted") } catch { }
        p = try plan(f)
        var denied = try value(f.furnishing); denied["rights"]["permissions"]["display"] = .bool(false)
        p.descriptors.append(try JSONEncoder().encode(denied))
        ThumbnailAPIProtocol.configure(p); api = try await connect(f)
        do { _ = try await api.workspaceFurnishingThumbnailSelection(f.id, version: 1, in: f.document); XCTFail("revoked rights admitted") } catch { }
    }

    func testInvalidPNGEvenWithMatchingIssuedHashAndBinary403AreRejected() async throws {
        let f = try fixture(); var p = try plan(f)
        var broken = f.bytes; broken[broken.count - 1] ^= 1
        var candidate = f.candidate, selected = f.selection(1, selected: true)
        candidate["thumbnail"]["sha256"] = .string(nativeImportSHA256(broken))
        candidate["receipt"]["png"] = candidate["thumbnail"]; selected["selected"]["png"] = candidate["thumbnail"]
        p.image = broken; p.candidate = try JSONEncoder().encode(candidate); p.selections = [try JSONEncoder().encode(selected)]
        ThumbnailAPIProtocol.configure(p); var api = try await connect(f)
        do { _ = try await api.selectedWorkspaceFurnishingThumbnail(f.id, version: 1, in: f.document); XCTFail("invalid PNG admitted") } catch { }
        p = try plan(f); p.imageStatus = 403; p.selections = [try JSONEncoder().encode(f.selection(1, selected: true))]
        ThumbnailAPIProtocol.configure(p); api = try await connect(f)
        do { _ = try await api.selectedWorkspaceFurnishingThumbnail(f.id, version: 1, in: f.document); XCTFail("binary denial admitted") }
        catch { XCTAssertEqual((error as? AuthoringFailure)?.code, "DISPLAY_PERMISSION_REQUIRED") }
    }

    func testSessionGenerationChangeDuringReadCannotReturnOldCandidate() async throws {
        let f = try fixture(); var p = try plan(f); p.holdCandidate = true
        ThumbnailAPIProtocol.configure(p); let api = try await connect(f)
        let operation = Task { try await api.workspaceFurnishingThumbnailCandidate(f.id, version: 1, candidateId: f.candidate["candidateId"].string!, in: f.document) }
        for _ in 0..<10_000 { if ThumbnailAPIProtocol.hasHeldResponse() { break }; await Task.yield() }
        guard ThumbnailAPIProtocol.hasHeldResponse() else { operation.cancel(); return XCTFail("Mock request did not reach barrier") }
        await api.endLocalAuthoringSession()
        ThumbnailAPIProtocol.release()
        do { _ = try await operation.value; XCTFail("old generation admitted") }
        catch { XCTAssertEqual((error as? AuthoringFailure)?.code, "ACCOUNT_CHANGED") }
    }
}

private final class ThumbnailAPIProtocol: URLProtocol, @unchecked Sendable {
    struct Plan: Sendable {
        var session: Data, descriptors: [Data], candidate: Data, page: Data, selections: [Data], recorded: Data, image: Data
        var imageStatus = 200
        var selectionPostStatus = 200, selectionGetStatus = 200
        var holdCandidate = false
    }
    struct Observation: Sendable {
        let method: String, path: String
        let origin: String?, csrf: String?, body: Data?
    }
    private static let lock = NSLock()
    nonisolated(unsafe) private static var plan: Plan?
    nonisolated(unsafe) private static var observed: [Observation] = []
    nonisolated(unsafe) private static var descriptorIndex = 0, selectionIndex = 0
    nonisolated(unsafe) private static var held: (ThumbnailAPIProtocol, Int, String, Data)?
    static func configure(_ value: Plan) {
        lock.lock(); defer { lock.unlock() }; plan = value; observed = []; descriptorIndex = 0; selectionIndex = 0; held = nil
    }
    static func requests() -> [Observation] { lock.lock(); defer { lock.unlock() }; return observed }
    static func hasHeldResponse() -> Bool { lock.lock(); defer { lock.unlock() }; return held != nil }
    static func release() {
        lock.lock(); let value = held; held = nil; lock.unlock()
        if let (host, status, mime, data) = value { host.finish(status, mime: mime, data: data) }
    }
    private func response() -> (Int, String, Data)? {
        Self.lock.lock(); defer { Self.lock.unlock() }
        guard let p = Self.plan else { return nil }
        let path = request.url!.path, method = request.httpMethod ?? "GET"
        var body = request.httpBody
        if body == nil, let stream = request.httpBodyStream {
            stream.open(); defer { stream.close() }; var bytes = Data(), buffer = [UInt8](repeating: 0, count: 4096)
            while true { let n = stream.read(&buffer, maxLength: buffer.count); if n <= 0 { break }; bytes.append(contentsOf: buffer.prefix(n)) }; body = bytes
        }
        Self.observed.append(Observation(method: method, path: path, origin: request.value(forHTTPHeaderField: "Origin"), csrf: request.value(forHTTPHeaderField: "x-csrf-token"), body: body))
        if path == "/api/v1/auth/session" { return (200, "application/json", p.session) }
        if path.hasSuffix("/versions/1") {
            let result = p.descriptors[min(Self.descriptorIndex, p.descriptors.count - 1)]; Self.descriptorIndex += 1
            return (200, "application/json", result)
        }
        if path.hasSuffix("/thumbnail-selection") {
            if method == "POST" { return (p.selectionPostStatus, "application/json", p.recorded) }
            let result = p.selections[min(Self.selectionIndex, p.selections.count - 1)]; Self.selectionIndex += 1
            return (p.selectionGetStatus, "application/json", result)
        }
        if path.hasSuffix("/image") { return (p.imageStatus, "image/png", p.image) }
        if path.hasSuffix("/thumbnail-candidates") { return (200, "application/json", method == "POST" ? p.candidate : p.page) }
        if path.contains("/thumbnail-candidates/") {
            if p.holdCandidate { Self.held = (self, 200, "application/json", p.candidate); return nil }
            return (200, "application/json", p.candidate)
        }
        return (404, "application/json", Data("{\"code\":\"NOT_FOUND\"}".utf8))
    }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() { if let (status, mime, data) = response() { finish(status, mime: mime, data: data) } }
    private func finish(_ status: Int, mime: String, data: Data) {
        let response = HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: "HTTP/1.1", headerFields: ["Content-Type": mime, "Content-Length": String(data.count)])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
