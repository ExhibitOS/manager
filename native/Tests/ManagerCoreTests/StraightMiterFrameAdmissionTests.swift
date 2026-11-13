// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class StraightMiterFrameAdmissionTests: XCTestCase {
    private let id = "11111111-1111-4111-8111-111111111111"
    private let exact = "74bd7aaa8500a4bd52240827b9c6a2da0e22b2757111bb57a3b05c969ea308be"

    func testExactRuntimeAndLiveProfilesRequiredBeforeCreateRequest() async throws {
        let cases: [(String, String?, String, Bool)] = [
            (exact, "source-bound-fit-v1", "admin", true),
            ("73c0878ee50623114081cfb14cc6e2f4870727c3f399519ee67e6208342af822", "source-bound-fit-v1", "admin", true),
            ("0f04520a8742f9774206d12c8a2b56da8ed2a4e07c095b3cb8ad3b3e988dd988", "source-bound-fit-v1", "admin", false),
            (exact, nil, "admin", false), (exact, "unknown", "admin", false),
            (exact, "source-bound-fit-v1", "viewer", false),
            ("1b542833230e3f80c3850b09fb677cc119415a04b15e53c4904942eb76b5a238", "source-bound-fit-v1", "admin", false),
            (String(repeating: "f", count: 64), "source-bound-fit-v1", "admin", false)]
        for (manifest, profile, role, expected) in cases {
            MiterAdmissionProtocol.configure(profile: profile, role: role)
            let api = try AuthoringAPI(port: 18092, tenantId: id, nativeRuntimeManifestSHA256: manifest,
                protocolClasses: [MiterAdmissionProtocol.self])
            _ = try await api.connect(LocalAuthoringSession(tenantId: id, principalId: id,
                origin: "http://127.0.0.1:18092", token: String(repeating: "a", count: 43)))
            let admitted = await api.admitsWorkspaceStraightMiterFrameFit
            XCTAssertEqual(admitted, expected)
            let details = try JSONDecoder().decode(WorkspaceFurnishingDetails.self, from: Data("""
                {"kind":"artwork-frame","name":"Synthetic","provenance":"Synthetic only",
                 "rights":{"holder":"Fixture","ownership":"owner","licenseId":"CC0-1.0","creditLine":"",
                   "permissions":{"display":true,"download":false,"export":false,"commercial":false}},
                 "model":{"mime":"model/gltf-binary","bytes":23,"sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}
                """.utf8))
            do {
                _ = try await api.createWorkspaceFurnishing(requestId: id, details: details, fitProfile: .straightMiterRing)
                XCTAssertTrue(expected)
            } catch {
                XCTAssertFalse(expected, "Admitted runtime failed: \(error)")
                XCTAssertEqual((error as? AuthoringFailure)?.code, "WORKSPACE_FURNISHING_UNAVAILABLE")
            }
            XCTAssertEqual(MiterAdmissionProtocol.postCount, expected ? 1 : 0)
        }
    }
}

private final class MiterAdmissionProtocol: URLProtocol, @unchecked Sendable {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var profile: String?
    nonisolated(unsafe) private static var role = "admin"
    nonisolated(unsafe) private static var posts = 0
    static var postCount: Int { lock.lock(); defer { lock.unlock() }; return posts }
    static func configure(profile: String?, role: String) {
        lock.lock(); defer { lock.unlock() }; self.profile = profile; self.role = role; posts = 0
    }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        Self.lock.lock(); let profile = Self.profile, role = Self.role
        if request.httpMethod == "POST" { Self.posts += 1 }
        Self.lock.unlock()
        let id = "11111111-1111-4111-8111-111111111111"
        var body: [String: Any] = ["tenantId": id, "userId": id, "role": role, "csrfToken": "synthetic-csrf",
            "expiresAt": "2099-01-01T00:00:00Z", "workspaceFurnishingProfile": "workspace-furnishing-intake-v1",
            "workspaceFurnishingStoolProfile": "workspace-furnishing-stool-v1",
            "workspaceFurnishingRoomVisualProfile": "workspace-furnishing-room-visual-v1",
            "workspaceArtworkFrameProfile": "workspace-artwork-frame-v1",
            "workspaceArtworkFrameModelsProfile": "workspace-artwork-frame-models-v1"]
        if let profile { body["workspaceFurnishingReusableFitProfile"] = profile }
        if request.httpMethod == "POST" {
            let data: Data
            if let value = request.httpBody { data = value }
            else {
                let stream = request.httpBodyStream!; stream.open(); defer { stream.close() }
                var buffer = [UInt8](repeating: 0, count: 4096); var value = Data()
                while stream.hasBytesAvailable { let n = stream.read(&buffer, maxLength: buffer.count); if n <= 0 { break }; value.append(contentsOf: buffer.prefix(n)) }
                data = value
            }
            var declaration = try! JSONSerialization.jsonObject(with: data) as! [String: Any]
            XCTAssertEqual(declaration.removeValue(forKey: "fitProfile") as? String, "straight-miter-ring-v1")
            let requestId = declaration.removeValue(forKey: "requestId")!
            XCTAssertNil(declaration["fit"]); XCTAssertNil(declaration["preparationPolicy"])
            body = ["intakeId": id, "requestId": requestId, "phase": "uploading", "retryable": false,
                "error": NSNull(), "received": NSNull(), "declaration": declaration, "fitProfile": "straight-miter-ring-v1"]
        }
        let data = try! JSONSerialization.data(withJSONObject: body)
        let response = HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: nil,
            headerFields: ["Content-Type": "application/json"])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
