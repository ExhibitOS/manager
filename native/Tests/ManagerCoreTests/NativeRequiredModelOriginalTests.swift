// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class NativeRequiredModelOriginalTests: XCTestCase {
    private let id = "10000000-0000-4000-8000-000000000001"
    private let other = "20000000-0000-4000-8000-000000000002"
    private var digest: String { String(repeating: "a", count: 64) }
    private func custody() -> NativeRequiredModelCustody {
        .init(version: 1, kind: NativeRequiredModelCustody.profile,
            workspaceId: id, tenantId: id, principalId: id,
            artworkId: id, artworkRevisionId: id, importJobId: id, artworkRevision: 2,
            serverReceiptSha256: digest, rightsSha256: digest,
            original: .init(assetId: id, mime: "model/gltf-binary", sha256: digest, bytes: 100),
            display: .init(assetId: other, mime: "model/gltf-binary", sha256: digest, bytes: 80, reusedOriginal: false))
    }
    private func state(workspace: String? = nil) -> NativeDocumentState {
        .init(format: 1, revision: 0, workspaceId: workspace ?? id, tenantId: id, principalId: id,
            draft: nil, etag: nil, savedEditVersion: nil, pending: nil)
    }
    private func artwork() -> DocumentValue {
        .object(["id": .string(id), "revision": .integer(2), "revisionId": .string(id), "primaryAssetId": .string(other)])
    }
    func testDiscoveryBindsCurrentOwnerAndCMSSelectionWithoutTransferState() throws {
        let value = custody()
        try value.validate(in: state(), artwork: artwork())
        XCTAssertThrowsError(try value.validate(in: state(workspace: other), artwork: artwork()))
        var stale = artwork(); stale["revision"] = .integer(1)
        XCTAssertThrowsError(try value.validate(in: state(), artwork: stale))
        var replaced = artwork(); replaced["primaryAssetId"] = .string(id)
        XCTAssertThrowsError(try value.validate(in: state(), artwork: replaced))
        let bytes = try JSONEncoder().encode(value)
        XCTAssertEqual(try JSONDecoder().decode(NativeRequiredModelCustody.self, from: bytes), value)
    }
    func testApprovalReadbackBindsExactCustodyAndCurrentHead() throws {
        let value = custody(), etag = "\"studio-r7-\(digest)\""
        let receipt = NativeRequiredModelOriginalApproval(version: 1, approved: true, exhibitionId: id,
            revision: 7, etag: etag, custody: value, approvalSha256: digest)
        try receipt.validate(custody: value, exhibitionId: id, revision: 7, etag: etag)
        XCTAssertThrowsError(try receipt.validate(custody: value, exhibitionId: id, revision: 8, etag: etag))
        XCTAssertThrowsError(try receipt.validate(custody: value, exhibitionId: other, revision: 7, etag: etag))
        let incomplete = NativeRequiredModelOriginalApproval(version: 1, approved: true, exhibitionId: id,
            revision: 7, etag: etag, custody: value, approvalSha256: nil)
        XCTAssertThrowsError(try incomplete.validate(custody: value, exhibitionId: id, revision: 7, etag: etag))
        let unapproved = NativeRequiredModelOriginalApproval(version: 1, approved: false, exhibitionId: id,
            revision: 7, etag: etag, custody: value, approvalSha256: nil)
        try unapproved.validate(custody: value, exhibitionId: id, revision: 7, etag: etag)
    }

    func testApprovalUsesClosedBodyAndProtectedReadCSRFWithoutSavingDraft() async throws {
        let value = custody(), draft = try NativeExhibitionDraft.new(), etag = "\"studio-r7-\(digest)\""
        let document = NativeDocumentState(format: 1, revision: 9, workspaceId: id, tenantId: id, principalId: id,
            draft: draft, etag: etag, savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        let session = try JSONSerialization.data(withJSONObject: ["tenantId": id, "userId": id, "role": "owner",
            "csrfToken": "synthetic-csrf", "expiresAt": "2099-01-01T00:00:00Z",
            "nativeRequiredModelOriginalProfile": NativeRequiredModelCustody.profile])
        var scopedArtwork = artwork()
        scopedArtwork["extensions"] = .object(["org.exhibitos.studio/cms": .object(["tenantId": .string(id)])])
        let snapshot = try JSONEncoder().encode(DocumentValue.object(["artwork": scopedArtwork,
            "previewUrl": .string("/api/v1/tenants/\(id)/cms/artworks/\(id)/preview")]))
        let saved = try JSONEncoder().encode(DocumentValue.object(["revision": .integer(7), "etag": .string(etag),
            "draft": try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(draft))]))
        let receipt = NativeRequiredModelOriginalApproval(version: 1, approved: true, exhibitionId: draft.id,
            revision: 7, etag: etag, custody: value, approvalSha256: digest)
        let body = try JSONEncoder().encode(receipt)
        RequiredOriginalProtocol.configure { request in
            let path = request.url!.path
            if path == "/api/v1/auth/session" { return (200, session, nil) }
            if path.hasSuffix("/native-original-static-approval") || path.hasSuffix("/native-required-model-original") {
                guard request.value(forHTTPHeaderField: "x-csrf-token") == "synthetic-csrf",
                      request.value(forHTTPHeaderField: "Origin") == "http://127.0.0.1:18091" else {
                    return (403, Data("{\"code\":\"FORBIDDEN\"}".utf8), nil)
                }
                if request.httpMethod == "POST", request.value(forHTTPHeaderField: "If-Match") != etag {
                    return (412, Data("{\"code\":\"REMOTE_CONFLICT\"}".utf8), nil)
                }
                return (200, body, etag)
            }
            if path.contains("/studio/artworks/") { return (200, snapshot, nil) }
            return (200, saved, etag)
        }
        defer { RequiredOriginalProtocol.configure(nil) }
        let api = try AuthoringAPI(port: 18091, tenantId: id, nativeRuntimeManifestSHA256: String(repeating: "b", count: 64), protocolClasses: [RequiredOriginalProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: id, principalId: id,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        let readback = try await api.requiredModelOriginalApproval(value, in: document, approve: true)
        XCTAssertEqual(readback, receipt)
        let posts = RequiredOriginalProtocol.posts()
        XCTAssertEqual(posts.count, 1)
        XCTAssertTrue(posts[0].url!.path.hasSuffix("/native-original-static-approval"))
        let input = try XCTUnwrap(try JSONSerialization.jsonObject(with: XCTUnwrap(posts[0].httpBody)) as? [String: Any])
        XCTAssertEqual(Set(input.keys), ["kind", "artworkId", "artworkRevision", "importJobId", "serverReceiptSha256"])
        XCTAssertEqual(input["kind"] as? String, NativeRequiredModelCustody.profile)
    }
}

private final class RequiredOriginalProtocol: URLProtocol, @unchecked Sendable {
    typealias Handler = @Sendable (URLRequest) -> (Int, Data, String?)
    private static let lock = NSLock()
    nonisolated(unsafe) private static var handler: Handler?
    nonisolated(unsafe) private static var requests: [URLRequest] = []
    static func configure(_ value: Handler?) { lock.lock(); defer { lock.unlock() }; handler = value; requests = [] }
    static func posts() -> [URLRequest] { lock.lock(); defer { lock.unlock() }; return requests.filter { $0.httpMethod == "POST" } }
    private static func response(_ request: URLRequest) -> (Int, Data, String?) {
        lock.lock(); defer { lock.unlock() }
        var recorded = request
        if request.httpMethod == "POST", request.httpBody == nil, let stream = request.httpBodyStream {
            stream.open(); defer { stream.close() }
            var data = Data(), buffer = [UInt8](repeating: 0, count: 4096)
            while true {
                let count = stream.read(&buffer, maxLength: buffer.count)
                if count <= 0 { break }
                data.append(contentsOf: buffer.prefix(count))
            }
            recorded.httpBodyStream = nil; recorded.httpBody = data
        }
        requests.append(recorded)
        return handler?(request) ?? (500, Data(), nil)
    }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        let (status, data, etag) = Self.response(request)
        var headers = ["Content-Type": "application/json"]; if let etag { headers["ETag"] = etag }
        let response = HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: "HTTP/1.1", headerFields: headers)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
