// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class NativeCurrentImageThumbnailRequestTests: XCTestCase {
    func testQualifiedPrepareSendsOnlyRequestAndRevisionWithoutSelection() async throws {
        ThumbnailRequestProtocol.reset(stale: false)
        let (api, document) = try await connected()
        let job = try await api.prepareCurrentImageThumbnail(artworkId: ThumbnailRequestProtocol.id,
            revision: 2, requestId: UUID(uuidString: ThumbnailRequestProtocol.requestID)!, in: document)
        XCTAssertEqual(job.state, "queued")
        let writes = ThumbnailRequestProtocol.writes()
        XCTAssertEqual(writes.count, 1)
        XCTAssertEqual(writes.first?.0, "/api/v1/tenants/\(ThumbnailRequestProtocol.id)/cms/artworks/\(ThumbnailRequestProtocol.id)/image-thumbnail-preparations")
        let body = try XCTUnwrap(writes.first?.1)
        let fields = try XCTUnwrap(JSONSerialization.jsonObject(with: body) as? [String: Any])
        XCTAssertEqual(Set(fields.keys), ["requestId", "expectedRevision"])
        XCTAssertEqual(fields["requestId"] as? String, ThumbnailRequestProtocol.requestID)
        XCTAssertEqual(fields["expectedRevision"] as? Int, 2)
    }
    func testChangedCurrentApprovalStopsBeforePreparationPost() async throws {
        ThumbnailRequestProtocol.reset(stale: true)
        let (api, document) = try await connected()
        do {
            _ = try await api.prepareCurrentImageThumbnail(artworkId: ThumbnailRequestProtocol.id,
                revision: 2, requestId: UUID(), in: document)
            XCTFail("Stale current approval must not start a job")
        } catch let error as AuthoringFailure { XCTAssertEqual(error.code, "REVISION_CONFLICT") }
        XCTAssertTrue(ThumbnailRequestProtocol.writes().isEmpty)
    }
    private func connected() async throws -> (AuthoringAPI, NativeDocumentState) {
        let id = ThumbnailRequestProtocol.id
        let api = try AuthoringAPI(port: 18091, tenantId: id,
            nativeRuntimeManifestSHA256: "0f04520a8742f9774206d12c8a2b56da8ed2a4e07c095b3cb8ad3b3e988dd988",
            protocolClasses: [ThumbnailRequestProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: id, principalId: id,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        let draft = try NativeExhibitionDraft.new(title: "Synthetic thumbnail source")
        return (api, NativeDocumentState(format: 1, revision: 304, workspaceId: id,
            tenantId: id, principalId: id, draft: draft, etag: nil,
            savedEditVersion: draft.document["editVersion"].integer, pending: nil))
    }
}
private final class ThumbnailRequestProtocol: URLProtocol, @unchecked Sendable {
    static let id = "11111111-1111-4111-8111-111111111111"
    static let requestID = "22222222-2222-4222-8222-222222222222"
    static let sourceID = "33333333-3333-4333-8333-333333333333"
    static let jobID = "44444444-4444-4444-8444-444444444444"
    private static let lock = NSLock()
    nonisolated(unsafe) private static var stale = false
    nonisolated(unsafe) private static var observed: [(String, Data?)] = []
    static func reset(stale: Bool) { lock.lock(); defer { lock.unlock() }; Self.stale = stale; observed = [] }
    static func writes() -> [(String, Data?)] { lock.lock(); defer { lock.unlock() }; return observed }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        Self.lock.lock(); let stale = Self.stale
        if request.httpMethod == "POST" {
            var body = request.httpBody
            if body == nil, let stream = request.httpBodyStream {
                stream.open(); defer { stream.close() }
                var bytes = Data(), buffer = [UInt8](repeating: 0, count: 1024)
                while stream.hasBytesAvailable {
                    let count = stream.read(&buffer, maxLength: buffer.count)
                    if count <= 0 { break }; bytes.append(contentsOf: buffer.prefix(count))
                }
                body = bytes
            }
            Self.observed.append((request.url!.path, body))
        }
        Self.lock.unlock()
        let data: Data
        if request.url!.path == "/api/v1/auth/session" {
            data = try! JSONSerialization.data(withJSONObject: ["tenantId": Self.id, "userId": Self.id,
                "role": "admin", "csrfToken": "synthetic-csrf", "expiresAt": "2099-01-01T00:00:00Z"])
        } else if request.httpMethod == "POST" {
            let source: [String: Any] = ["assetId": Self.sourceID, "sha256": String(repeating: "a", count: 64), "bytes": 1000, "mime": "image/png"]
            data = try! JSONSerialization.data(withJSONObject: ["id": Self.jobID, "state": "queued", "progress": 0,
                "attempts": 0, "errorCode": NSNull(), "retryAt": NSNull(), "assetId": NSNull(),
                "originalAssetId": NSNull(), "workerDrainVersion": 1, "workerDrained": false,
                "thumbnailPreparation": ["profile": "current-approved-image-thumbnail-256-v1", "artworkRevision": 2,
                    "source": source, "status": "pending", "workerDrained": false]])
        } else {
            var metadata = ArtworkMetadata(); metadata.rights.permissions.display = true; metadata.rights.permissions.export = true
            data = try! JSONEncoder().encode(NativeArtwork(id: Self.id, artistId: Self.requestID,
                revision: 2, approvedRevision: stale ? 1 : 2, approvedAssetId: Self.sourceID, metadata: metadata,
                assets: [NativeArtwork.Asset(id: Self.sourceID, mime: "image/png", state: "approved",
                    sha256: String(repeating: "a", count: 64), bytes: 1000)], archived: false))
        }
        let response = HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: nil,
            headerFields: ["Content-Type": "application/json"])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
