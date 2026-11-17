// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class NativeFinishedHayesOriginalTests: XCTestCase {
    private let id = "10000000-0000-4000-8000-000000000001"
    private let other = "20000000-0000-4000-8000-000000000002"
    private var digest: String { String(repeating: "a", count: 64) }
    private func custody() -> NativeFinishedHayesCustody {
        .init(version: 1, kind: NativeFinishedHayesCustody.profile,
            workspaceId: id, tenantId: id, principalId: id,
            artworkId: id, artworkRevisionId: id, importJobId: id, artworkRevision: 2,
            release: .init(attemptId: id, receiptId: other, receiptSha256: digest, completionFrameSha256: digest),
            original: .init(assetId: id, sha256: digest, mime: "model/gltf-binary", bytes: 100),
            display: .init(assetId: other, sha256: digest, mime: "model/gltf-binary", bytes: 80), rightsSha256: digest)
    }
    private func state(workspace: String? = nil) -> NativeDocumentState {
        .init(format: 1, revision: 0, workspaceId: workspace ?? id, tenantId: id, principalId: id,
            draft: nil, etag: nil, savedEditVersion: nil, pending: nil)
    }
    private func artwork() -> DocumentValue {
        .object(["id": .string(id), "revision": .integer(2), "revisionId": .string(id), "primaryAssetId": .string(other),
                 "assets": .array([.object(["id": .string(other), "sha256": .string(digest),
                    "mime": .string("model/gltf-binary"), "bytes": .integer(80)])])])
    }
    func testDiscoveryBindsCurrentOwnerAndCMSSelectionWithoutTransferState() throws {
        let value = custody()
        try value.validate(in: state(), artwork: artwork())
        XCTAssertThrowsError(try value.validate(in: state(workspace: other), artwork: artwork()))
        var stale = artwork(); stale["revision"] = .integer(1)
        XCTAssertThrowsError(try value.validate(in: state(), artwork: stale))
        var replaced = artwork(); replaced["primaryAssetId"] = .string(id)
        XCTAssertThrowsError(try value.validate(in: state(), artwork: replaced))
        var changedBytes = artwork(); changedBytes["assets"] = .array([.object(["id": .string(other),
            "sha256": .string(digest), "mime": .string("model/gltf-binary"), "bytes": .integer(81)])])
        XCTAssertThrowsError(try value.validate(in: state(), artwork: changedBytes))
        var raw = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(value)) as? [String: Any])
        var release = try XCTUnwrap(raw["release"] as? [String: Any]); release["receiptSha256"] = "not-a-digest"; raw["release"] = release
        let invalid = try JSONDecoder().decode(NativeFinishedHayesCustody.self, from: JSONSerialization.data(withJSONObject: raw))
        XCTAssertThrowsError(try invalid.validate(in: state(), artwork: artwork()))
        let bytes = try JSONEncoder().encode(value)
        XCTAssertEqual(try JSONDecoder().decode(NativeFinishedHayesCustody.self, from: bytes), value)
    }
    func testApprovalReadbackBindsExactCustodyAndCurrentHead() throws {
        let value = custody(), etag = "\"studio-r7-\(digest)\""
        let receipt = NativeFinishedHayesOriginalApproval(version: 1, approved: true, exhibitionId: id,
            revision: 7, etag: etag, custody: value, approvalSha256: digest)
        try receipt.validate(custody: value, exhibitionId: id, revision: 7, etag: etag)
        XCTAssertThrowsError(try receipt.validate(custody: value, exhibitionId: id, revision: 8, etag: etag))
        XCTAssertThrowsError(try receipt.validate(custody: value, exhibitionId: other, revision: 7, etag: etag))
        let incomplete = NativeFinishedHayesOriginalApproval(version: 1, approved: true, exhibitionId: id,
            revision: 7, etag: etag, custody: value, approvalSha256: nil)
        XCTAssertThrowsError(try incomplete.validate(custody: value, exhibitionId: id, revision: 7, etag: etag))
        let unapproved = NativeFinishedHayesOriginalApproval(version: 1, approved: false, exhibitionId: id,
            revision: 7, etag: etag, custody: value, approvalSha256: nil)
        try unapproved.validate(custody: value, exhibitionId: id, revision: 7, etag: etag)
    }

    func testApprovalUsesClosedBodyAndProtectedReadCSRFWithoutSavingDraft() async throws {
        let value = custody(), draft = try NativeExhibitionDraft.new(), etag = "\"studio-r7-\(digest)\""
        let document = NativeDocumentState(format: 1, revision: 9, workspaceId: id, tenantId: id, principalId: id,
            draft: draft, etag: etag, savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        let session = try JSONSerialization.data(withJSONObject: ["tenantId": id, "userId": id, "role": "owner",
            "csrfToken": "synthetic-csrf", "expiresAt": "2099-01-01T00:00:00Z",
            "nativeFinishedHayesOriginalProfile": NativeFinishedHayesCustody.profile])
        var scopedArtwork = artwork()
        scopedArtwork["extensions"] = .object(["org.exhibitos.studio/cms": .object(["tenantId": .string(id)])])
        let snapshot = try JSONEncoder().encode(DocumentValue.object(["artwork": scopedArtwork,
            "previewUrl": .string("/api/v1/tenants/\(id)/cms/artworks/\(id)/preview")]))
        let saved = try JSONEncoder().encode(DocumentValue.object(["revision": .integer(7), "etag": .string(etag),
            "draft": try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(draft))]))
        let receipt = NativeFinishedHayesOriginalApproval(version: 1, approved: true, exhibitionId: draft.id,
            revision: 7, etag: etag, custody: value, approvalSha256: digest)
        let body = try JSONEncoder().encode(receipt)
        FinishedHayesOriginalProtocol.configure { request in
            let path = request.url!.path
            if path == "/api/v1/auth/session" { return (200, session, nil) }
            if path.hasSuffix("/native-original-static-approval") || path.hasSuffix("/native-finished-hayes-original") {
                guard request.value(forHTTPHeaderField: "x-csrf-token") == "synthetic-csrf",
                      request.value(forHTTPHeaderField: "Origin") == "http://127.0.0.1:18091" else {
                    return (403, Data("{\"code\":\"FORBIDDEN\"}".utf8), nil)
                }
                if request.httpMethod == "POST", request.value(forHTTPHeaderField: "If-Match") != etag {
                    return (412, Data("{\"code\":\"REMOTE_CONFLICT\"}".utf8), nil)
                }
                if request.httpMethod == "GET" {
                    let fields = Dictionary(uniqueKeysWithValues: (URLComponents(url: request.url!, resolvingAgainstBaseURL: false)?.queryItems ?? []).map { ($0.name, $0.value ?? "") })
                    guard Set(fields.keys) == ["artworkId", "artworkRevision", "importJobId", "attemptId", "receiptId", "receiptSha256", "completionFrameSha256"],
                          fields["importJobId"] == value.importJobId, fields["attemptId"] == value.release.attemptId,
                          fields["receiptId"] == value.release.receiptId, fields["receiptSha256"] == value.release.receiptSha256,
                          fields["completionFrameSha256"] == value.release.completionFrameSha256 else {
                        return (422, Data("{\"code\":\"INVALID_INPUT\"}".utf8), nil)
                    }
                }
                return (200, body, etag)
            }
            if path.contains("/studio/artworks/") { return (200, snapshot, nil) }
            return (200, saved, etag)
        }
        defer { FinishedHayesOriginalProtocol.configure(nil) }
        let api = try AuthoringAPI(port: 18091, tenantId: id, nativeRuntimeManifestSHA256: String(repeating: "b", count: 64), protocolClasses: [FinishedHayesOriginalProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: id, principalId: id,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        let readback = try await api.finishedHayesOriginalApproval(value, in: document, approve: true)
        XCTAssertEqual(readback, receipt)
        let posts = FinishedHayesOriginalProtocol.posts()
        XCTAssertEqual(posts.count, 1)
        XCTAssertTrue(posts[0].url!.path.hasSuffix("/native-original-static-approval"))
        let input = try XCTUnwrap(try JSONSerialization.jsonObject(with: XCTUnwrap(posts[0].httpBody)) as? [String: Any])
        XCTAssertEqual(Set(input.keys), ["kind", "artworkId", "artworkRevision", "importJobId", "attemptId", "receiptId", "receiptSha256", "completionFrameSha256"])
        XCTAssertEqual(input["kind"] as? String, NativeFinishedHayesCustody.profile)
    }
    func testDistinctDiscoveryHasNoGenericFallbackAndRequiresLiveCapability() async throws {
        let value = custody()
        var art = artwork()
        art["extensions"] = .object(["org.exhibitos.studio/cms": .object(["tenantId": .string(id)])])
        let snapshot = try JSONEncoder().encode(DocumentValue.object(["artwork": art,
            "previewUrl": .string("/api/v1/tenants/\(id)/cms/artworks/\(id)/preview")]))
        for (advertised, member, expected) in [(true, "finishedHayesCustody", true),
                                               (true, "requiredModelCustody", false),
                                               (false, "finishedHayesCustody", false)] {
            var fields: [String: Any] = ["tenantId": id, "userId": id, "role": "owner", "csrfToken": "synthetic-csrf", "expiresAt": "2099-01-01T00:00:00Z"]
            if advertised { fields["nativeFinishedHayesOriginalProfile"] = NativeFinishedHayesCustody.profile }
            else { fields["nativeRequiredModelOriginalProfile"] = NativeRequiredModelCustody.profile }
            let session = try JSONSerialization.data(withJSONObject: fields)
            let projection = try JSONEncoder().encode(DocumentValue.object([member: try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(value))]))
            FinishedHayesOriginalProtocol.configure { request in
                if request.url!.path == "/api/v1/auth/session" { return (200, session, nil) }
                if request.url!.path.hasSuffix("/native-model-candidate") { return (200, projection, nil) }
                return (200, snapshot, nil)
            }
            let api = try AuthoringAPI(port: 18091, tenantId: id, nativeRuntimeManifestSHA256: String(repeating: "b", count: 64), protocolClasses: [FinishedHayesOriginalProtocol.self])
            let before = await api.admitsFinishedHayesOriginalApproval
            XCTAssertFalse(before)
            _ = try await api.connect(LocalAuthoringSession(tenantId: id, principalId: id, origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
            do {
                let found = try await api.discoverFinishedHayesOriginal(artworkId: id, revision: 2, in: state())
                XCTAssertEqual(found, value)
                XCTAssertTrue(expected)
            } catch { XCTAssertFalse(expected) }
            XCTAssertTrue(FinishedHayesOriginalProtocol.posts().isEmpty)
        }
        FinishedHayesOriginalProtocol.configure(nil)
    }

}

private final class FinishedHayesOriginalProtocol: URLProtocol, @unchecked Sendable {
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
