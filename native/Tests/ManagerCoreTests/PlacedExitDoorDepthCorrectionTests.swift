// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

@MainActor
final class PlacedExitDoorDepthCorrectionTests: XCTestCase {
    private func value<T: Encodable>(_ item: T) throws -> DocumentValue { try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(item)) }
    private func fixture() throws -> (NativeExhibitionDraft, DocumentValue, WorkspaceFurnishing, WorkspaceFurnishing) {
        var draft = try NativeExhibitionDraft.new()
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let wall = UUID().uuidString.lowercased(), doorID = UUID().uuidString.lowercased(), signID = UUID().uuidString.lowercased()
        func furnishing(kind: String, hash: String, bytes: Int64) throws -> WorkspaceFurnishing {
            let row: DocumentValue = .object(["id": .string(UUID().uuidString.lowercased()), "version": .integer(1), "kind": .string(kind),
                "name": .string("Synthetic fixture"), "provenance": .string("Synthetic test fixture"), "rights": try value(ArtworkMetadata.Rights()),
                "model": .object(["bytes": .integer(bytes), "mime": .string("model/gltf-binary"), "sha256": .string(hash)])])
            return try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(row))
        }
        let door = try furnishing(kind: "barrier", hash: PlacedExitDoorSettings.modelSHA256, bytes: 8684)
        let sign = try furnishing(kind: "info-stand", hash: PlacedExitDoorDepthCorrection.signModelSHA256, bytes: 10596)
        func placement(id: String, furnishing: String, height: Double) -> WorkspaceFurnishingPlacement {
            .init(id: id, roomId: room, furnishingId: furnishing, furnishingVersion: 1, position: [-1.05, height, 1.985], yaw: .pi, scale: 1)
        }
        let oldDoor = placement(id: doorID, furnishing: door.id, height: 0), oldSign = placement(id: signID, furnishing: sign.id, height: 2.33)
        var candidate = draft.candidate
        candidate["surfaces"] = .array([.object(["id": .string(wall), "roomId": .string(room), "type": .string("wall"),
            "dimensions": .object(["width": .integer(4), "height": .integer(4)]),
            "transform": .object(["position": .vector([0, 2, 1.99]), "scale": .vector([1, 1, 1]), "rotation": .vector([0, 1, 0, 0])])])])
        if candidate["extensions"] == .null { candidate["extensions"] = .object([:]) }
        candidate["extensions"][WorkspaceFurnishingPlacement.namespace] = .object(["version": .integer(11), "furnishings": .array([try value(door), try value(sign)]),
            "placements": .array([try value(oldDoor), try value(oldSign)]), "frameAttachments": .array([]), "openingAttachments": .array([]), "ceilingAttachments": .array([])])
        try draft.replaceCandidate(candidate)
        let settings = try PlacedExitDoorSettings(placementId: doorID, surfaceId: wall,
            primary: PlacedExitDoorSettings.Destination(name: "Fixture", url: "https://example.org/"), additional: [])
        try draft.replaceCandidate(settings.candidate(in: draft))
        let command: DocumentValue = .object(["door": try value(oldDoor), "sign": try value(oldSign), "depthOffset": .number(0.06)])
        return (draft, command, door, sign)
    }
    func testOnlyTwoPositionVectorsChangeAndOldRowsCannotReplay() throws {
        let (source, command, door, sign) = try fixture()
        let correction = try PlacedExitDoorDepthCorrection(value: command)
        let changed = try correction.applying(to: source, registeredDoor: door, registeredSign: sign)
        let ns = WorkspaceFurnishingPlacement.namespace
        var restored = changed.candidate
        let originalRows = try XCTUnwrap(source.candidate["extensions"][ns]["placements"].items)
        var rows = try XCTUnwrap(restored["extensions"][ns]["placements"].items)
        XCTAssertEqual(rows.count, 2)
        for index in rows.indices {
            XCTAssertEqual(rows[index]["position"].items?.last?.double ?? 0, 1.925, accuracy: 1e-12)
            rows[index]["position"] = originalRows[index]["position"]
        }
        restored["extensions"][ns]["placements"] = .array(rows)
        XCTAssertEqual(restored, source.candidate)
        XCTAssertThrowsError(try correction.applying(to: changed, registeredDoor: door, registeredSign: sign))
    }
    func testStaleIdentityModelAxisAndAnchorChangesFailClosed() throws {
        let (source, command, door, sign) = try fixture()
        var invalid: [DocumentValue] = []
        for offset in [0.0, -0.06, 0.081] { var row = command; row["depthOffset"] = .number(offset); invalid.append(row) }
        var extra = command; extra["candidate"] = source.candidate; invalid.append(extra)
        var stale = command; stale["door"]["position"] = .vector([-1.05, 0, 1.984]); invalid.append(stale)
        var yaw = command; yaw["sign"]["yaw"] = .integer(0); invalid.append(yaw)
        var wrong = command; wrong["sign"]["furnishingId"] = .string(door.id); invalid.append(wrong)
        var duplicate = command; duplicate["sign"]["id"] = command["door"]["id"]; invalid.append(duplicate)
        var beyond = command; beyond["depthOffset"] = .number(0.08); invalid.append(beyond)
        for input in invalid {
            XCTAssertThrowsError(try PlacedExitDoorDepthCorrection(value: input).applying(to: source, registeredDoor: door, registeredSign: sign))
        }
        XCTAssertThrowsError(try PlacedExitDoorDepthCorrection(value: command).applying(to: source, registeredDoor: sign, registeredSign: door))
        var modified = source.candidate
        modified["extensions"][WorkspaceFurnishingPlacement.namespace]["placements"] = .array([])
        var missing = source; try missing.replaceCandidate(modified)
        XCTAssertThrowsError(try PlacedExitDoorDepthCorrection(value: command).applying(to: missing, registeredDoor: door, registeredSign: sign))
    }
    func testPreparationUsesSavedOwnerRegistryAndViewportCASBeforeReturningAnyEdit() async throws {
        let (draft, command, door, sign) = try fixture()
        let owner = "11111111-1111-4111-8111-111111111111"
        let state = NativeDocumentState(format: 1, revision: 7, workspaceId: owner, tenantId: owner,
            principalId: owner, draft: draft, etag: "\"studio-r1-" + String(repeating: "a", count: 64) + "\"",
            savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        for mode in ["valid", "stale-policy", "stale-saved", "read-only", "late-policy", "model", "rights", "revoked", "capability", "server-refusal", "foreign", "dirty"] {
            try ExitDepthProtocol.configure(state, door: door, sign: sign, mode: mode)
            let api = try AuthoringAPI(port: 18091, tenantId: owner,
                nativeRuntimeManifestSHA256: "9cc5b33143c6fc887e405f96952a8a96c4b17cde573ebf77bc77b8f72c55651b", protocolClasses: [ExitDepthProtocol.self])
            _ = try await api.connect(LocalAuthoringSession(tenantId: owner, principalId: owner,
                origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
            var input = state
            if mode == "dirty" { input.savedEditVersion = -1 }
            if mode == "foreign" {
                input = NativeDocumentState(format: 1, revision: state.revision, workspaceId: owner,
                    tenantId: owner, principalId: "22222222-2222-4222-8222-222222222222", draft: draft,
                    etag: state.etag, savedEditVersion: state.savedEditVersion, pending: nil)
            }
            do {
                let result = try await api.preparePlacedExitDoorDepth(PlacedExitDoorDepthCorrection(value: command), in: input)
                XCTAssertEqual(mode, "valid")
                XCTAssertNotEqual(result.candidate, draft.candidate)
            } catch { XCTAssertNotEqual(mode, "valid", "\(error)") }
            let calls = ExitDepthProtocol.calls()
            XCTAssertTrue(calls.allSatisfy { $0.httpMethod == "GET" || $0.url?.path.hasSuffix("/native-viewport-check") == true })
            for check in calls.filter({ $0.httpMethod == "POST" }) {
                XCTAssertEqual(check.value(forHTTPHeaderField: "If-Match"), state.etag)
                XCTAssertEqual(check.value(forHTTPHeaderField: "x-csrf-token"), "fixture")
            }
            if mode == "valid" { XCTAssertEqual(calls.filter { $0.httpMethod == "POST" }.count, 1) }
            if ["foreign", "dirty"].contains(mode) { XCTAssertEqual(calls.count, 1) }
        }
    }

}

private final class ExitDepthProtocol: URLProtocol, @unchecked Sendable {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var requests: [URLRequest] = []
    nonisolated(unsafe) private static var responses: [String: Data] = [:]
    nonisolated(unsafe) private static var mode = "valid"
    static func configure(_ state: NativeDocumentState, door: WorkspaceFurnishing, sign: WorkspaceFurnishing, mode: String) throws {
        let draft = try JSONSerialization.jsonObject(with: JSONEncoder().encode(state.draft!))
        let prefix = "/api/v1/tenants/\(state.tenantId)/studio/exhibitions/\(state.draft!.id)"
        let session: [String: Any] = ["tenantId": state.tenantId, "userId": state.principalId, "role": "admin", "csrfToken": "fixture",
            "expiresAt": "2099-01-01T00:00:00Z", "workspaceFurnishingProfile": "workspace-furnishing-intake-v1"]
        let policy: [String: Any] = ["exhibitionId": state.draft!.id, "revision": 1, "etag": mode == "stale-policy" ? "stale" : state.etag!,
            "boundedReadOnly": mode == "read-only", "boundedProfile": true, "boundedEditRequired": false]
        let saved: [String: Any] = ["revision": 1, "etag": mode == "stale-saved" ? "stale" : state.etag!, "draft": draft]
        var registered = try JSONSerialization.jsonObject(with: JSONEncoder().encode(sign)) as! [String: Any]
        if mode == "model", var model = registered["model"] as? [String: Any] { model["sha256"] = String(repeating: "b", count: 64); registered["model"] = model }
        if mode == "rights", var rights = registered["rights"] as? [String: Any], var permissions = rights["permissions"] as? [String: Any] {
            permissions["display"] = false; rights["permissions"] = permissions; registered["rights"] = rights
        }
        let values = try ["/api/v1/auth/session": JSONSerialization.data(withJSONObject: session), prefix: JSONSerialization.data(withJSONObject: saved),
            prefix + "/native-viewport-policy": JSONSerialization.data(withJSONObject: policy),
            prefix + "/native-viewport-check": JSONSerialization.data(withJSONObject: ["valid": true]),
            "/api/v1/tenants/\(state.tenantId)/workspace/furnishings/\(door.id)/versions/1": JSONEncoder().encode(door),
            "/api/v1/tenants/\(state.tenantId)/workspace/furnishings/\(sign.id)/versions/1": JSONSerialization.data(withJSONObject: registered)]
        lock.lock(); defer { lock.unlock() }; responses = values; requests = []; Self.mode = mode
    }
    static func calls() -> [URLRequest] { lock.lock(); defer { lock.unlock() }; return requests }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        Self.lock.lock()
        let path = request.url!.path
        let count = Self.requests.filter { $0.url?.path == path }.count
        var data = Self.responses[path] ?? Data("{}".utf8)
        var status = 200
        if path == "/api/v1/auth/session", count > 0, Self.mode == "revoked" { status = 401; data = Data("{\"code\":\"AUTH_REQUIRED\"}".utf8) }
        if path == "/api/v1/auth/session", count > 1, Self.mode == "capability", var session = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            session["workspaceCeilingRailProfile"] = "changed"; data = try! JSONSerialization.data(withJSONObject: session)
        }
        if path.hasSuffix("/native-viewport-policy"), count > 0, Self.mode == "late-policy", var policy = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            policy["boundedReadOnly"] = true; data = try! JSONSerialization.data(withJSONObject: policy)
        }
        if path.hasSuffix("/native-viewport-check"), Self.mode == "server-refusal" { status = 422; data = Data("{\"code\":\"DRAFT_INVALID\"}".utf8) }
        Self.requests.append(request); Self.lock.unlock()
        var headers = ["Content-Type": "application/json"]
        if let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any], let etag = object["etag"] as? String { headers["ETag"] = etag }
        client?.urlProtocol(self, didReceive: HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: nil, headerFields: headers)!, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
