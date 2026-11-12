// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class SharedFurnishingResourceAPITests: XCTestCase {
    private let tenant = "10000000-0000-4000-8000-000000000001"
    private let user = "10000000-0000-4000-8000-000000000002"

    private func configure(_ mode: SharedResourceProtocol.Mode, model: Data = Data(), notice: Data = Data()) throws -> AuthoringAPI {
        let fixture = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("Fixtures/shared-furnishing-catalog-fixture.json")
        let session = try JSONSerialization.data(withJSONObject: ["tenantId": tenant, "userId": user, "role": "admin", "csrfToken": "synthetic", "expiresAt": "2099-01-01T00:00:00Z"])
        SharedResourceProtocol.configure(.init(session: session, catalog: try Data(contentsOf: fixture), model: model, notice: notice, mode: mode))
        return try AuthoringAPI(port: 18091, tenantId: tenant, protocolClasses: [SharedResourceProtocol.self])
    }

    private func connect(_ api: AuthoringAPI) async throws {
        _ = try await api.connect(LocalAuthoringSession(tenantId: tenant, principalId: user,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
    }

    func testRejectsUnauthenticatedAndNonNativeSessionsBeforeCatalogRead() async throws {
        let api = try configure(.normal)
        defer { SharedResourceProtocol.configure(nil) }
        do { _ = try await api.sharedFurnishingsWithRefinedBenchChecks(); XCTFail("unauthenticated") } catch { }
        XCTAssertTrue(SharedResourceProtocol.paths().isEmpty)
        _ = try await api.login(subject: "fixture", password: "fixture")
        do { _ = try await api.sharedFurnishingsWithRefinedBenchChecks(); XCTFail("non-native session") } catch { }
        XCTAssertEqual(SharedResourceProtocol.paths(), ["/api/v1/auth/login", "/api/v1/auth/session"])
    }

    func testRejectsBadModelResponseBeforeNoticeRead() async throws {
        for mode in [SharedResourceProtocol.Mode.wrongMIME, .wrongLength, .wrongHash, .redirect, .forbidden] {
            let api = try configure(mode, model: Data(repeating: 0, count: 345_608))
            try await connect(api)
            do { _ = try await api.sharedFurnishingsWithRefinedBenchChecks(); XCTFail("bad model accepted: \(mode)") } catch { }
            let paths = SharedResourceProtocol.paths()
            XCTAssertEqual(paths.count, 3)
            XCTAssertTrue(paths.last?.hasSuffix("/sculpture-bench-refined/1/model") == true)
            XCTAssertFalse(paths.contains { $0.hasSuffix("/notice") })
        }
        SharedResourceProtocol.configure(nil)
    }

    func testRejectsUnexpectedLiveCatalogBeforeAssetRequests() async throws {
        let api = try configure(.wrongCatalog)
        defer { SharedResourceProtocol.configure(nil) }
        try await connect(api)
        do { _ = try await api.sharedFurnishingsWithRefinedBenchChecks(); XCTFail("unrecognized catalog accepted") } catch { }
        XCTAssertEqual(SharedResourceProtocol.paths().count, 2)
        XCTAssertTrue(SharedResourceProtocol.paths().last?.hasSuffix("/studio/furnishings") == true)
    }

    func testExactFrozenPairIsReadAndReportedWithoutBytes() async throws {
        guard let path = ProcessInfo.processInfo.environment["EXHIBITOS_TEST_REFINED_BENCH_ROOT"] else { throw XCTSkip("Requires reviewed refined bench source files") }
        let root = URL(fileURLWithPath: path, isDirectory: true)
        let model = try Data(contentsOf: root.appendingPathComponent("model.glb"))
        let notice = try Data(contentsOf: root.appendingPathComponent("LICENSE-NOTICE.txt"))
        let api = try configure(.normal, model: model, notice: notice)
        defer { SharedResourceProtocol.configure(nil) }
        try await connect(api)
        let result = try await api.sharedFurnishingsWithRefinedBenchChecks()
        XCTAssertEqual(result.items.count, 7)
        let checks = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(result.checks))
        XCTAssertEqual(checks, .array([
            .object(["role": .string("model"), "mime": .string("model/gltf-binary"), "bytes": .integer(345_608), "sha256": .string("e22f0acdda9f0e33f00fbe5c874eb12bc2799062e34ad2939fa0d88721bef1f9")]),
            .object(["role": .string("notice"), "mime": .string("text/plain"), "bytes": .integer(892), "sha256": .string("4e14161b2a63d79429a0f4861d0e8400691ad84d80f4b8974f48b1644db359b9")])]))
        XCTAssertEqual(SharedResourceProtocol.paths().suffix(2).map { $0.components(separatedBy: "/").last! }, ["model", "notice"])
        var changedNotice = notice
        changedNotice[changedNotice.startIndex] ^= 1
        let changedAPI = try configure(.normal, model: model, notice: changedNotice)
        try await connect(changedAPI)
        do { _ = try await changedAPI.sharedFurnishingsWithRefinedBenchChecks(); XCTFail("altered notice accepted") } catch { }
        XCTAssertTrue(SharedResourceProtocol.paths().last?.hasSuffix("/notice") == true)
    }
}

private final class SharedResourceProtocol: URLProtocol, @unchecked Sendable {
    enum Mode: Sendable { case normal, wrongMIME, wrongLength, wrongHash, wrongCatalog, redirect, forbidden }
    struct Plan: Sendable { let session: Data, catalog: Data, model: Data, notice: Data, mode: Mode }
    private static let lock = NSLock()
    nonisolated(unsafe) private static var plan: Plan?
    nonisolated(unsafe) private static var seen: [String] = []
    static func configure(_ next: Plan?) { lock.lock(); defer { lock.unlock() }; plan = next; seen = [] }
    static func paths() -> [String] { lock.lock(); defer { lock.unlock() }; return seen }
    private static func read(_ request: URLRequest) -> Plan? {
        lock.lock(); defer { lock.unlock() }; seen.append(request.url?.path ?? ""); return plan
    }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        guard let url = request.url, let plan = Self.read(request) else { client?.urlProtocol(self, didFailWithError: URLError(.badServerResponse)); return }
        var status = 200, mime = "application/json", body: Data
        if url.path == "/api/v1/auth/login" { body = Data("{\"authenticated\":true}".utf8) }
        else if url.path == "/api/v1/auth/session" { body = plan.session }
        else if url.path.hasSuffix("/studio/furnishings") { body = plan.mode == .wrongCatalog ? Data("{}".utf8) : plan.catalog }
        else if url.path.hasSuffix("/model") {
            body = plan.model; mime = plan.mode == .wrongMIME ? "text/plain" : "model/gltf-binary"
            if plan.mode == .redirect { status = 302 }
            if plan.mode == .forbidden { status = 403 }
        } else if url.path.hasSuffix("/notice") { body = plan.notice; mime = "text/plain; charset=utf-8" }
        else { status = 404; body = Data() }
        var headers = ["Content-Type": mime, "Content-Length": String(body.count)]
        if plan.mode == .wrongLength && url.path.hasSuffix("/model") { headers["Content-Length"] = "1" }
        if status == 302 { headers["Location"] = "http://127.0.0.1:18091/other" }
        guard let response = HTTPURLResponse(url: url, statusCode: status, httpVersion: "HTTP/1.1", headerFields: headers) else { return }
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: body); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
