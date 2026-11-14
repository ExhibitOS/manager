// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class RoomAudioAdmissionAPITests: XCTestCase {
    private let owner = "11111111-1111-4111-8111-111111111111"
    private func fixture() throws -> NativeDocumentState {
        let draft = try NativeExhibitionDraft.new(title: "Synthetic room audio")
        return NativeDocumentState(format: 1, revision: 7, workspaceId: owner, tenantId: owner,
            principalId: owner, draft: draft, etag: "\"studio-r1-" + String(repeating: "a", count: 64) + "\"",
            savedEditVersion: draft.document["editVersion"].integer, pending: nil)
    }
    private func connected(_ state: NativeDocumentState, mode: String = "valid") async throws -> AuthoringAPI {
        try RoomAudioAPIProtocol.configure(state, mode: mode)
        let api = try AuthoringAPI(port: 18091, tenantId: owner,
            nativeRuntimeManifestSHA256: "899c866846127b435794ecc4b07ea8e69328a2432450189ab31eb9b88a43cb59",
            protocolClasses: [RoomAudioAPIProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: owner, principalId: owner,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        return api
    }
    func testRetainedProvenanceAdmitsNoopAndTypedRoomEditWithoutWriting() async throws {
        for mode in ["valid", "ordinary"] {
            let state = try fixture(), api = try await connected(state, mode: mode)
            let room = try XCTUnwrap(state.draft?.rooms.first?["id"].string)
            let noop = try await api.prepareRoomAudio(RoomAudioSettings(), roomId: room, in: state)
            XCTAssertNil(noop)
            var settings = RoomAudioSettings(); settings.bgmOn = true
            let next = try await api.prepareRoomAudio(settings, roomId: room, in: state)
            XCTAssertEqual(try RoomAudioSettings.read(in: XCTUnwrap(next), roomId: room).bgmOn, true)
            XCTAssertTrue(RoomAudioAPIProtocol.calls().allSatisfy { $0.httpMethod == "GET" })
        }
    }
    func testUnknownRestrictedAndStalePolicyOrSavedHeadRefuseBeforeChange() async throws {
        for mode in ["unknown-profile", "unknown-required", "bounded-required", "read-only",
                     "capability", "stale-policy", "stale-saved", "revision-mismatch", "late-restriction"] {
            let state = try fixture(), api = try await connected(state, mode: mode)
            let room = try XCTUnwrap(state.draft?.rooms.first?["id"].string)
            var settings = RoomAudioSettings(); settings.bgmOn = true
            do { _ = try await api.prepareRoomAudio(settings, roomId: room, in: state); XCTFail(mode) }
            catch let error as AuthoringFailure {
                XCTAssertTrue(["ROOM_AUDIO_SAVE_REQUIRED", "EXHIBITION_COMPARISON_STALE"].contains(error.code), mode)
            }
            XCTAssertTrue(RoomAudioAPIProtocol.calls().allSatisfy { $0.httpMethod == "GET" }, mode)
        }
    }
    func testAudioIntakeStatusAndApprovalNeverReplayHistoricalReceipt() throws {
        let bytes = Data("exact request".utf8)
        for operation in ["room-audio-intake", "room-audio-status", "room-audio-approve"] {
            for state in ["uploading", "validated", "approved", "revoked"] {
                let prior: DocumentValue = .object(["audio": .object(["state": .string(state)])])
                XCTAssertNil(try NativeAutomationSession.replayResponse(for: operation,
                    prior: (bytes, prior), matching: bytes), "\(operation): \(state)")
            }
        }
        // Ordinary mutation replay retains its existing exact-request contract.
        let cached = DocumentValue.object(["changed": .bool(false)])
        XCTAssertEqual(try NativeAutomationSession.replayResponse(for: "room-audio-apply",
            prior: (bytes, cached), matching: bytes), cached)
        XCTAssertThrowsError(try NativeAutomationSession.replayResponse(for: "room-audio-apply",
            prior: (bytes, cached), matching: Data("changed request".utf8)))
    }
    private func audioFile() -> ImportFileSnapshot {
        ImportFileSnapshot(name: "synthetic.wav", mime: "audio/wav", sha256: "85759b3811ff7dc47b03792ac85317be51431a3f9e01dcafce317ed736a391b0", data: Data(repeating: 0, count: 44))
    }
    private var rights: DocumentValue {
        .object(["creditLine": .string("Synthetic WAV"), "permissions": .object(["display": .bool(true), "export": .bool(true)])])
    }
    func testIntakeValidatesBytesWithoutApprovalThenExplicitApprovalReadsExactSnapshot() async throws {
        let state = try fixture(), api = try await connected(state)
        let record = try await api.intakeRoomAudio(audioFile(), requestId: "33333333-3333-4333-8333-333333333333", rights: rights, in: state)
        XCTAssertEqual(record.state, "validated")
        let mutations = RoomAudioAPIProtocol.calls().filter { $0.httpMethod != "GET" }
        XCTAssertEqual(mutations.map { $0.httpMethod! }, ["POST", "PUT"])
        XCTAssertFalse(mutations.contains { $0.url!.path.hasSuffix("/approve") })
        XCTAssertTrue(mutations.allSatisfy { $0.value(forHTTPHeaderField: "x-csrf-token") == "synthetic-csrf" })
        XCTAssertEqual(mutations.last?.value(forHTTPHeaderField: "Content-Type"), "application/octet-stream")
        let approved = try await api.approveRoomAudio(record.id, expectedMedia: record.mediaAsset, in: state)
        XCTAssertEqual(approved.state, "approved")
        XCTAssertEqual(approved.mediaAsset, record.mediaAsset)
        XCTAssertEqual(RoomAudioAPIProtocol.calls().filter { $0.url!.path.hasSuffix("/approve") }.count, 1)
    }
    func testRightsAndReviewedMediaMismatchRefuseWithoutMutation() async throws {
        let state = try fixture(), api = try await connected(state)
        var denied = rights; denied["permissions"]["export"] = .bool(false)
        do { _ = try await api.intakeRoomAudio(audioFile(), requestId: UUID().uuidString.lowercased(), rights: denied, in: state); XCTFail() }
        catch let error as AuthoringFailure { XCTAssertEqual(error.code, "ROOM_AUDIO_TRACK_UNAVAILABLE") }
        let id = "44444444-4444-4444-8444-444444444444"
        let record = try await api.roomAudioStatus(id, in: state)
        var changed = record.mediaAsset; changed["sha256"] = .string(String(repeating: "b", count: 64))
        do { _ = try await api.approveRoomAudio(id, expectedMedia: changed, in: state); XCTFail() }
        catch let error as AuthoringFailure { XCTAssertEqual(error.code, "ROOM_AUDIO_RESPONSE_INVALID") }
        XCTAssertTrue(RoomAudioAPIProtocol.calls().allSatisfy { $0.httpMethod == "GET" })
    }
    func testServerUploadRefusalPropagatesWithoutApprovalOrSceneMutation() async throws {
        let state = try fixture(), api = try await connected(state, mode: "upload-reject")
        do { _ = try await api.intakeRoomAudio(audioFile(), requestId: UUID().uuidString.lowercased(), rights: rights, in: state); XCTFail() }
        catch let error as AuthoringFailure { XCTAssertEqual(error.code, "AUDIO_INVALID") }
        XCTAssertFalse(RoomAudioAPIProtocol.calls().contains { $0.url!.path.hasSuffix("/approve") })
    }
    func testForeignAndDirtyDocumentsRefuseBeforePolicyLookup() async throws {
        for mode in ["tenant", "principal", "dirty"] {
            let original = try fixture(), api = try await connected(original)
            var state = original
            if mode == "dirty" { state.savedEditVersion = -1 }
            if mode != "dirty" {
                state = NativeDocumentState(format: original.format, revision: original.revision,
                    workspaceId: original.workspaceId,
                    tenantId: mode == "tenant" ? "22222222-2222-4222-8222-222222222222" : original.tenantId,
                    principalId: mode == "principal" ? "22222222-2222-4222-8222-222222222222" : original.principalId,
                    draft: original.draft, etag: original.etag,
                    savedEditVersion: original.savedEditVersion, pending: nil)
            }
            do { _ = try await api.roomAudioTracks(in: state); XCTFail(mode) }
            catch let error as AuthoringFailure { XCTAssertEqual(error.code, "ROOM_AUDIO_SAVE_REQUIRED") }
            XCTAssertEqual(RoomAudioAPIProtocol.calls().count, 1, mode)
        }
    }
}

private final class RoomAudioAPIProtocol: URLProtocol, @unchecked Sendable {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var requests: [URLRequest] = []
    nonisolated(unsafe) private static var responses: [String: Data] = [:]
    nonisolated(unsafe) private static var mode = "valid"
    nonisolated(unsafe) private static var audioState = "validated"
    static func configure(_ state: NativeDocumentState, mode: String) throws {
        let draft = try JSONSerialization.jsonObject(with: JSONEncoder().encode(state.draft!))
        let id = state.draft!.id, prefix = "/api/v1/tenants/\(state.tenantId)/studio/exhibitions/\(id)"
        let session: [String: Any] = ["tenantId": state.tenantId, "userId": state.principalId, "role": "admin",
            "csrfToken": "synthetic-csrf", "expiresAt": "2099-01-01T00:00:00Z"]
        var policy: [String: Any] = ["exhibitionId": id, "revision": 1, "etag": state.etag!,
            "boundedReadOnly": mode == "read-only", "boundedProfile": mode != "ordinary",
            "boundedEditRequired": mode == "bounded-required"]
        if mode == "unknown-profile" { policy.removeValue(forKey: "boundedProfile") }
        if mode == "unknown-required" { policy.removeValue(forKey: "boundedEditRequired") }
        if mode == "stale-policy" { policy["etag"] = "stale" }
        if mode == "capability" { policy["editingCapability"] = ["version": 1, "kind": "native-bounded-edit-v1",
            "tenantId": state.tenantId, "principalId": state.principalId, "workspaceId": state.workspaceId,
            "exhibitionId": id, "revision": 1, "etag": state.etag!] }
        let savedRevision = mode == "revision-mismatch" ? 2 : 1
        let savedETag = "\"studio-r\(savedRevision)-" + String(repeating: mode == "stale-saved" ? "b" : "a", count: 64) + "\""
        let saved: [String: Any] = ["revision": savedRevision, "etag": savedETag, "draft": draft]
        let values = try ["/api/v1/auth/session": JSONSerialization.data(withJSONObject: session),
            prefix: JSONSerialization.data(withJSONObject: saved),
            prefix + "/native-viewport-policy": JSONSerialization.data(withJSONObject: policy)]
        lock.lock(); defer { lock.unlock() }; requests = []; responses = values; Self.mode = mode; audioState = "validated"
    }
    static func calls() -> [URLRequest] { lock.lock(); defer { lock.unlock() }; return requests }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        Self.lock.lock()
        let path = request.url!.path
        let late = Self.mode == "late-restriction" && path.hasSuffix("/native-viewport-policy")
            && Self.requests.contains { $0.url?.path == path }
        Self.requests.append(request)
        var data = Self.responses[path] ?? Data("{}".utf8)
        if late, var policy = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            policy["boundedEditRequired"] = true
            data = try! JSONSerialization.data(withJSONObject: policy)
        }
        let audioId = "44444444-4444-4444-8444-444444444444"
        let isAudio = path.contains("/audio")
        let reject = isAudio && path.hasSuffix("/bytes") && Self.mode == "upload-reject"
        if isAudio {
            if request.httpMethod == "POST" { Self.audioState = path.hasSuffix("/approve") ? "approved" : "uploading" }
            if request.httpMethod == "PUT" && !reject { Self.audioState = "validated" }
            let media: [String: Any] = ["id": audioId, "path": "media/\(audioId)/audio.wav", "mime": "audio/wav",
                "bytes": 44, "sha256": "85759b3811ff7dc47b03792ac85317be51431a3f9e01dcafce317ed736a391b0",
                "rights": ["creditLine": "Synthetic WAV", "permissions": ["display": true, "export": true]]]
            let audio: [String: Any] = ["id": audioId, "state": Self.audioState, "revision": 1, "revoked": false,
                "durationSeconds": Self.audioState == "uploading" ? (NSNull() as Any) : (1 as Any), "mediaAsset": media]
            let payload: [String: Any]
            if reject { payload = ["code": "AUDIO_INVALID"] } else { payload = audio }
            data = try! JSONSerialization.data(withJSONObject: payload)
        }
        Self.lock.unlock()
        var headers = ["Content-Type": "application/json"]
        if let body = try? JSONSerialization.jsonObject(with: data) as? [String: Any], let etag = body["etag"] as? String {
            headers["ETag"] = etag
        }
        let response = HTTPURLResponse(url: request.url!, statusCode: reject ? 422 : 200, httpVersion: nil,
            headerFields: headers)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
