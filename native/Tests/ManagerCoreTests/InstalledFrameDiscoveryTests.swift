// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class InstalledFrameDiscoveryTests: XCTestCase {
    func testInstalledAndTenantFramesWithSharedBytesKeepSeparateIdentities() async throws {
        let black = "4e670339-c7db-4d89-9430-94f24761b6b7"
        let installed = "af88109b-27e2-4d10-a08c-3ddc0224451a"
        let tenant = "66d45ba8-457e-4d23-999d-f242fc1c7590"
        let ids = [black, installed, tenant]
        let bytes = Data("Synthetic frame model".utf8), digest = nativeImportSHA256(bytes)
        var descriptors: [String: DocumentValue] = [:]
        for id in ids {
            let modelDigest = id == black ? String(repeating: "c", count: 64) : digest
            var item = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(
                FrameFitSynthetic.furnishing(id: id, kind: "artwork-frame", profile: "straight-miter-ring-v1")))
            item["model"]["bytes"] = .integer(Int64(bytes.count))
            item["model"]["sha256"] = .string(modelDigest)
            item["fit"]["sourceBytes"] = .integer(Int64(bytes.count))
            item["fit"]["sourceSha256"] = .string(modelDigest)
            descriptors[id] = item
        }
        try InstalledFrameProtocol.configure(ids: ids, descriptors: descriptors, model: bytes)
        let api = try AuthoringAPI(port: 18091, tenantId: FrameFitSynthetic.owner,
            nativeRuntimeManifestSHA256: "7e4baad682f01ae4896b051aa24b7fc93cc971fb57d85e689e570c5a808ca902",
            protocolClasses: [InstalledFrameProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: FrameFitSynthetic.owner, principalId: FrameFitSynthetic.owner,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        let page = try await api.workspaceFurnishings()
        XCTAssertEqual(page.items.map(\.id), ids)
        XCTAssertNil(page.nextCursor)
        let selected = try await api.workspaceFurnishing(installed, version: 1)
        let existingTenant = try await api.workspaceFurnishing(tenant, version: 1)
        XCTAssertEqual(selected.id, installed)
        XCTAssertEqual(existingTenant.id, tenant)
        XCTAssertNotEqual(selected.id, existingTenant.id)
        XCTAssertEqual(selected.details.model.sha256, existingTenant.details.model.sha256)
        let selectedValue = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(selected))
        XCTAssertEqual(selectedValue["fit"], descriptors[installed]!["fit"])
        let loaded = try await api.workspaceFurnishingModel(selected)
        XCTAssertEqual(loaded, bytes)
        let calls = InstalledFrameProtocol.requests()
        XCTAssertTrue(calls.contains { $0.url?.path.hasSuffix("/furnishings/\(installed)/versions/1/model") == true })
        XCTAssertTrue(calls.allSatisfy { $0.httpMethod == "GET" })
        XCTAssertFalse(calls.contains { $0.url?.path.contains("furnishing-intakes") == true })
    }
}

private final class InstalledFrameProtocol: URLProtocol, @unchecked Sendable {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var responses: [String: Data] = [:]
    nonisolated(unsafe) private static var calls: [URLRequest] = []
    nonisolated(unsafe) private static var model = Data()
    static func configure(ids: [String], descriptors: [String: DocumentValue], model: Data) throws {
        let session: [String: String] = ["tenantId": FrameFitSynthetic.owner, "userId": FrameFitSynthetic.owner,
            "role": "admin", "csrfToken": "synthetic-csrf", "expiresAt": "2099-01-01T00:00:00Z",
            "workspaceFurnishingProfile": "workspace-furnishing-intake-v1",
            "workspaceFurnishingStoolProfile": "workspace-furnishing-stool-v1",
            "workspaceFurnishingRoomVisualProfile": "workspace-furnishing-room-visual-v1",
            "workspaceArtworkFrameProfile": "workspace-artwork-frame-v1"]
        let prefix = "/api/v1/tenants/\(FrameFitSynthetic.owner)/workspace/furnishings"
        let items: [DocumentValue] = ids.map { .object(["id": .string($0), "version": .integer(1),
            "kind": .string("artwork-frame"), "name": .string("Synthetic"),
            "rightsAvailability": .object(["display": .bool(true), "download": .bool(false), "export": .bool(false)])]) }
        var responses = ["/api/v1/auth/session": try JSONSerialization.data(withJSONObject: session),
            prefix: try JSONEncoder().encode(DocumentValue.object(["items": .array(items), "nextCursor": .null]))]
        for id in ids { responses[prefix + "/\(id)/versions/1"] = try JSONEncoder().encode(descriptors[id]!) }
        lock.lock(); defer { lock.unlock() }
        Self.responses = responses; Self.calls = []; Self.model = model
    }
    static func requests() -> [URLRequest] { lock.lock(); defer { lock.unlock() }; return calls }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        Self.lock.lock()
        Self.calls.append(request)
        let model = Self.model, response = Self.responses[request.url!.path]
        Self.lock.unlock()
        let isModel = request.url!.path.hasSuffix("/model")
        let data = isModel ? model : response ?? Data()
        let headers = ["Content-Type": isModel ? "model/gltf-binary" : "application/json",
            "Content-Length": String(data.count), "x-exhibitos-sha256": nativeImportSHA256(data)]
        let http = HTTPURLResponse(url: request.url!, statusCode: isModel || response != nil ? 200 : 404,
            httpVersion: "HTTP/1.1", headerFields: headers)!
        client?.urlProtocol(self, didReceive: http, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
