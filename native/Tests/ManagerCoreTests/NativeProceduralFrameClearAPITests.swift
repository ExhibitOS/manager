// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class NativeProceduralFrameClearAPITests: XCTestCase {
    private let owner = "11111111-1111-4111-8111-111111111111"
    private let placement = "22222222-2222-4222-8222-222222222222"
    private func fixture() throws -> (NativeDocumentState, NativeProceduralFrameClear.Proposal) {
        var draft = try NativeExhibitionDraft.new(title: "Synthetic frame clear")
        var candidate = draft.candidate
        candidate["placements"] = .array([.object(["id": .string(placement), "roomId": draft.rooms[0]["id"]])])
        try draft.replaceCandidate(candidate)
        try draft.setInstallation(.init(frame: .init(kind: .flat)), for: placement)
        let state = NativeDocumentState(format: 1, revision: 7, workspaceId: owner, tenantId: owner,
            principalId: owner, draft: draft, etag: "\"studio-r1-synthetic\"",
            savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        return (state, try .init(command: .init(.object(["placementId": .string(placement)])), draft: draft))
    }
    private func connected(_ state: NativeDocumentState, role: String = "admin", mode: String = "valid") async throws -> AuthoringAPI {
        try FrameClearAPIProtocol.configure(state, role: role, mode: mode)
        let api = try AuthoringAPI(port: 18091, tenantId: owner,
            nativeRuntimeManifestSHA256: "a81d7ec85895c455dfeca5b67ecff5124eebb0dc43fbd703e7a9a34da164d98f",
            protocolClasses: [FrameClearAPIProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: owner, principalId: owner,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        return api
    }
    func testPreparationAndApplyRevalidationBothUseServerCASWithoutPUT() async throws {
        let (state, proposal) = try fixture(), api = try await connected(state)
        try await api.validateProceduralFrameClear(proposal, in: state)
        try await api.validateProceduralFrameClear(proposal, in: state)
        let calls = FrameClearAPIProtocol.calls()
        let posts = calls.filter { $0.httpMethod == "POST" }
        XCTAssertEqual(posts.count, 2)
        XCTAssertFalse(calls.contains { $0.httpMethod == "PUT" })
        for request in posts {
            XCTAssertTrue(request.url!.path.hasSuffix("/native-viewport-check"))
            XCTAssertEqual(request.value(forHTTPHeaderField: "If-Match"), state.etag)
        }
    }
    func testServerRefusalIsPreservedBeforeAnyLocalOrDurableSave() async throws {
        let (state, proposal) = try fixture(), api = try await connected(state, mode: "reject")
        do { try await api.validateProceduralFrameClear(proposal, in: state); XCTFail("Server refusal must propagate") }
        catch let failure as AuthoringFailure { XCTAssertEqual(failure.code, "BOUNDED_EDIT_UNSUPPORTED") }
        XCTAssertEqual(FrameClearAPIProtocol.calls().filter { $0.httpMethod == "POST" }.count, 1)
        XCTAssertFalse(FrameClearAPIProtocol.calls().contains { $0.httpMethod == "PUT" })
    }
    func testUnknownPolicyStaleHeadRevokedCapabilityAndViewerCannotValidate() async throws {
        for mode in ["unknown-policy", "bounded-required", "stale", "revoked"] {
            let (state, proposal) = try fixture(), api = try await connected(state, mode: mode)
            do { try await api.validateProceduralFrameClear(proposal, in: state); XCTFail("Must refuse \(mode)") }
            catch { }
            XCTAssertFalse(FrameClearAPIProtocol.calls().contains { $0.httpMethod == "POST" }, mode)
        }
        let (state, proposal) = try fixture(), api = try await connected(state, role: "viewer")
        do { try await api.validateProceduralFrameClear(proposal, in: state); XCTFail("Viewer must refuse") }
        catch let failure as AuthoringFailure { XCTAssertEqual(failure.code, "PROCEDURAL_FRAME_CLEAR_UNAVAILABLE") }
        XCTAssertEqual(FrameClearAPIProtocol.calls().count, 1)
    }
}

private final class FrameClearAPIProtocol: URLProtocol, @unchecked Sendable {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var requests: [URLRequest] = []
    nonisolated(unsafe) private static var responses: [String: Data] = [:]
    nonisolated(unsafe) private static var mode = "valid"
    static func configure(_ state: NativeDocumentState, role: String, mode: String) throws {
        let draft = try JSONSerialization.jsonObject(with: JSONEncoder().encode(state.draft!))
        let id = state.draft!.id, prefix = "/api/v1/tenants/\(state.tenantId)/studio/exhibitions/\(id)"
        var session: [String: Any] = ["tenantId": state.tenantId, "userId": state.principalId, "role": role,
            "csrfToken": "synthetic-csrf", "expiresAt": "2099-01-01T00:00:00Z",
            "workspaceFurnishingProfile": "workspace-furnishing-intake-v1", "workspaceFurnishingStoolProfile": "workspace-furnishing-stool-v1",
            "workspaceFurnishingRoomVisualProfile": "workspace-furnishing-room-visual-v1", "workspaceArtworkFrameProfile": "workspace-artwork-frame-v1"]
        var policy: [String: Any] = ["exhibitionId": id, "revision": 1, "etag": state.etag!,
            "boundedReadOnly": false, "boundedProfile": true, "boundedEditRequired": false]
        if mode == "unknown-policy" { policy.removeValue(forKey: "boundedEditRequired") }
        if mode == "bounded-required" { policy["boundedEditRequired"] = true }
        let saved: [String: Any] = ["revision": 1, "etag": mode == "stale" ? "stale" : state.etag!, "draft": draft]
        let initial = try JSONSerialization.data(withJSONObject: session)
        if mode == "revoked" { session.removeValue(forKey: "workspaceArtworkFrameProfile") }
        let data = try ["/api/v1/auth/session": initial, "/retained-session": JSONSerialization.data(withJSONObject: session),
            prefix: JSONSerialization.data(withJSONObject: saved), prefix + "/native-viewport-policy": JSONSerialization.data(withJSONObject: policy)]
        lock.lock(); defer { lock.unlock() }; requests = []; responses = data; Self.mode = mode
    }
    static func calls() -> [URLRequest] { lock.lock(); defer { lock.unlock() }; return requests }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        Self.lock.lock(); let path = request.url!.path
        let retained = path == "/api/v1/auth/session" && Self.requests.contains { $0.url?.path == path }
        Self.requests.append(request)
        let rejected = request.httpMethod == "POST" && Self.mode == "reject"
        let data = Self.responses[retained ? "/retained-session" : path]
            ?? Data((rejected ? "{\"code\":\"BOUNDED_EDIT_UNSUPPORTED\"}" : "{\"valid\":true}").utf8)
        Self.lock.unlock()
        let response = HTTPURLResponse(url: request.url!, statusCode: rejected ? 422 : 200, httpVersion: nil,
            headerFields: ["Content-Type": "application/json"])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
