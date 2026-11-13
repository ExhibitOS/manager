// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class PrivateHostPortalAdmissionTests: XCTestCase {
    private let id = "11111111-1111-4111-8111-111111111111"
    private let current = "1b542833230e3f80c3850b09fb677cc119415a04b15e53c4904942eb76b5a238"
    private func connect(manifest: String, reusable: String?) async throws -> AuthoringAPI {
        PortalAdmissionProtocol.configure(reusable)
        let api = try AuthoringAPI(port: 18091, tenantId: id, nativeRuntimeManifestSHA256: manifest,
            protocolClasses: [PortalAdmissionProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: id, principalId: id,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        return api
    }
    func testExactCurrentFamilyAndLiveReusableProfileAdmitExistingRectangularContract() async throws {
        let api = try await connect(manifest: current, reusable: "source-bound-fit-v1")
        let furnishing = await api.admitsWorkspaceFurnishings
        let portal = await api.admitsWorkspacePortalFinish
        let fit = await api.admitsWorkspaceRectangularPortalFit
        XCTAssertTrue(furnishing); XCTAssertTrue(portal); XCTAssertTrue(fit)
    }
    func testMissingWrongProfileOrUnknownRuntimeDoesNotGainFitAdmission() async throws {
        for profile in [nil, "unqualified"] {
            let api = try await connect(manifest: current, reusable: profile)
            let fit = await api.admitsWorkspaceRectangularPortalFit
            XCTAssertFalse(fit)
        }
        let unknown = try await connect(manifest: String(repeating: "f", count: 64), reusable: "source-bound-fit-v1")
        let admitted = await unknown.admitsWorkspacePortalFinish
        XCTAssertFalse(admitted)
    }
}
private final class PortalAdmissionProtocol: URLProtocol, @unchecked Sendable {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var profile: String?
    static func configure(_ value: String?) { lock.lock(); defer { lock.unlock() }; profile = value }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        Self.lock.lock(); let profile = Self.profile; Self.lock.unlock()
        let id = "11111111-1111-4111-8111-111111111111"
        var body: [String: Any] = ["tenantId": id, "userId": id, "role": "admin", "csrfToken": "synthetic-csrf",
            "expiresAt": "2099-01-01T00:00:00Z", "workspaceFurnishingProfile": "workspace-furnishing-intake-v1",
            "workspaceFurnishingStoolProfile": "workspace-furnishing-stool-v1",
            "workspaceFurnishingRoomVisualProfile": "workspace-furnishing-room-visual-v1",
            "workspaceArtworkFrameProfile": "workspace-artwork-frame-v1",
            "workspaceArtworkFrameModelsProfile": "workspace-artwork-frame-models-v1",
            "workspacePortalFinishProfile": "workspace-portal-finish-v1", "workspacePortalRectangularFitProfile": "rectangular-portal-v1"]
        if let profile { body["workspaceFurnishingReusableFitProfile"] = profile }
        let data = try! JSONSerialization.data(withJSONObject: body)
        let response = HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: nil,
            headerFields: ["Content-Type": "application/json"])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
