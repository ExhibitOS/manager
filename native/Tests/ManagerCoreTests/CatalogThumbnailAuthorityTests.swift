// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class CatalogThumbnailAuthorityTests: XCTestCase {
    func testRevokingAuthorityDuringArtworkReadPreventsSelectionPost() async throws {
        let tenant = UUID().uuidString.lowercased()
        let user = UUID().uuidString.lowercased()
        let artwork = UUID().uuidString.lowercased()
        let asset = UUID().uuidString.lowercased()
        let sha = String(repeating: "a", count: 64)
        let authority = CatalogThumbnailAuthority()
        let credentials = LocalAuthoringSession(tenantId: tenant, principalId: user, origin: "http://127.0.0.1:18091",
                                                token: String(repeating: "a", count: 43))
        let sessionBody = try JSONSerialization.data(withJSONObject: [
            "tenantId": tenant, "userId": user, "role": "owner", "csrfToken": "test",
            "expiresAt": "2099-01-01T00:00:00Z",
        ])
        let artworkBody = try JSONEncoder().encode(NativeArtwork(id: artwork, artistId: user, revision: 1,
            approvedRevision: 1, approvedAssetId: asset, metadata: nil,
            assets: [.init(id: asset, mime: "model/gltf-binary", state: "approved", sha256: sha, bytes: 1)],
            archived: false))
        CatalogThumbnailProtocol.configure { request in
            switch (request.httpMethod, request.url?.path) {
            case ("GET", "/api/v1/auth/session"): return (200, sessionBody)
            case ("GET", "/api/v1/tenants/\(tenant)/cms/artworks/\(artwork)"):
                // Simulate a lifecycle change while the artwork GET is in flight.
                authority.invalidate()
                return (200, artworkBody)
            default: return (500, Data("{}".utf8))
            }
        }
        defer { CatalogThumbnailProtocol.configure(nil) }
        let api = try AuthoringAPI(port: 18091, tenantId: tenant, protocolClasses: [CatalogThumbnailProtocol.self])
        let session = try await api.connect(credentials)
        let rendered = ViewportRenderedThumbnail(artworkId: artwork, revision: 1, sourceAssetId: asset,
            sourceSha256: sha, displaySha256: sha, pngData: Data([0]), pngSha256: sha)
        do {
            try await api.saveRenderedArtworkThumbnail(rendered, requestId: UUID(), expectedSelectionVersion: 0,
                                                       session: session, authority: authority)
            XCTFail("A revoked workbench must not save a selection")
        } catch let failure as AuthoringFailure {
            XCTAssertEqual(failure.code, "CATALOG_PREVIEW_UNAVAILABLE")
        }
        XCTAssertEqual(CatalogThumbnailProtocol.observedMethods(), ["GET", "GET"])
    }
}

private final class CatalogThumbnailProtocol: URLProtocol, @unchecked Sendable {
    private typealias Handler = @Sendable (URLRequest) -> (Int, Data)
    private static let lock = NSLock()
    nonisolated(unsafe) private static var handler: Handler?
    nonisolated(unsafe) private static var methods: [String] = []

    fileprivate static func configure(_ handler: (@Sendable (URLRequest) -> (Int, Data))?) {
        lock.lock(); defer { lock.unlock() }
        self.handler = handler
        methods = []
    }

    static func observedMethods() -> [String] {
        lock.lock(); defer { lock.unlock() }
        return methods
    }

    private static func handler(for method: String) -> Handler? {
        lock.lock(); defer { lock.unlock() }
        methods.append(method)
        return handler
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        guard let url = request.url, let handler = Self.handler(for: request.httpMethod ?? "") else {
            client?.urlProtocol(self, didFailWithError: URLError(.badURL))
            return
        }
        let (status, body) = handler(request)
        guard let response = HTTPURLResponse(url: url, statusCode: status, httpVersion: "HTTP/1.1",
                                            headerFields: ["Content-Type": "application/json"]) else {
            client?.urlProtocol(self, didFailWithError: URLError(.badServerResponse))
            return
        }
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: body)
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}
