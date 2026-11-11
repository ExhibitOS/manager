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
        for runtime in [manifest, "bc4dc0736a07774550107f12cbab729d91d2156d6ec8828c321dcf0677555c7d"] {
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
