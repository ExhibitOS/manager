// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
import CoreGraphics
import ImageIO
@testable import ManagerCore

@MainActor
final class CatalogThumbnailAuthorityTests: XCTestCase {
    func testRendererSharesCurrentAPICredentialsAndCannotReuseAnOldOrClosedSession() async throws {
        let id = UUID().uuidString.lowercased()
        func body(_ csrf: String) throws -> Data {
            try JSONSerialization.data(withJSONObject: ["tenantId": id, "userId": id, "role": "owner",
                "csrfToken": csrf, "expiresAt": "2099-01-01T00:00:00Z",
                "nativeRenderedArtworkThumbnailProfile": ViewportRenderedThumbnail.profile])
        }
        let firstBody = try body("first")
        CatalogThumbnailProtocol.configure { _ in (200, firstBody) }
        defer { CatalogThumbnailProtocol.configure(nil) }
        let api = try AuthoringAPI(port: 18091, tenantId: id, protocolClasses: [CatalogThumbnailProtocol.self])
        let firstCredentials = LocalAuthoringSession(tenantId: id, principalId: id,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43))
        let first = try await api.connect(firstCredentials)
        let shared = try await api.thumbnailRendererCredentials(for: first)
        XCTAssertEqual(shared.token, firstCredentials.token)
        XCTAssertEqual(shared.origin, firstCredentials.origin)
        XCTAssertEqual(shared.tenantId, id); XCTAssertEqual(shared.principalId, id)
        XCTAssertEqual(CatalogThumbnailProtocol.observedMethods(), ["GET"])
        let nextBody = try body("next")
        CatalogThumbnailProtocol.configure { _ in (200, nextBody) }
        let nextCredentials = LocalAuthoringSession(tenantId: id, principalId: id,
            origin: "http://127.0.0.1:18091", token: String(repeating: "b", count: 43))
        let next = try await api.connect(nextCredentials)
        do { _ = try await api.thumbnailRendererCredentials(for: first); XCTFail("An old session cannot reuse renewed credentials") }
        catch let failure as AuthoringFailure { XCTAssertEqual(failure.code, "AUTH_REQUIRED") }
        let current = try await api.thumbnailRendererCredentials(for: next)
        XCTAssertEqual(current.token, nextCredentials.token)
        await api.endLocalAuthoringSession()
        do { _ = try await api.thumbnailRendererCredentials(for: next); XCTFail("A closed session cannot render") }
        catch let failure as AuthoringFailure { XCTAssertEqual(failure.code, "AUTH_REQUIRED") }
        XCTAssertEqual(CatalogThumbnailProtocol.observedMethods(), ["GET"])
    }

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
            "expiresAt": "2099-01-01T00:00:00Z", "nativeRenderedArtworkThumbnailProfile": ViewportRenderedThumbnail.profile,
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
            sourceSha256: sha, displaySha256: sha, recipeId: ViewportRenderedThumbnail.recipe,
            toolId: ViewportRenderedThumbnail.tool, modelBytes: 128, pngData: Data([0]), pngSha256: sha)
        do {
            try await api.saveRenderedArtworkThumbnail(rendered, requestId: UUID(), expectedSelectionVersion: 0,
                                                       session: session, authority: authority)
            XCTFail("A revoked workbench must not save a selection")
        } catch let failure as AuthoringFailure {
            XCTAssertEqual(failure.code, "CATALOG_PREVIEW_UNAVAILABLE")
        }
        XCTAssertEqual(CatalogThumbnailProtocol.observedMethods(), ["GET", "GET"])
    }
    func testOldSessionProfileCannotReadOrPostRenderedSelection() async throws {
        let id = UUID().uuidString.lowercased()
        let body = try JSONSerialization.data(withJSONObject: ["tenantId": id, "userId": id, "role": "owner", "csrfToken": "test", "expiresAt": "2099-01-01T00:00:00Z"])
        CatalogThumbnailProtocol.configure { _ in (200, body) }
        defer { CatalogThumbnailProtocol.configure(nil) }
        let api = try AuthoringAPI(port: 18091, tenantId: id, protocolClasses: [CatalogThumbnailProtocol.self])
        let session = try await api.connect(LocalAuthoringSession(tenantId: id, principalId: id,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        XCTAssertFalse(session.advertisesRenderedArtworkThumbnail)
        do {
            _ = try await api.artworkThumbnailSelectionVersion(artworkId: id, revision: 1, session: session, authority: CatalogThumbnailAuthority())
            XCTFail("Old runtime readiness must not admit rendered selection")
        } catch let error as AuthoringFailure { XCTAssertEqual(error.code, "CATALOG_PREVIEW_UNAVAILABLE") }
        XCTAssertEqual(CatalogThumbnailProtocol.observedMethods(), ["GET"])
    }

    func testConfirmedCanonicalCandidateUsesSameTenFieldsAndReceiptOnReplay() async throws {
        let id = UUID().uuidString.lowercased(), source = UUID().uuidString.lowercased()
        let thumb = UUID().uuidString.lowercased(), binding = UUID().uuidString.lowercased(), requestID = UUID()
        let sourceHash = String(repeating: "a", count: 64), displayHash = String(repeating: "b", count: 64)
        let context = try XCTUnwrap(CGContext(data: nil, width: 256, height: 256, bitsPerComponent: 8, bytesPerRow: 1024,
            space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        context.setFillColor(CGColor(red: 0.2, green: 0.4, blue: 0.6, alpha: 1)); context.fill(CGRect(x: 0, y: 0, width: 256, height: 256))
        let image = try XCTUnwrap(context.makeImage()), data = NSMutableData()
        let destination = try XCTUnwrap(CGImageDestinationCreateWithData(data, "public.png" as CFString, 1, nil))
        CGImageDestinationAddImage(destination, image, nil); XCTAssertTrue(CGImageDestinationFinalize(destination))
        let png = try GeneratedThumbnailPNG.canonical(data as Data), pngHash = nativeImportSHA256(png)
        let sessionBody = try JSONSerialization.data(withJSONObject: ["tenantId": id, "userId": id, "role": "owner", "csrfToken": "test",
            "expiresAt": "2099-01-01T00:00:00Z", "nativeRenderedArtworkThumbnailProfile": ViewportRenderedThumbnail.profile])
        let artworkBody = try JSONEncoder().encode(NativeArtwork(id: id, artistId: id, revision: 2, approvedRevision: 2,
            approvedAssetId: source, metadata: nil, assets: [.init(id: source, mime: "model/gltf-binary", state: "approved", sha256: sourceHash, bytes: 256)], archived: false))
        let receiptBody = try JSONSerialization.data(withJSONObject: ["id": binding, "thumbnailAssetId": thumb,
            "thumbnailSha256": pngHash, "selectionVersion": 1, "selected": true])
        CatalogThumbnailProtocol.configure { request in
            if request.url?.path == "/api/v1/auth/session" { return (200, sessionBody) }
            if request.httpMethod == "GET" { return (200, artworkBody) }
            return (200, receiptBody)
        }
        defer { CatalogThumbnailProtocol.configure(nil) }
        let api = try AuthoringAPI(port: 18091, tenantId: id, protocolClasses: [CatalogThumbnailProtocol.self])
        let session = try await api.connect(LocalAuthoringSession(tenantId: id, principalId: id,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        let rendered = ViewportRenderedThumbnail(artworkId: id, revision: 2, sourceAssetId: source, sourceSha256: sourceHash,
            displaySha256: displayHash, recipeId: ViewportRenderedThumbnail.recipe, toolId: ViewportRenderedThumbnail.tool,
            modelBytes: 128, pngData: png, pngSha256: pngHash)
        let authority = CatalogThumbnailAuthority()
        for _ in 0..<2 {
            let receipt = try await api.saveRenderedArtworkThumbnail(rendered, requestId: requestID, expectedSelectionVersion: 0,
                session: session, authority: authority)
            XCTAssertEqual(receipt.id.uuidString.lowercased(), binding)
            XCTAssertEqual(receipt.thumbnailAssetId.uuidString.lowercased(), thumb)
            XCTAssertEqual(receipt.thumbnailSha256, pngHash)
        }
        let bodies = CatalogThumbnailProtocol.observedBodies()
        XCTAssertEqual(bodies.count, 2)
        XCTAssertEqual(try JSONSerialization.jsonObject(with: bodies[0]) as? NSDictionary,
                       try JSONSerialization.jsonObject(with: bodies[1]) as? NSDictionary,
                       "Retry must use the same canonical candidate and request ID")
        let payload = try XCTUnwrap(JSONSerialization.jsonObject(with: bodies[0]) as? [String: Any])
        XCTAssertEqual(Set(payload.keys), ["kind", "requestId", "expectedRevision", "expectedSelectionVersion", "sourceAssetId", "sourceSha256", "displaySha256", "pngSha256", "pngBase64", "view"])
        XCTAssertEqual(payload["requestId"] as? String, requestID.uuidString.lowercased())
        XCTAssertEqual(payload["sourceSha256"] as? String, sourceHash)
        XCTAssertEqual(payload["displaySha256"] as? String, displayHash)
        XCTAssertEqual(Data(base64Encoded: try XCTUnwrap(payload["pngBase64"] as? String)), png)
    }

}

private final class CatalogThumbnailProtocol: URLProtocol, @unchecked Sendable {
    private typealias Handler = @Sendable (URLRequest) -> (Int, Data)
    private static let lock = NSLock()
    nonisolated(unsafe) private static var handler: Handler?
    nonisolated(unsafe) private static var methods: [String] = []
    nonisolated(unsafe) private static var bodies: [Data] = []

    fileprivate static func configure(_ handler: (@Sendable (URLRequest) -> (Int, Data))?) {
        lock.lock(); defer { lock.unlock() }
        self.handler = handler
        methods = []; bodies = []
    }

    static func observedMethods() -> [String] {
        lock.lock(); defer { lock.unlock() }
        return methods
    }

    static func observedBodies() -> [Data] {
        lock.lock(); defer { lock.unlock() }; return bodies
    }
    private static func handler(for request: URLRequest) -> Handler? {
        lock.lock(); defer { lock.unlock() }
        methods.append(request.httpMethod ?? "")
        if request.httpMethod == "POST" {
            if let body = request.httpBody { bodies.append(body) }
            else if let stream = request.httpBodyStream {
                stream.open(); defer { stream.close() }; var body = Data(), buffer = [UInt8](repeating: 0, count: 4096)
                while true { let count = stream.read(&buffer, maxLength: buffer.count); if count <= 0 { break }; body.append(contentsOf: buffer.prefix(count)) }
                bodies.append(body)
            }
        }
        return handler
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        guard let url = request.url, let handler = Self.handler(for: request) else {
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
