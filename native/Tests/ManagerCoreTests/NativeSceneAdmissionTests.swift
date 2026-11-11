// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class NativeSceneAdmissionTests: XCTestCase {
    private let owner = "10000000-0000-4000-8000-000000000001"
    private let manifest = "d6f8eb6a9abca42685de3b8d2671a63ab12795a45387129d76e4343874aa985f"

    private func attempt(operation: String, markers: [String: String], runtime: String) async throws -> (String, [String]) {
        let fields: [String: Any] = ["tenantId": owner, "userId": owner, "role": "owner",
            "csrfToken": "synthetic-csrf", "expiresAt": "2099-01-01T00:00:00Z"]
            .merging(markers) { _, value in value }
        SceneAdmissionProtocol.configure(try JSONSerialization.data(withJSONObject: fields))
        defer { SceneAdmissionProtocol.reset() }
        let api = try AuthoringAPI(port: 18091, tenantId: owner, nativeRuntimeManifestSHA256: runtime, protocolClasses: [SceneAdmissionProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: owner, principalId: owner,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        let etag = "\"studio-r1-" + String(repeating: "a", count: 64) + "\""
        do {
            _ = try await api.nativeAuthoringProposal(exhibitionId: owner, studioRevision: 1, etag: etag,
                proposal: .object(["type": .string(operation), "tenantId": .string(owner), "principalId": .string(owner)]))
            XCTFail("The synthetic server must reject this incomplete proposal")
            return ("unexpected-success", SceneAdmissionProtocol.methods())
        } catch let failure as AuthoringFailure {
            return (failure.code, SceneAdmissionProtocol.methods())
        }
    }

    func testSceneRequiresItsDistinctLiveMarkerBeforeHTTP() async throws {
        for runtime in [manifest, "bc4dc0736a07774550107f12cbab729d91d2156d6ec8828c321dcf0677555c7d", "21de5a4cdc00d01f51f4ae0746c8abc08bc062e71c2d73778f706d2f4e8d6088", "5b3aadfe5332bdaf084741f2b9a20b0e9c6733c44be36cec7f4d1ddb8ba338f2", "1a0333bcb3f880878348911c6f7e3dfde81044506b3b887609740e4dc06c1738"] {
        let denied = try await attempt(operation: "add-light", markers: ["nativeAuthoringProposalProfile": "saved-head-v1"], runtime: runtime)
        XCTAssertEqual(denied.0, "NATIVE_AUTHORING_PROPOSAL_UNAVAILABLE")
        XCTAssertEqual(denied.1, ["GET"])
        let admitted = try await attempt(operation: "add-light", markers: ["nativeSceneAuthoringProposalProfile": "saved-head-scene-v1"], runtime: runtime)
        XCTAssertEqual(admitted.0, "AUTHORING_PROPOSAL_INVALID")
        XCTAssertEqual(admitted.1, ["GET", "POST"])
        }
    }

    func testCarriedPlaqueUsesItsActualMarkerAndRetainsServerValidation() async throws {
        let eac = "eac64ed106bdac283c1aa0c6c3dee5541c98919ed4a67bd9e5509b21496b5bce"
        let admitted = try await attempt(operation: "add-plaque", markers: ["nativeAuthoringProposalProfile": "saved-head-v1"], runtime: eac)
        XCTAssertEqual(admitted.0, "AUTHORING_PROPOSAL_INVALID")
        XCTAssertEqual(admitted.1, ["GET", "POST"])
        let denied = try await attempt(operation: "add-plaque", markers: ["nativeSceneAuthoringProposalProfile": "saved-head-scene-v1"], runtime: eac)
        XCTAssertEqual(denied.0, "NATIVE_AUTHORING_PROPOSAL_UNAVAILABLE")
        XCTAssertEqual(denied.1, ["GET"])
    }

    func testFixtureRequiresBothDistinctLiveProfilesBeforeHTTP() async throws {
        for runtime in ["21de5a4cdc00d01f51f4ae0746c8abc08bc062e71c2d73778f706d2f4e8d6088", "5b3aadfe5332bdaf084741f2b9a20b0e9c6733c44be36cec7f4d1ddb8ba338f2", "1a0333bcb3f880878348911c6f7e3dfde81044506b3b887609740e4dc06c1738"] {
        let scene = ["nativeSceneAuthoringProposalProfile": "saved-head-scene-v1"]
        let fixture = ["nativeFixtureAuthoringProposalProfile": "saved-head-fixture-v1"]
        let admitted = scene.merging(fixture) { _, value in value }
        for operation in ["set-furnishing-visual", "set-light-attachment"] {
            for markers in [[:], scene, fixture,
                            scene.merging(["nativeFixtureAuthoringProposalProfile": "unknown"]) { _, value in value }] {
                let denied = try await attempt(operation: operation, markers: markers, runtime: runtime)
                XCTAssertEqual(denied.0, "NATIVE_AUTHORING_PROPOSAL_UNAVAILABLE")
                XCTAssertEqual(denied.1, ["GET"])
            }
            let reply = try await attempt(operation: operation, markers: admitted, runtime: runtime)
            XCTAssertEqual(reply.0, "AUTHORING_PROPOSAL_INVALID")
            XCTAssertEqual(reply.1, ["GET", "POST"])
        }
        let unknown = try await attempt(operation: "set-any-fixture", markers: admitted, runtime: runtime)
        XCTAssertEqual(unknown.1, ["GET"])
        let sceneOnly = try await attempt(operation: "add-light", markers: fixture, runtime: runtime)
        XCTAssertEqual(sceneOnly.1, ["GET"])
        }
    }

    func testDefault17ReplacementStillRequiresItsLiveProfileAndRoomVisualAdmission() async throws {
        let base = ["workspaceFurnishingProfile": "workspace-furnishing-intake-v1",
                    "workspaceFurnishingStoolProfile": "workspace-furnishing-stool-v1",
                    "workspaceFurnishingRoomVisualProfile": "workspace-furnishing-room-visual-v1"]
        let active = base.merging(["workspaceInfoStandReplacementProfile": WorkspaceInfoStandReplacement.profile]) { _, value in value }
        var missingRoom = active; missingRoom.removeValue(forKey: "workspaceFurnishingRoomVisualProfile")
        var unknown = active; unknown["workspaceInfoStandReplacementProfile"] = "unknown"
        for (markers, expected) in [(base, false), (active, true), (missingRoom, false), (unknown, false)] {
            let fields = ["tenantId": owner, "userId": owner, "role": "admin", "csrfToken": "synthetic-csrf", "expiresAt": "2099-01-01T00:00:00Z"]
                .merging(markers) { _, value in value }
            SceneAdmissionProtocol.configure(try JSONSerialization.data(withJSONObject: fields))
            let api = try AuthoringAPI(port: 18091, tenantId: owner,
                nativeRuntimeManifestSHA256: "1a0333bcb3f880878348911c6f7e3dfde81044506b3b887609740e4dc06c1738",
                protocolClasses: [SceneAdmissionProtocol.self])
            _ = try await api.connect(LocalAuthoringSession(tenantId: owner, principalId: owner,
                origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
            let admitted = await api.admitsWorkspaceInfoStandReplacement
            XCTAssertEqual(admitted, expected)
            XCTAssertEqual(SceneAdmissionProtocol.methods(), ["GET"])
            await api.endLocalAuthoringSession()
        }
        SceneAdmissionProtocol.reset()
    }

    func testLightObjectRequiresDistinctLiveProfileAndSceneAuthorityBeforeHTTP() async throws {
        let scene = ["nativeSceneAuthoringProposalProfile": "saved-head-scene-v1"]
        let object = ["nativeLightObjectAuthoringProposalProfile": LightObjectAuthoring.profile]
        let admitted = scene.merging(object) { _, value in value }
        for operation in LightObjectAuthoring.commands.sorted() {
            for markers in [[:], scene, object,
                            scene.merging(["nativeLightObjectAuthoringProposalProfile": "unknown"]) { _, value in value }] {
                let reply = try await attempt(operation: operation, markers: markers, runtime: manifest)
                XCTAssertEqual(reply.0, "NATIVE_AUTHORING_PROPOSAL_UNAVAILABLE")
                XCTAssertEqual(reply.1, ["GET"])
            }
            let reply = try await attempt(operation: operation, markers: admitted, runtime: manifest)
            XCTAssertEqual(reply.0, "AUTHORING_PROPOSAL_INVALID")
            XCTAssertEqual(reply.1, ["GET", "POST"])
        }
        let unknown = try await attempt(operation: "set-any-light-object", markers: admitted, runtime: manifest)
        XCTAssertEqual(unknown.1, ["GET"])
    }

    func testInstalledExportEntryUsesExportCapabilitiesWithoutImagePreparation() throws {
        let examples: [([String: String], Bool)] = [
            ([:], false),
            (["nativeFreshImagePreparationProfile": "ordinary-image-preparation-v1"], false),
            (["nativeAuthoringProposalProfile": "saved-head-v1"], false),
            (["nativeOexExportProfile": "bounded-file-v1"], true),
            (["nativePublicationProfile": "bounded-local-v1"], true),
            (["nativeOexExportProfile": "unknown", "nativePublicationProfile": "unknown"], false)
        ]
        for (markers, expected) in examples {
            let fields: [String: String] = ["tenantId": owner, "userId": owner, "role": "curator",
                "csrfToken": "synthetic-csrf", "expiresAt": "2099-01-01T00:00:00Z"].merging(markers) { _, new in new }
            let session = try JSONDecoder().decode(AuthoringSession.self, from: JSONSerialization.data(withJSONObject: fields))
            XCTAssertEqual(NativeAutomationSession.installedExportEntryAllowed(session: session), expected)
        }
        XCTAssertFalse(NativeAutomationSession.installedExportEntryAllowed(session: nil))
    }

    func testInitialOrdinaryCreationUsesAuthenticatedRoleWithoutBorrowingProposalMarkers() async throws {
        for role in ["admin", "curator", "artist", "viewer"] {
            let session = try JSONSerialization.data(withJSONObject: ["tenantId": owner, "userId": owner,
                "role": role, "csrfToken": "synthetic-csrf", "expiresAt": "2099-01-01T00:00:00Z"])
            SceneAdmissionProtocol.configure(session)
            let api = try AuthoringAPI(port: 18091, tenantId: owner, nativeRuntimeManifestSHA256: manifest,
                protocolClasses: [SceneAdmissionProtocol.self])
            let connected = try await api.connect(LocalAuthoringSession(tenantId: owner, principalId: owner,
                origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
            let allowed = await api.admitsPersonalExhibitionCreation
            XCTAssertEqual(allowed, role != "viewer")
            XCTAssertEqual(NativeAutomationSession.installedAuthoringEntryAllowed(canCreate: allowed, session: connected), role != "viewer")
            XCTAssertEqual(SceneAdmissionProtocol.methods(), ["GET"])
        }
        XCTAssertFalse(NativeAutomationSession.installedAuthoringEntryAllowed(canCreate: true, session: nil))
        SceneAdmissionProtocol.reset()
    }
}

private final class SceneAdmissionProtocol: URLProtocol, @unchecked Sendable {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var session = Data()
    nonisolated(unsafe) private static var observed: [String] = []
    static func configure(_ data: Data) { lock.lock(); defer { lock.unlock() }; session = data; observed = [] }
    static func reset() { configure(Data()) }
    static func methods() -> [String] { lock.lock(); defer { lock.unlock() }; return observed }
    private static func response(_ request: URLRequest) -> (Int, Data) {
        lock.lock(); defer { lock.unlock() }; observed.append(request.httpMethod ?? "")
        if request.httpMethod == "GET" { return (200, session) }
        let valid = request.value(forHTTPHeaderField: "x-csrf-token") == "synthetic-csrf"
            && request.value(forHTTPHeaderField: "Origin") == "http://127.0.0.1:18091"
            && request.value(forHTTPHeaderField: "If-Match")?.hasPrefix("\"studio-r1-") == true
        return (valid ? 422 : 403, Data((valid ? "{\"code\":\"AUTHORING_PROPOSAL_INVALID\"}" : "{\"code\":\"FORBIDDEN\"}").utf8))
    }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        let (status, data) = Self.response(request)
        let response = HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: "HTTP/1.1", headerFields: ["Content-Type": "application/json"])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
