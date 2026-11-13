// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class FrameImageFitReconciliationAPITests: XCTestCase {
    private func fixture(fitted: Bool = false) throws -> NativeDocumentState {
        let synthetic = try FrameFitSynthetic.draft(version: fitted ? 10 : 9, fitted: fitted)
        let draft = try JSONDecoder().decode(NativeExhibitionDraft.self, from: JSONEncoder().encode(synthetic))
        return NativeDocumentState(format: 1, revision: 7, workspaceId: FrameFitSynthetic.owner,
            tenantId: FrameFitSynthetic.owner, principalId: FrameFitSynthetic.owner, draft: draft,
            etag: "\"studio-r1-" + String(repeating: "a", count: 64) + "\"",
            savedEditVersion: draft.document["editVersion"].integer, pending: nil)
    }
    private func connect(_ state: NativeDocumentState, role: String = "admin", mode: String = "valid") async throws -> AuthoringAPI {
        try FrameFitAPIProtocol.configure(state, role: role, mode: mode)
        let api = try AuthoringAPI(port: 18091, tenantId: FrameFitSynthetic.owner,
            nativeRuntimeManifestSHA256: "a81d7ec85895c455dfeca5b67ecff5124eebb0dc43fbd703e7a9a34da164d98f",
            protocolClasses: [FrameFitAPIProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: FrameFitSynthetic.owner, principalId: FrameFitSynthetic.owner,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        return api
    }
    func testPreparationAndApplyRevalidationRederiveWithCASAndNoSave() async throws {
        let state = try fixture(), api = try await connect(state)
        let admitted = await api.admitsFrameImageFitReconciliation
        XCTAssertTrue(admitted)
        let preview = try await api.prepareFrameImageFitReconciliation(placementId: FrameFitSynthetic.placement, in: state)
        XCTAssertTrue(preview.changed)
        try await api.validateFrameImageFitReconciliation(preview, in: state)
        let calls = FrameFitAPIProtocol.calls(), proposals = calls.filter { $0.url!.path.hasSuffix("/native-authoring-proposal") }
        XCTAssertEqual(proposals.count, 2)
        XCTAssertEqual(calls.filter { $0.url!.path.hasSuffix("/native-viewport-check") }.count, 2)
        XCTAssertFalse(calls.contains { $0.httpMethod == "PUT" })
        for call in proposals {
            XCTAssertEqual(call.value(forHTTPHeaderField: "If-Match"), state.etag)
            let body = try XCTUnwrap(call.httpBody)
            let object = try XCTUnwrap(try JSONSerialization.jsonObject(with: body) as? [String: Any])
            XCTAssertEqual(object["expectedRevision"] as? Int, 1)
            let request = try XCTUnwrap(object["request"] as? [String: Any])
            XCTAssertEqual(Set(request.keys), Set(["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "type", "placementId"]))
            XCTAssertEqual(request["type"] as? String, "reconcile-frame-image-fit")
        }
    }
    func testUnavailableProfileAndViewerNeverPost() async throws {
        for (role, mode) in [("admin", "missing-profile"), ("admin", "unknown-profile"), ("viewer", "valid")] {
            let state = try fixture(), api = try await connect(state, role: role, mode: mode)
            do { _ = try await api.prepareFrameImageFitReconciliation(placementId: FrameFitSynthetic.placement, in: state); XCTFail("Expected refusal: \(role), \(mode)") } catch { }
            XCTAssertFalse(FrameFitAPIProtocol.calls().contains { $0.httpMethod == "POST" })
        }
        let state = try fixture(), api = try await connect(state)
        let foreign = NativeDocumentState(format: 1, revision: state.revision, workspaceId: state.workspaceId,
            tenantId: state.tenantId, principalId: FrameFitSynthetic.otherPlacement, draft: state.draft,
            etag: state.etag, savedEditVersion: state.savedEditVersion, pending: nil)
        do { _ = try await api.prepareFrameImageFitReconciliation(placementId: FrameFitSynthetic.placement, in: foreign); XCTFail("Foreign owner accepted") } catch { }
        XCTAssertFalse(FrameFitAPIProtocol.calls().contains { $0.httpMethod == "POST" })
    }
    func testExistingRecordIsNoopAndNoSave() async throws {
        let state = try fixture(fitted: true), api = try await connect(state)
        let preview = try await api.prepareFrameImageFitReconciliation(placementId: FrameFitSynthetic.placement, in: state)
        XCTAssertFalse(preview.changed)
        XCTAssertNil(try preview.apply(to: try XCTUnwrap(state.draft)))
        XCTAssertFalse(FrameFitAPIProtocol.calls().contains { $0.httpMethod == "PUT" })
    }
    func testStaleHeadRejectedReplyETagAndTamperingCannotApply() async throws {
        for mode in ["stale-head", "reject", "bad-etag", "tampered", "stale-primary", "bad-reply"] {
            let state = try fixture(), api = try await connect(state, mode: mode)
            do { _ = try await api.prepareFrameImageFitReconciliation(placementId: FrameFitSynthetic.placement, in: state); XCTFail("Expected refusal: \(mode)") } catch { }
            XCTAssertFalse(FrameFitAPIProtocol.calls().contains { $0.httpMethod == "PUT" }, mode)
        }
    }
    func testRevocationDuringViewportValidationIsNotAccepted() async throws {
        let state = try fixture(), api = try await connect(state, mode: "revoke-after-check")
        do { _ = try await api.prepareFrameImageFitReconciliation(placementId: FrameFitSynthetic.placement, in: state); XCTFail("Revoked live profile accepted") } catch { }
        XCTAssertTrue(FrameFitAPIProtocol.calls().contains { $0.url!.path.hasSuffix("/native-viewport-check") })
        XCTAssertFalse(FrameFitAPIProtocol.calls().contains { $0.httpMethod == "PUT" })
    }
    func testApplyRevalidationRejectsChangedServerProposal() async throws {
        let state = try fixture(), api = try await connect(state)
        let preview = try await api.prepareFrameImageFitReconciliation(placementId: FrameFitSynthetic.placement, in: state)
        FrameFitAPIProtocol.setMode("tampered")
        do { try await api.validateFrameImageFitReconciliation(preview, in: state); XCTFail("Changed proposal accepted") } catch { }
        XCTAssertEqual(FrameFitAPIProtocol.calls().filter { $0.url!.path.hasSuffix("/native-authoring-proposal") }.count, 2)
        XCTAssertFalse(FrameFitAPIProtocol.calls().contains { $0.httpMethod == "PUT" })
    }
}

private final class FrameFitAPIProtocol: URLProtocol, @unchecked Sendable {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var requests: [URLRequest] = []
    nonisolated(unsafe) private static var responses: [String: Data] = [:]
    nonisolated(unsafe) private static var mode = "valid"
    nonisolated(unsafe) private static var savedCandidate: DocumentValue = .null
    nonisolated(unsafe) private static var etag = ""
    static func configure(_ state: NativeDocumentState, role: String, mode: String) throws {
        let draft = try JSONSerialization.jsonObject(with: JSONEncoder().encode(state.draft!))
        let prefix = "/api/v1/tenants/\(state.tenantId)/studio/exhibitions/\(state.draft!.id)"
        var session: [String: Any] = ["tenantId": state.tenantId, "userId": state.principalId, "role": role,
            "csrfToken": "synthetic-csrf", "expiresAt": "2099-01-01T00:00:00Z",
            "workspaceFurnishingProfile": "workspace-furnishing-intake-v1", "workspaceFurnishingStoolProfile": "workspace-furnishing-stool-v1",
            "workspaceFurnishingRoomVisualProfile": "workspace-furnishing-room-visual-v1", "workspaceArtworkFrameProfile": "workspace-artwork-frame-v1",
            "nativeAuthoringProposalProfile": "saved-head-v1", "nativeFrameImageFitProposalProfile": FrameImageFitReconciliationRequest.profile]
        if mode == "missing-profile" { session.removeValue(forKey: "nativeFrameImageFitProposalProfile") }
        if mode == "unknown-profile" { session["nativeFrameImageFitProposalProfile"] = "unknown" }
        let initial = try JSONSerialization.data(withJSONObject: session)
        if mode == "revoke-after-check" { session.removeValue(forKey: "nativeFrameImageFitProposalProfile") }
        let saved: [String: Any] = ["revision": 1, "etag": mode == "stale-head" ? "\"studio-r2-" + String(repeating: "b", count: 64) + "\"" : state.etag!, "draft": draft]
        let data = try ["/api/v1/auth/session": initial, "/retained-session": JSONSerialization.data(withJSONObject: session),
                        prefix: JSONSerialization.data(withJSONObject: saved)]
        lock.lock(); defer { lock.unlock() }
        requests = []; responses = data; Self.mode = mode; savedCandidate = state.draft!.candidate; etag = state.etag!
    }
    static func calls() -> [URLRequest] { lock.lock(); defer { lock.unlock() }; return requests }
    static func setMode(_ value: String) { lock.lock(); defer { lock.unlock() }; mode = value }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        Self.lock.lock()
        let path = request.url!.path, mode = Self.mode
        let retained = path == "/api/v1/auth/session" && Self.requests.contains { $0.url?.path.hasSuffix("/native-viewport-check") == true }
        var body = request.httpBody
        if body == nil, let stream = request.httpBodyStream {
            stream.open(); defer { stream.close() }
            var bytes = Data(), buffer = [UInt8](repeating: 0, count: 4096)
            while true {
                let count = stream.read(&buffer, maxLength: buffer.count)
                if count <= 0 { break }
                bytes.append(contentsOf: buffer.prefix(count))
            }
            body = bytes
        }
        var recorded = request; recorded.httpBody = body
        Self.requests.append(recorded)
        var status = 200, headers = ["Content-Type": "application/json"], data: Data
        if path.hasSuffix("/native-authoring-proposal") {
            headers["ETag"] = mode == "bad-etag" ? "\"wrong\"" : Self.etag
            if mode == "reject" { status = 422; data = Data("{\"code\":\"BOUNDED_EDIT_UNSUPPORTED\"}".utf8) }
            else if let body, let parsed = try? JSONDecoder().decode(DocumentValue.self, from: body) {
                let input = parsed["request"]
                var candidate = Self.savedCandidate, rows = candidate["extensions"][FrameFitSynthetic.ns]["frameAttachments"].items!
                if rows[0]["imageFit"] == .null { rows[0]["imageFit"] = FrameFitSynthetic.fit }
                candidate["extensions"][FrameFitSynthetic.ns]["frameAttachments"] = .array(rows)
                candidate["extensions"][FrameFitSynthetic.ns]["version"] = .integer(10)
                if mode == "tampered" { candidate["title"] = .string("unrelated") }
                if mode == "stale-primary", var arts = candidate["artworks"].items {
                    arts[0]["primaryAssetId"] = .string(FrameFitSynthetic.otherPlacement); candidate["artworks"] = .array(arts)
                }
                var reply: [String: DocumentValue] = [:]
                for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId", "placementId"] { reply[key] = input[key] }
                reply["candidate"] = candidate
                if mode == "bad-reply" { reply["extra"] = .bool(true) }
                data = (try? JSONEncoder().encode(DocumentValue.object(reply))) ?? Data()
            } else { status = 500; data = Data() }
        } else if path.hasSuffix("/native-viewport-check") { data = Data("{\"valid\":true}".utf8) }
        else {
            data = Self.responses[retained ? "/retained-session" : path] ?? Data("{}".utf8)
            if let body = try? JSONSerialization.jsonObject(with: data) as? [String: Any], let tag = body["etag"] as? String { headers["ETag"] = tag }
        }
        Self.lock.unlock()
        let response = HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: nil, headerFields: headers)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
