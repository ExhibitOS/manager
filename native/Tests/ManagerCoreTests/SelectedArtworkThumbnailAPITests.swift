// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
import CoreGraphics
import ImageIO
@testable import ManagerCore

@MainActor
final class SelectedArtworkThumbnailAPITests: XCTestCase {
    private let manifest = "1b542833230e3f80c3850b09fb677cc119415a04b15e53c4904942eb76b5a238"
    private let id = "11111111-1111-4111-8111-111111111111"
    private let binding = UUID(uuidString: "22222222-2222-4222-8222-222222222222")!

    private func png() throws -> Data {
        let context = try XCTUnwrap(CGContext(data: nil, width: 8, height: 8, bitsPerComponent: 8,
            bytesPerRow: 32, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        let image = try XCTUnwrap(context.makeImage()), output = NSMutableData()
        let destination = try XCTUnwrap(CGImageDestinationCreateWithData(output, "public.png" as CFString, 1, nil))
        CGImageDestinationAddImage(destination, image, nil)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
        return output as Data
    }
    private func configure(_ image: Data, status: Int = 200, changes: [String: String] = [:],
                           revoke: CatalogThumbnailAuthority? = nil, advertise: Bool = true) throws {
        var sessionFields: [String: Any] = ["tenantId": id, "userId": id, "role": "admin",
            "csrfToken": "synthetic-csrf", "expiresAt": "2099-01-01T00:00:00Z",
            "nativeRenderedArtworkThumbnailProfile": ViewportRenderedThumbnail.profile]
        if !advertise { sessionFields.removeValue(forKey: "nativeRenderedArtworkThumbnailProfile") }
        let session = try JSONSerialization.data(withJSONObject: sessionFields)
        let selection = try JSONEncoder().encode(NativeArtworkThumbnailSelection(selectionVersion: 1, bindingId: binding))
        let bindingId = binding.uuidString.lowercased(), digest = nativeImportSHA256(image)
        SelectedPNGProtocol.configure { request in
            if request.url?.path == "/api/v1/auth/session" { return (200, ["Content-Type": "application/json"], session) }
            if request.url?.path.hasSuffix("/thumbnail-selection") == true {
                return (200, ["Content-Type": "application/json"], selection)
            }
            revoke?.invalidate()
            var headers = ["Content-Type": "image/png", "Content-Length": String(image.count), "Cache-Control": "no-store",
                "x-exhibitos-sha256": digest, "x-exhibitos-artwork-revision": "2",
                "x-exhibitos-thumbnail-selection-version": "1", "x-exhibitos-thumbnail-binding-id": bindingId]
            headers.merge(changes) { _, changed in changed }
            if status != 200 { return (status, ["Content-Type": "application/json"], Data("{\"code\":\"SELECTION_CONFLICT\"}".utf8)) }
            return (status, headers, image)
        }
    }
    private func connect(_ manifest: String?) async throws -> (AuthoringAPI, AuthoringSession) {
        let api = try AuthoringAPI(port: 18091, tenantId: id, nativeRuntimeManifestSHA256: manifest,
            protocolClasses: [SelectedPNGProtocol.self])
        let session = try await api.connect(LocalAuthoringSession(tenantId: id, principalId: id,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        return (api, session)
    }

    func testReadsCurrentSelectionAndItsExactIssuedBodyWithNativeHeaders() async throws {
        let image = try png(); try configure(image)
        defer { SelectedPNGProtocol.configure(nil) }
        let (api, session) = try await connect(manifest), authority = CatalogThumbnailAuthority()
        let selection = try await api.artworkThumbnailSelection(artworkId: id, revision: 2, session: session, authority: authority)
        let body = try await api.selectedArtworkThumbnail(artworkId: id, revision: 2, selection: selection, session: session, authority: authority)
        XCTAssertEqual(body, image)
        let requests = SelectedPNGProtocol.requests()
        XCTAssertEqual(requests.count, 3)
        XCTAssertEqual(requests.map(\.httpMethod), ["GET", "GET", "GET"])
        let read = try XCTUnwrap(requests.last)
        XCTAssertEqual(read.url?.path, "/api/v1/tenants/\(id)/cms/artworks/\(id)/thumbnail-selection/\(binding.uuidString.lowercased())/image")
        XCTAssertEqual(read.url?.query, "expectedRevision=2&expectedSelectionVersion=1")
        XCTAssertEqual(read.value(forHTTPHeaderField: "Origin"), "http://127.0.0.1:18091")
        XCTAssertEqual(read.value(forHTTPHeaderField: "x-csrf-token"), "synthetic-csrf")
        XCTAssertEqual(read.value(forHTTPHeaderField: "Accept-Encoding"), "identity")
    }

    func testQualifiedAreaRuntimeUsesExistingIdentityHeadersAndRejectsStaleBinding() async throws {
        let image = try png(), area = "a81d7ec85895c455dfeca5b67ecff5124eebb0dc43fbd703e7a9a34da164d98f"
        try configure(image)
        let (api, session) = try await connect(area), authority = CatalogThumbnailAuthority()
        let admitted = await api.admitsSelectedArtworkThumbnailRead; XCTAssertTrue(admitted)
        let selection = try await api.artworkThumbnailSelection(artworkId: id, revision: 2, session: session, authority: authority)
        let body = try await api.selectedArtworkThumbnail(artworkId: id, revision: 2, selection: selection, session: session, authority: authority)
        XCTAssertEqual(body, image)
        XCTAssertEqual(SelectedPNGProtocol.requests().map(\.httpMethod), ["GET", "GET", "GET"])
        try configure(image, changes: ["x-exhibitos-thumbnail-selection-version": "2"])
        let (staleAPI, staleSession) = try await connect(area)
        do {
            _ = try await staleAPI.selectedArtworkThumbnail(artworkId: id, revision: 2, selection: selection,
                session: staleSession, authority: CatalogThumbnailAuthority())
            XCTFail("Stale header must be rejected")
        } catch let error as AuthoringFailure { XCTAssertEqual(error.code, "CATALOG_PREVIEW_INVALID") }
        SelectedPNGProtocol.configure(nil)
    }
    func testQualifiedAreaPinStillRequiresItsLiveProfile() async throws {
        try configure(png(), advertise: false)
        let (api, session) = try await connect("a81d7ec85895c455dfeca5b67ecff5124eebb0dc43fbd703e7a9a34da164d98f")
        let admitted = await api.admitsSelectedArtworkThumbnailRead; XCTAssertFalse(admitted)
        do {
            _ = try await api.selectedArtworkThumbnail(artworkId: id, revision: 2,
                selection: .init(selectionVersion: 1, bindingId: binding), session: session, authority: CatalogThumbnailAuthority())
            XCTFail("Exact pin alone must not admit an image read")
        } catch let error as AuthoringFailure { XCTAssertEqual(error.code, "CATALOG_PREVIEW_UNAVAILABLE") }
        XCTAssertEqual(SelectedPNGProtocol.requests().count, 1)
        SelectedPNGProtocol.configure(nil)
    }

    func testOldRecipeFlagDoesNotAdmitBodyEndpointWithoutExactNewRuntime() async throws {
        for old in [nil, "ac2248e1184ad4514f386ab6ee11b6ddd8bf2ca71792f4ec9eb9242e189724d7",
                    "4b8a50b3b9debcafdc757048dd98a1a8997feaf894ead81543739d7bd1b85b84", String(repeating: "f", count: 64)] {
            try configure(png())
            let (api, session) = try await connect(old)
            do {
                _ = try await api.selectedArtworkThumbnail(artworkId: id, revision: 2,
                    selection: .init(selectionVersion: 1, bindingId: binding), session: session, authority: CatalogThumbnailAuthority())
                XCTFail("Old runtime admitted new image route")
            } catch let error as AuthoringFailure { XCTAssertEqual(error.code, "CATALOG_PREVIEW_UNAVAILABLE") }
            XCTAssertEqual(SelectedPNGProtocol.requests().count, 1)
        }
        SelectedPNGProtocol.configure(nil)
    }

    func testRejectsStaleSelectionWrongBindingAndRevokedAuthorityWithoutAnotherRequest() async throws {
        let image = try png()
        for scenario in ["stale", "wrong-binding", "revoked"] {
            let authority = CatalogThumbnailAuthority()
            try configure(image, status: scenario == "stale" ? 409 : 200,
                changes: scenario == "wrong-binding" ? ["x-exhibitos-thumbnail-binding-id": id] : [:],
                revoke: scenario == "revoked" ? authority : nil)
            let (api, session) = try await connect(manifest)
            do {
                _ = try await api.selectedArtworkThumbnail(artworkId: id, revision: 2,
                    selection: .init(selectionVersion: 1, bindingId: binding), session: session, authority: authority)
                XCTFail("Rejected image accepted: \(scenario)")
            } catch let error as AuthoringFailure {
                if scenario == "stale" { XCTAssertEqual(error.status, 409); XCTAssertEqual(error.code, "SELECTION_CONFLICT") }
            }
            XCTAssertEqual(SelectedPNGProtocol.requests().count, 2)
        }
        SelectedPNGProtocol.configure(nil)
    }
}

private final class SelectedPNGProtocol: URLProtocol, @unchecked Sendable {
    typealias Handler = @Sendable (URLRequest) -> (Int, [String: String], Data)
    private static let lock = NSLock()
    nonisolated(unsafe) private static var handler: Handler?
    nonisolated(unsafe) private static var observed: [URLRequest] = []
    static func configure(_ handler: Handler?) { lock.lock(); defer { lock.unlock() }; self.handler = handler; observed = [] }
    static func requests() -> [URLRequest] { lock.lock(); defer { lock.unlock() }; return observed }
    private static func next(_ request: URLRequest) -> Handler? {
        lock.lock(); defer { lock.unlock() }; observed.append(request); return handler
    }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        guard let handler = Self.next(request), let url = request.url else {
            client?.urlProtocol(self, didFailWithError: URLError(.badURL)); return
        }
        let (status, headers, body) = handler(request)
        guard let response = HTTPURLResponse(url: url, statusCode: status, httpVersion: "HTTP/1.1", headerFields: headers) else {
            client?.urlProtocol(self, didFailWithError: URLError(.badServerResponse)); return
        }
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: body)
        client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
