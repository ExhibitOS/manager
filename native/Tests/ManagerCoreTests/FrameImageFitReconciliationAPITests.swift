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
    private func connect(_ state: NativeDocumentState, role: String = "admin", mode: String = "valid",
                         runtime: String = "a81d7ec85895c455dfeca5b67ecff5124eebb0dc43fbd703e7a9a34da164d98f") async throws -> AuthoringAPI {
        try FrameFitAPIProtocol.configure(state, role: role, mode: mode)
        let api = try AuthoringAPI(port: 18091, tenantId: FrameFitSynthetic.owner, nativeRuntimeManifestSHA256: runtime,
            protocolClasses: [FrameFitAPIProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: FrameFitSynthetic.owner, principalId: FrameFitSynthetic.owner,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        return api
    }
    #if DEBUG && os(macOS)
    func testOccupiedWheatUsesOnlyItsConditionalOrdinaryProposalAndChecksLateSession() async throws {
        let runtime = "3299659f0be756424c2abfa80f866cb54e4976c75f398d19f9d1de33481c4912"
        let target = "89ec602d-79ff-4851-9ead-85bdd4a382cf"
        let model = "a96b4942c2cd10b25fe1b33001443e1824fc125bc258f28a68547d38d8c54362"
        let image = "cde67c823c0fed9574d94d4a95facbcc7e3b7782af8424d55fb41c8a53c250b7"
        let ids = [
            FrameFitSynthetic.placement: target, FrameFitSynthetic.artwork: "53d66af3-0ff2-4dd9-8cc7-9056a6ddf8b9",
            FrameFitSynthetic.asset: "000266b5-34b5-44c2-a794-1fe8c344f949", FrameFitSynthetic.frameId: "4ddff48b-c2b2-5166-9e35-cd1c7aabb5b3",
            FrameFitSynthetic.model: model, FrameFitSynthetic.image: image
        ]
        func replacingBindings(_ value: DocumentValue) -> DocumentValue {
            switch value {
            case .object(let fields): return .object(fields.mapValues(replacingBindings))
            case .array(let rows): return .array(rows.map(replacingBindings))
            case .string(let value): return .string(ids[value] ?? value)
            default: return value
            }
        }
        var source = replacingBindings(try FrameFitSynthetic.draft(version: 11, fitted: false).document)
        source["editVersion"] = .integer(281)
        source["candidate"]["extensions"][FrameFitSynthetic.ns]["ceilingAttachments"] = .array([])
        let draft = try NativeExhibitionDraft(document: source)
        let state = NativeDocumentState(format: 1, revision: 618, workspaceId: "23b8c20f-2c3e-4c77-aa0c-6e4c6df015ef",
            tenantId: FrameFitSynthetic.owner, principalId: FrameFitSynthetic.owner, draft: draft,
            etag: #""studio-r218-ea370957db37cfb1016cb296eb3843d2585de0cb4bb42b885f91ffdabcfa074e""#,
            savedEditVersion: 281, pending: nil)
        let api = try await connect(state, runtime: runtime)
        let broad = await api.admitsWorkspaceFurnishings
        XCTAssertFalse(broad)
        let preview = try await api.prepareFrameImageFitReconciliation(placementId: target, in: state)
        var expected = draft.candidate
        var rows = try XCTUnwrap(expected["extensions"][FrameFitSynthetic.ns]["frameAttachments"].items)
        rows[0]["imageFit"] = .object(["version": .integer(1), "profile": .string("contain-contact-v1"),
            "sourceSha256": .string(image), "pixelWidth": .integer(2048), "pixelHeight": .integer(1630)])
        expected["extensions"][FrameFitSynthetic.ns]["frameAttachments"] = .array(rows)
        XCTAssertEqual(try preview.apply(to: draft)?.candidate, expected)
        let calls = FrameFitAPIProtocol.calls()
        XCTAssertEqual(calls.suffix(6).compactMap { $0.url?.lastPathComponent },
                       ["session", draft.id, "native-viewport-policy", "native-authoring-proposal", "native-viewport-check", "session"])
        for call in calls where call.httpMethod == "POST" {
            XCTAssertEqual(call.value(forHTTPHeaderField: "If-Match"), state.etag)
        }
        for (mode, reachesProposal, reachesCheck) in [
            ("stale-head", false, false), ("bounded", false, false), ("reject-check", true, true), ("revoke-after-check", true, true)
        ] {
            let refused = try await connect(state, mode: mode, runtime: runtime)
            do {
                _ = try await refused.prepareFrameImageFitReconciliation(placementId: target, in: state)
                XCTFail("Expected occupied frame fit refusal: \(mode)")
            } catch {}
            let observed = FrameFitAPIProtocol.calls()
            XCTAssertEqual(observed.contains { $0.url?.lastPathComponent == "native-authoring-proposal" }, reachesProposal, mode)
            XCTAssertEqual(observed.contains { $0.url?.lastPathComponent == "native-viewport-check" }, reachesCheck, mode)
            XCTAssertFalse(observed.contains { $0.httpMethod == "PUT" }, mode)
        }
        let mixed = try await connect(state, runtime: "c64537c2f33dd731270a0ec14d9466af459227bf0603af155ee31bd35e5196a0")
        do {
            _ = try await mixed.prepareFrameImageFitReconciliation(placementId: target, in: state)
            XCTFail("Mixed runtime accepted")
        } catch {}
        XCTAssertFalse(FrameFitAPIProtocol.calls().contains { $0.httpMethod == "POST" })
    }
    #endif
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
        let revision = state.savedEditVersion == 281 ? 218 : 1
        let saved: [String: Any] = ["revision": revision, "etag": mode == "stale-head" ? "\"studio-r2-" + String(repeating: "b", count: 64) + "\"" : state.etag!, "draft": draft]
        let policy: [String: Any] = ["exhibitionId": state.draft!.id, "revision": revision, "etag": state.etag!,
            "boundedReadOnly": false, "boundedProfile": mode == "bounded", "boundedEditRequired": false]
        let data = try ["/api/v1/auth/session": initial, "/retained-session": JSONSerialization.data(withJSONObject: session),
                        prefix: JSONSerialization.data(withJSONObject: saved),
                        prefix + "/native-viewport-policy": JSONSerialization.data(withJSONObject: policy)]
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
                let occupied = candidate["extensions"][FrameFitSynthetic.ns]["version"] == .integer(11)
                if rows[0]["imageFit"] == .null {
                    var fit = FrameFitSynthetic.fit
                    if occupied {
                        fit["sourceSha256"] = candidate["artworks"].items![0]["assets"].items![0]["sha256"]
                        fit["pixelWidth"] = .integer(2048); fit["pixelHeight"] = .integer(1630)
                    }
                    rows[0]["imageFit"] = fit
                }
                candidate["extensions"][FrameFitSynthetic.ns]["frameAttachments"] = .array(rows)
                if !occupied { candidate["extensions"][FrameFitSynthetic.ns]["version"] = .integer(10) }
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
        } else if path.hasSuffix("/native-viewport-check") {
            data = Data((mode == "reject-check" ? "{\"valid\":false}" : "{\"valid\":true}").utf8)
        }
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
