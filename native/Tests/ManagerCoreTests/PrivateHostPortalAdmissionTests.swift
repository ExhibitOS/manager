// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class PrivateHostPortalAdmissionTests: XCTestCase {
    private let id = "11111111-1111-4111-8111-111111111111"
    private let current = "1b542833230e3f80c3850b09fb677cc119415a04b15e53c4904942eb76b5a238"
    private func connect(manifest: String, reusable: String?, catalogue: Bool = false) async throws -> AuthoringAPI {
        PortalAdmissionProtocol.configure(reusable, catalogue: catalogue)
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

    func testExactPartialWallPortalRuntimeRequiresTheReusableFitProfile() async throws {
        let partial = "803ce4960489551e1a2ac9c84ca7f3a804c2f5e0324e719a590661c457ff2918"
        let qualified = try await connect(manifest: partial, reusable: "source-bound-fit-v1")
        let furnishing = await qualified.admitsWorkspaceFurnishings
        let portal = await qualified.admitsWorkspacePortalFinish
        let fit = await qualified.admitsWorkspaceRectangularPortalFit
        XCTAssertTrue(furnishing)
        XCTAssertTrue(portal)
        XCTAssertTrue(fit)

        let missing = try await connect(manifest: partial, reusable: nil)
        let denied = await missing.admitsWorkspaceRectangularPortalFit
        XCTAssertFalse(denied)
    }

    #if DEBUG && os(macOS)
    func testCurrentDefaultCatalogueRequiresItsExactManifestAndLiveHosts() async throws {
        let manifest = "67613da677830cdc9f177cfee6ed650df8f3037f86be745c36f32b1eac3052e8"
        let qualified = try await connect(manifest: manifest, reusable: "source-bound-fit-v1", catalogue: true)
        let update = await qualified.admitsDefaultCatalogueUpdate
        let rail = await qualified.admitsWorkspaceCeilingRail
        let portal = await qualified.admitsWorkspaceModularPortalFit
        XCTAssertTrue(update)
        XCTAssertTrue(rail)
        XCTAssertTrue(portal)
        XCTAssertTrue(AuthoringAPI.supportsOrdinarySavedHeadStaticRuntime(manifest: manifest))

        let old = try await connect(manifest: current, reusable: "source-bound-fit-v1", catalogue: true)
        let oldUpdate = await old.admitsDefaultCatalogueUpdate
        XCTAssertFalse(oldUpdate)
        let incomplete = try await connect(manifest: manifest, reusable: nil, catalogue: true)
        let incompleteUpdate = await incomplete.admitsDefaultCatalogueUpdate
        XCTAssertFalse(incompleteUpdate)
    }
    #endif
}
private final class PortalAdmissionProtocol: URLProtocol, @unchecked Sendable {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var profile: String?
    nonisolated(unsafe) private static var catalogue = false
    static func configure(_ value: String?, catalogue: Bool) {
        lock.lock(); defer { lock.unlock() }
        profile = value
        self.catalogue = catalogue
    }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        Self.lock.lock()
        let profile = Self.profile, catalogue = Self.catalogue
        Self.lock.unlock()
        let id = "11111111-1111-4111-8111-111111111111"
        var body: [String: Any] = ["tenantId": id, "userId": id, "role": "admin", "csrfToken": "synthetic-csrf",
            "expiresAt": "2099-01-01T00:00:00Z", "workspaceFurnishingProfile": "workspace-furnishing-intake-v1",
            "workspaceFurnishingStoolProfile": "workspace-furnishing-stool-v1",
            "workspaceFurnishingRoomVisualProfile": "workspace-furnishing-room-visual-v1",
            "workspaceArtworkFrameProfile": "workspace-artwork-frame-v1",
            "workspaceArtworkFrameModelsProfile": "workspace-artwork-frame-models-v1",
            "workspacePortalFinishProfile": "workspace-portal-finish-v1", "workspacePortalRectangularFitProfile": "rectangular-portal-v1"]
        if let profile { body["workspaceFurnishingReusableFitProfile"] = profile }
        if catalogue {
            body["defaultCatalogueUpdateProfile"] = "default-catalogue-update-v1"
            body["workspaceCeilingRailProfile"] = "workspace-ceiling-rail-v1"
            body["workspacePortalModularFitProfile"] = "modular-portal-v1"
        }
        let data = try! JSONSerialization.data(withJSONObject: body)
        let response = HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: nil,
            headerFields: ["Content-Type": "application/json"])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
