// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class NativeCurrentImageThumbnailAdmissionTests: XCTestCase {
    func testOldAndUnknownRuntimeNeverAdvertiseTheNewProducerFromLegacyProfiles() {
        for manifest in [nil, "1b542833230e3f80c3850b09fb677cc119415a04b15e53c4904942eb76b5a238", String(repeating: "f", count: 64)] {
            XCTAssertFalse(AuthoringAPI.supportsCurrentImageThumbnailRuntime(manifest: manifest))
        }
    }
    func testOnlyQualifiedThumbnailManifestAdvertisesProducer() {
        XCTAssertTrue(AuthoringAPI.supportsCurrentImageThumbnailRuntime(manifest: "0f04520a8742f9774206d12c8a2b56da8ed2a4e07c095b3cb8ad3b3e988dd988"))
    }
    func testQualifiedFramePayloadRetainsOnlyThumbnailAdmission() {
        XCTAssertTrue(AuthoringAPI.supportsCurrentImageThumbnailRuntime(manifest: "74bd7aaa8500a4bd52240827b9c6a2da0e22b2757111bb57a3b05c969ea308be"))
    }
    func testCorrected051RuntimeAdmissionIsExactAndPreservesLegacyProducerPins() throws {
        let exact = "73c0878ee50623114081cfb14cc6e2f4870727c3f399519ee67e6208342af822"
        XCTAssertTrue(AuthoringAPI.supportsCurrentImageThumbnailRuntime(manifest: exact))
        XCTAssertFalse(AuthoringAPI.supportsCurrentImageThumbnailRuntime(manifest: String(exact.dropLast()) + "3"))
        #if DEBUG
        XCTAssertTrue(AuthoringAPI.supportsCorrectedCurrentImageThumbnailRuntime(manifest: exact))
        let api = try AuthoringAPI(port: 18091, tenantId: "11111111-1111-4111-8111-111111111111", nativeRuntimeManifestSHA256: exact)
        XCTAssertTrue(api.workspaceFurnishingRuntimeQualified)
        XCTAssertTrue(api.workspacePortalFinishRuntimeQualified)
        XCTAssertFalse(api.workspaceFramePreparationRuntimeQualified)
        #endif
    }

    func testOldRuntimeRejectsBeforeAnyRequest() async throws {
        let tenant = "11111111-1111-4111-8111-111111111111"
        LegacyThumbnailAdmissionProtocol.reset()
        let api = try AuthoringAPI(port: 18091, tenantId: tenant,
            nativeRuntimeManifestSHA256: "1b542833230e3f80c3850b09fb677cc119415a04b15e53c4904942eb76b5a238",
            protocolClasses: [LegacyThumbnailAdmissionProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: tenant, principalId: tenant,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        let draft = try NativeExhibitionDraft.new(title: "Synthetic protected document")
        let document = NativeDocumentState(format: 1, revision: 304, workspaceId: tenant,
            tenantId: tenant, principalId: tenant, draft: draft, etag: nil,
            savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        do {
            _ = try await api.prepareCurrentImageThumbnail(artworkId: tenant, revision: 2,
                requestId: UUID(), in: document)
            XCTFail("Old runtime must fail locally")
        } catch let failure as AuthoringFailure {
            XCTAssertEqual(failure.code, "IMAGE_THUMBNAIL_PREPARATION_UNAVAILABLE")
            XCTAssertEqual(LegacyThumbnailAdmissionProtocol.paths(), ["/api/v1/auth/session"])
        }
    }
}

private final class LegacyThumbnailAdmissionProtocol: URLProtocol, @unchecked Sendable {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var observed: [String] = []
    static func reset() { lock.lock(); defer { lock.unlock() }; observed = [] }
    static func paths() -> [String] { lock.lock(); defer { lock.unlock() }; return observed }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        Self.lock.lock(); Self.observed.append(request.url?.path ?? ""); Self.lock.unlock()
        let id = "11111111-1111-4111-8111-111111111111"
        let body: [String: Any] = ["tenantId": id, "userId": id, "role": "admin", "csrfToken": "synthetic-csrf",
            "expiresAt": "2099-01-01T00:00:00Z", "modelPreparationProfile": "embedded-model-compact-20k-512-v1",
            "nativeProductImagePreparationProfile": "product-image-pair-2048-512-v1"]
        let data = try! JSONSerialization.data(withJSONObject: body)
        let response = HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: nil,
            headerFields: ["Content-Type": "application/json"])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
