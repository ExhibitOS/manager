// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class WorkspaceBenchVisualReplacementTests: XCTestCase {
    private let bench = "11111111-1111-4111-8111-111111111111"
    private let placement = "22222222-2222-4222-8222-222222222222"
    private let oldId = "33333333-3333-4333-8333-333333333333"
    private let newId = "44444444-4444-4444-8444-444444444444"
    private let oldHash = String(repeating: "a", count: 64), newHash = nativeImportSHA256(Data(repeating: 0, count: 44))
    func testExactDisabledPreviewSuccessorRetainsRuntimeScope() {
        let manifest = "9d122f7fc6718601c5f7dea1b332dc744c604b6f71c11b32cf6f384c2d87cbc0"
        #if DEBUG
        XCTAssertTrue(AuthoringAPI.supportsCombinedThumbnailRuntime(manifest: manifest))
        XCTAssertTrue(AuthoringAPI.supportsSavedHeadAuthoringRuntime(manifest: manifest))
        #else
        XCTAssertFalse(AuthoringAPI.supportsCombinedThumbnailRuntime(manifest: manifest))
        XCTAssertFalse(AuthoringAPI.supportsSavedHeadAuthoringRuntime(manifest: manifest))
        #endif
        XCTAssertFalse(AuthoringAPI.supportsSavedHeadAuthoringRuntime(manifest: String(repeating: "f", count: 64)))
        XCTAssertFalse(AuthoringAPI.supportsSavedHeadAuthoringRuntime(manifest: manifest.uppercased()))
    }
    private func descriptor(_ id: String, hash: String, kind: String = "bench") -> DocumentValue {
        .object(["id": .string(id), "version": .integer(1), "kind": .string(kind), "name": .string("Synthetic bench"),
            "provenance": .string("Synthetic test fixture"), "model": .object(["mime": .string("model/gltf-binary"), "bytes": .integer(44), "sha256": .string(hash)]),
            "rights": .object(["holder": .string("Fixture"), "ownership": .string("owner"), "licenseId": .string("CC0-1.0"),
                "creditLine": .string("Fixture"), "permissions": .object(["display": .bool(true), "export": .bool(true), "download": .bool(false), "commercial": .bool(false)])])])
    }
    private func replacement(expected: String? = nil) throws -> WorkspaceBenchVisualReplacement {
        try .init(benchId: bench, placementId: placement, expectedModelSHA256: expected ?? oldHash,
            furnishingId: newId, modelSHA256: newHash)
    }
    private func fixture(shared: Bool = false) throws -> NativeExhibitionDraft {
        var draft = try NativeExhibitionDraft.new(), candidate = draft.candidate
        let room = try XCTUnwrap(draft.rooms.first?["id"].string)
        let seat = try BenchSettings(id: bench, name: "Backed seat", roomId: room, kind: .backed,
            position: [0,0,2], yaw: 0, width: 1.5, depth: 0.7, height: 0.45, material: .defaults(.wood),
            viewpointId: "55555555-5555-4555-8555-555555555555",
            workspaceVisual: .init(placementId: placement, modelSha256: oldHash, modelForward: .positiveZ))
        var rows: [DocumentValue] = [.object(["id": .string(placement), "roomId": .string(room),
            "furnishingId": .string(oldId), "furnishingVersion": .integer(1), "position": .vector([0,0,2]), "yaw": .number(0), "scale": .number(1)])]
        if shared { var other = rows[0]; other["id"] = .string("66666666-6666-4666-8666-666666666666"); rows.append(other) }
        candidate["extensions"] = .object([
            BenchSettings.namespace: .object(["version": .integer(1), "benches": .array([seat.value])]),
            WorkspaceFurnishingPlacement.namespace: .object(["version": .integer(1), "furnishings": .array([descriptor(oldId, hash: oldHash)]), "placements": .array(rows)]),
            "fixture/collision-and-viewpoint": .object(["unchanged": .bool(true)])])
        try draft.replaceCandidate(candidate); return draft
    }
    private func newFurnishing(kind: String = "bench") throws -> WorkspaceFurnishing {
        try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(descriptor(newId, hash: newHash, kind: kind)))
    }
    func testReplacementPreservesHostAndPoseWhileChangingOnlyLinkedVisual() throws {
        let before = try fixture(), next = try replacement().applying(newFurnishing(), to: before, supportedExtensionVersion: 11)
        var expected = before.candidate
        let workspace = WorkspaceFurnishingPlacement.namespace
        expected["extensions"][workspace]["furnishings"] = .array([descriptor(newId, hash: newHash)])
        var rows = try XCTUnwrap(expected["extensions"][workspace]["placements"].items)
        rows[0]["furnishingId"] = .string(newId); expected["extensions"][workspace]["placements"] = .array(rows)
        var seats = try XCTUnwrap(expected["extensions"][BenchSettings.namespace]["benches"].items)
        seats[0]["workspaceVisual"]["modelSha256"] = .string(newHash)
        expected["extensions"][BenchSettings.namespace]["benches"] = .array(seats)
        XCTAssertEqual(next.candidate, expected)
        XCTAssertEqual(try BenchSettings.list(in: next)[0].workspaceVisual?.modelForward, .positiveZ)
    }
    func testSharedOldDescriptorIsRetainedAndOtherPlacementExact() throws {
        let before = try fixture(shared: true), next = try replacement().applying(newFurnishing(), to: before, supportedExtensionVersion: 11)
        let workspace = WorkspaceFurnishingPlacement.namespace
        XCTAssertEqual(next.candidate["extensions"][workspace]["furnishings"].items, [descriptor(oldId, hash: oldHash), descriptor(newId, hash: newHash)])
        XCTAssertEqual(next.candidate["extensions"][workspace]["placements"].items?[1], before.candidate["extensions"][workspace]["placements"].items?[1])
    }
    func testWrongSourceKindDuplicateLinkAndPoseMismatchRefuse() throws {
        let before = try fixture()
        XCTAssertThrowsError(try replacement(expected: String(repeating: "c", count: 64)).applying(newFurnishing(), to: before, supportedExtensionVersion: 11))
        XCTAssertThrowsError(try replacement().applying(newFurnishing(kind: "display-case"), to: before, supportedExtensionVersion: 11))
        var candidate = before.candidate, rows = try XCTUnwrap(candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["placements"].items)
        rows[0]["scale"] = .number(1.1); candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["placements"] = .array(rows)
        var changed = before; try changed.replaceCandidate(candidate)
        XCTAssertThrowsError(try replacement().applying(newFurnishing(), to: changed, supportedExtensionVersion: 11))
        rows.append(rows[0]); candidate["extensions"][WorkspaceFurnishingPlacement.namespace]["placements"] = .array(rows)
        try changed.replaceCandidate(candidate)
        XCTAssertThrowsError(try replacement().applying(newFurnishing(), to: changed, supportedExtensionVersion: 11))
    }
    private func connected(_ state: NativeDocumentState, furnishing: WorkspaceFurnishing, mode: String = "valid", role: String = "admin") async throws -> AuthoringAPI {
        try BenchReplacementAPIProtocol.configure(state, furnishing: furnishing, mode: mode, role: role)
        let api = try AuthoringAPI(port: 18091, tenantId: state.tenantId,
            nativeRuntimeManifestSHA256: "899c866846127b435794ecc4b07ea8e69328a2432450189ab31eb9b88a43cb59",
            protocolClasses: [BenchReplacementAPIProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: state.tenantId, principalId: state.principalId,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        return api
    }
    private func state() throws -> NativeDocumentState {
        let draft = try fixture(), owner = "77777777-7777-4777-8777-777777777777"
        return NativeDocumentState(format: 1, revision: 7, workspaceId: owner, tenantId: owner, principalId: owner,
            draft: draft, etag: "\"studio-r1-" + String(repeating: "a", count: 64) + "\"",
            savedEditVersion: draft.document["editVersion"].integer, pending: nil)
    }
    func testCurrentRegisteredModelChecksServerCandidateWithoutSave() async throws {
        let state = try state(), furnishing = try newFurnishing(), api = try await connected(state, furnishing: furnishing)
        let next = try await api.prepareWorkspaceBenchVisualReplacement(replacement(), intakeId: BenchReplacementAPIProtocol.intakeId, in: state)
        XCTAssertEqual(next.candidate, try replacement().applying(furnishing, to: XCTUnwrap(state.draft), supportedExtensionVersion: 11).candidate)
        let mutations = BenchReplacementAPIProtocol.calls().filter { $0.httpMethod != "GET" }
        XCTAssertEqual(mutations.count, 1)
        XCTAssertTrue(mutations[0].url!.path.hasSuffix("/native-viewport-check"))
        XCTAssertEqual(mutations[0].value(forHTTPHeaderField: "If-Match"), state.etag)
        XCTAssertEqual(mutations[0].value(forHTTPHeaderField: "x-csrf-token"), "synthetic-csrf")
    }
    func testUnregisteredMismatchedRightsAndStaleHeadRefuseBeforeServerMutation() async throws {
        for mode in ["pending", "wrong-intake", "rights", "stale-head"] {
            let state = try state(), api = try await connected(state, furnishing: newFurnishing(), mode: mode)
            do { _ = try await api.prepareWorkspaceBenchVisualReplacement(replacement(), intakeId: BenchReplacementAPIProtocol.intakeId, in: state); XCTFail(mode) } catch { }
            XCTAssertTrue(BenchReplacementAPIProtocol.calls().allSatisfy { $0.httpMethod == "GET" }, mode)
        }
    }
    func testServerRejectionLatePolicyAndViewerNeverProduceSavedChange() async throws {
        for mode in ["reject", "late-policy"] {
            let state = try state(), api = try await connected(state, furnishing: newFurnishing(), mode: mode)
            do { _ = try await api.prepareWorkspaceBenchVisualReplacement(replacement(), intakeId: BenchReplacementAPIProtocol.intakeId, in: state); XCTFail(mode) } catch { }
            XCTAssertFalse(BenchReplacementAPIProtocol.calls().contains { $0.httpMethod == "PUT" }, mode)
        }
        let state = try state(), api = try await connected(state, furnishing: newFurnishing(), role: "viewer")
        do { _ = try await api.prepareWorkspaceBenchVisualReplacement(replacement(), intakeId: BenchReplacementAPIProtocol.intakeId, in: state); XCTFail() } catch { }
        XCTAssertEqual(BenchReplacementAPIProtocol.calls().count, 1)
    }

}


private final class BenchReplacementAPIProtocol: URLProtocol, @unchecked Sendable {
    static let intakeId = "88888888-8888-4888-8888-888888888888"
    private static let lock = NSLock()
    nonisolated(unsafe) private static var requests: [URLRequest] = []
    nonisolated(unsafe) private static var responses: [String: Data] = [:]
    nonisolated(unsafe) private static var mode = "valid"
    static func configure(_ state: NativeDocumentState, furnishing: WorkspaceFurnishing, mode: String, role: String) throws {
        let encoder = JSONEncoder()
        func object<T: Encodable>(_ value: T) throws -> Any { try JSONSerialization.jsonObject(with: encoder.encode(value)) }
        let session: [String: Any] = ["tenantId": state.tenantId, "userId": state.principalId, "role": role,
            "csrfToken": "synthetic-csrf", "expiresAt": "2099-01-01T00:00:00Z", "workspaceFurnishingProfile": "workspace-furnishing-intake-v1"]
        let id = state.draft!.id, prefix = "/api/v1/tenants/\(state.tenantId)"
        let studio = prefix + "/studio/exhibitions/" + id
        var descriptor = try object(furnishing) as! [String: Any]
        var details = try object(furnishing.details) as! [String: Any]
        if mode == "rights", var rights = details["rights"] as? [String: Any], var permissions = rights["permissions"] as? [String: Any] {
            permissions["display"] = false; rights["permissions"] = permissions
            details["rights"] = rights; descriptor["rights"] = rights
        }
        var issued = descriptor
        if mode == "wrong-intake" { issued["id"] = "99999999-9999-4999-8999-999999999999" }
        var intake: [String: Any] = ["intakeId": intakeId, "requestId": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "phase": mode == "pending" ? "validating" : "registered", "retryable": false,
            "error": NSNull(), "received": ["bytes": 44, "sha256": furnishing.details.model.sha256], "declaration": details]
        if mode != "pending" { intake["furnishing"] = issued }
        let policy: [String: Any] = ["exhibitionId": id, "revision": 1, "etag": state.etag!, "boundedReadOnly": false,
            "boundedProfile": true, "boundedEditRequired": false, "workspaceBenchSeatProfile": "workspace-bench-seat-v1"]
        let revision = mode == "stale-head" ? 2 : 1
        let saved: [String: Any] = ["revision": revision, "etag": "\"studio-r\(revision)-" + String(repeating: "a", count: 64) + "\"", "draft": try object(state.draft!)]
        let values = try ["/api/v1/auth/session": JSONSerialization.data(withJSONObject: session),
            studio: JSONSerialization.data(withJSONObject: saved), studio + "/native-viewport-policy": JSONSerialization.data(withJSONObject: policy),
            prefix + "/workspace/furnishing-intakes/" + intakeId: JSONSerialization.data(withJSONObject: intake),
            prefix + "/workspace/furnishings/" + furnishing.id + "/versions/1": JSONSerialization.data(withJSONObject: descriptor)]
        lock.lock(); defer { lock.unlock() }; requests = []; responses = values; Self.mode = mode
    }
    static func calls() -> [URLRequest] { lock.lock(); defer { lock.unlock() }; return requests }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        Self.lock.lock(); let path = request.url!.path
        let late = Self.mode == "late-policy" && path.hasSuffix("/native-viewport-policy") && Self.requests.contains { $0.url?.path == path }
        Self.requests.append(request)
        let model = path.hasSuffix("/model"), reject = Self.mode == "reject" && request.httpMethod == "POST"
        var data = Self.responses[path] ?? Data("{\"valid\":true}".utf8)
        if model { data = Data(repeating: 0, count: 44) }
        if reject { data = Data("{\"code\":\"BOUNDED_EDIT_UNSUPPORTED\"}".utf8) }
        if late, var policy = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            policy["boundedEditRequired"] = true; data = try! JSONSerialization.data(withJSONObject: policy)
        }
        Self.lock.unlock()
        var headers = ["Content-Type": model ? "model/gltf-binary" : "application/json"]
        if model { headers["x-exhibitos-sha256"] = nativeImportSHA256(data); headers["Content-Length"] = String(data.count) }
        else if let body = try? JSONSerialization.jsonObject(with: data) as? [String: Any], let etag = body["etag"] as? String { headers["ETag"] = etag }
        let response = HTTPURLResponse(url: request.url!, statusCode: reject ? 422 : 200, httpVersion: nil, headerFields: headers)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
