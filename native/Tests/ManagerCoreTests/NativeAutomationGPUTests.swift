// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

final class NativeAutomationGPUTests: XCTestCase {
    func testClosedCommandRetainsSameRequestAndOwnerScope() throws {
        let artwork = UUID().uuidString.lowercased(), asset = UUID().uuidString.lowercased()
        let key = UUID().uuidString.lowercased(), generation = UUID().uuidString.lowercased()
        let input = try NativeGPUModelPreparationRequest(revision: 2, assetId: asset, sha256: String(repeating: "a", count: 64), bytes: 512, idempotencyKey: key, generation: generation)
        let inputValue = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(input))
        let request: DocumentValue = .object(["artworkId": .string(artwork), "input": inputValue])
        let parsed = try NativeAutomationGPUCommand(request, creates: true)
        XCTAssertEqual(parsed.input, input)
        let workspace = UUID().uuidString.lowercased(), tenant = UUID().uuidString.lowercased(), principal = UUID().uuidString.lowercased()
        let retained = try NativeGPUModelPreparationTransfer(workspaceId: workspace, tenantId: tenant, principalId: principal, artworkId: artwork, request: parsed.input)
        XCTAssertTrue(retained.matches(workspaceId: workspace, tenantId: tenant, principalId: principal, artworkId: artwork, request: input, jobId: nil))
        XCTAssertFalse(retained.matches(workspaceId: workspace, tenantId: tenant, principalId: UUID().uuidString, artworkId: artwork, request: input, jobId: nil))
        var extra = request; extra["path"] = .string("/tmp/input")
        XCTAssertThrowsError(try NativeAutomationGPUCommand(extra, creates: true))
        XCTAssertThrowsError(try NativeAutomationGPUCommand(request, creates: false))
        var status = request; status["jobId"] = .string(UUID().uuidString.lowercased())
        XCTAssertNotNil(try NativeAutomationGPUCommand(status, creates: false).jobId)
        status["jobId"] = .string("not-an-id")
        XCTAssertThrowsError(try NativeAutomationGPUCommand(status, creates: false))
    }

    func testProposedCapabilityAloneDoesNotAdmitUnqualifiedRuntime() async throws {
        let api = try AuthoringAPI(port: 13200, tenantId: UUID().uuidString.lowercased())
        let admitted = await api.admitsGPUModelPreparation
        XCTAssertFalse(admitted)
    }

    func testOwnedCancelDoesNotRequireCapabilityOrStatusPreflight() async throws {
        let tenant = UUID().uuidString.lowercased(), user = UUID().uuidString.lowercased()
        let artwork = UUID().uuidString.lowercased(), asset = UUID().uuidString.lowercased(), job = UUID().uuidString.lowercased()
        let hash = String(repeating: "a", count: 64)
        let input = try NativeGPUModelPreparationRequest(revision: 2, assetId: asset, sha256: hash, bytes: 512, idempotencyKey: "op-1", generation: "gen-1")
        let session = try JSONSerialization.data(withJSONObject: ["tenantId": tenant, "userId": user, "role": "admin", "csrfToken": "synthetic", "expiresAt": "2099-01-01T00:00:00Z"])
        let source: DocumentValue = .object(["assetId": .string(asset), "mime": .string("model/gltf-binary"), "sha256": .string(hash), "bytes": .integer(512), "approvalSHA256": .string(hash), "rightsSHA256": .string(hash)])
        let response: DocumentValue = .object(["id": .string(job), "state": .string("cancelled"), "progress": .integer(20), "attempts": .integer(1), "errorCode": .null, "retryAt": .string("2026-11-12T10:00:00Z"), "gpuPreparation": .object(["version": .integer(1), "purpose": .string(NativeGPUModelPreparation.purpose), "profile": .string(NativeGPUModelPreparation.profile), "recipeSHA256": .string(NativeGPUModelPreparation.recipe), "operationID": .string("op-1"), "generation": .string("gen-1"), "artworkRevision": .integer(2), "status": .string("cancelled"), "workerDrained": .bool(false), "source": source])])
        let cancelled = try JSONEncoder().encode(response)
        GPUMockProtocol.configure { request in
            if request.url?.path == "/api/v1/auth/session" { return (200, session) }
            if request.url?.path == "/api/v1/tenants/\(tenant)/imports/\(job)/cancel", request.httpMethod == "POST" { return (200, cancelled) }
            return (403, Data("{}".utf8))
        }
        defer { GPUMockProtocol.configure(nil) }
        let api = try AuthoringAPI(port: 18091, tenantId: tenant, protocolClasses: [GPUMockProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: tenant, principalId: user, origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        let observed = try await api.cancelGPUModelPreparation(job, artworkId: artwork, input: input)
        XCTAssertEqual(observed.value["gpuPreparation"]["workerDrained"], .bool(false))
        XCTAssertEqual(GPUMockProtocol.observed(), ["GET", "POST"])
    }

    func testReadOnlyRecoveryDoesNotAdmitCreateAndNullStaysUnknown() async throws {
        let tenant = UUID().uuidString.lowercased(), user = UUID().uuidString.lowercased()
        let artwork = UUID().uuidString.lowercased()
        let session = try JSONSerialization.data(withJSONObject: ["tenantId": tenant, "userId": user, "role": "admin", "csrfToken": "synthetic", "expiresAt": "2099-01-01T00:00:00Z", "nativeGpuModelPreparationProfile": NativeGPUModelPreparation.profile])
        let empty = try JSONSerialization.data(withJSONObject: ["artworkId": artwork, "operationID": "op-1", "generation": "gen-1", "job": NSNull()])
        GPUMockProtocol.configure { request in
            if request.url?.path == "/api/v1/auth/session" { return (200, session) }
            if request.url?.path == "/api/v1/tenants/\(tenant)/cms/artworks/\(artwork)/gpu-preparations/recover", request.httpMethod == "POST" { return (200, empty) }
            return (500, Data("{}".utf8))
        }
        defer { GPUMockProtocol.configure(nil) }
        let api = try AuthoringAPI(port: 18091, tenantId: tenant, protocolClasses: [GPUMockProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: tenant, principalId: user, origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        let input = try NativeGPUModelPreparationRequest(revision: 2, assetId: UUID().uuidString.lowercased(), sha256: String(repeating: "a", count: 64), bytes: 512, idempotencyKey: "op-1", generation: "gen-1")
        do {
            _ = try await api.createGPUModelPreparationIdentity(artworkId: artwork, input: input)
            XCTFail("Unadmitted runtime dispatched a create")
        } catch let error as AuthoringFailure { XCTAssertEqual(error.code, "GPU_MODEL_PREPARATION_UNAVAILABLE") }
        let observed = try await api.recoverGPUModelPreparationIdentity(artworkId: artwork, input: input)
        XCTAssertNil(observed)
        XCTAssertEqual(GPUMockProtocol.observed(), ["GET", "POST"])
    }

    func testRecoveryRejectsMismatchedOperationEvenForNull() async throws {
        let tenant = UUID().uuidString.lowercased(), user = UUID().uuidString.lowercased(), artwork = UUID().uuidString.lowercased()
        let session = try JSONSerialization.data(withJSONObject: ["tenantId": tenant, "userId": user, "role": "admin", "csrfToken": "synthetic", "expiresAt": "2099-01-01T00:00:00Z"])
        let mismatched = try JSONSerialization.data(withJSONObject: ["artworkId": artwork, "operationID": "other", "generation": "gen-1", "job": NSNull()])
        GPUMockProtocol.configure { request in request.url?.path == "/api/v1/auth/session" ? (200, session) : (200, mismatched) }
        defer { GPUMockProtocol.configure(nil) }
        let api = try AuthoringAPI(port: 18091, tenantId: tenant, protocolClasses: [GPUMockProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: tenant, principalId: user, origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        let input = try NativeGPUModelPreparationRequest(revision: 2, assetId: UUID().uuidString.lowercased(), sha256: String(repeating: "a", count: 64), bytes: 512, idempotencyKey: "op-1", generation: "gen-1")
        do { _ = try await api.recoverGPUModelPreparationIdentity(artworkId: artwork, input: input); XCTFail("Mismatched request accepted") }
        catch let error as AuthoringFailure { XCTAssertEqual(error.code, "GPU_PREPARATION_INVALID") }
    }

    func testReadyLookingReplayRetainsOnlyJobIdentity() throws {
        let asset = UUID().uuidString.lowercased(), job = UUID().uuidString.lowercased()
        let hash = String(repeating: "a", count: 64)
        let input = try NativeGPUModelPreparationRequest(revision: 2, assetId: asset, sha256: hash, bytes: 512, idempotencyKey: "op-1", generation: "gen-1")
        let source: DocumentValue = .object(["assetId": .string(asset), "mime": .string("model/gltf-binary"), "sha256": .string(hash), "bytes": .integer(512), "approvalSHA256": .string(hash), "rightsSHA256": .string(hash)])
        var view: DocumentValue = .object(["id": .string(job), "state": .string("approved"), "progress": .integer(100), "attempts": .integer(1), "errorCode": .null, "retryAt": .string("2026-11-12T10:00:00Z"), "gpuPreparation": .object(["version": .integer(1), "purpose": .string(NativeGPUModelPreparation.purpose), "profile": .string(NativeGPUModelPreparation.profile), "recipeSHA256": .string(NativeGPUModelPreparation.recipe), "operationID": .string("op-1"), "generation": .string("gen-1"), "artworkRevision": .integer(2), "status": .string("ready"), "workerDrained": .bool(true), "source": source])])
        XCTAssertEqual(try NativeGPUModelJobIdentity(view, input: input).jobId, job)
        view["gpuPreparation"]["generation"] = .string("other")
        XCTAssertThrowsError(try NativeGPUModelJobIdentity(view, input: input))
    }

    func testUploadStateRoundTripsGPUIdentityAndPreservesLegacy() throws {
        let workspace = UUID().uuidString.lowercased(), tenant = UUID().uuidString.lowercased()
        let principal = UUID().uuidString.lowercased(), artwork = UUID().uuidString.lowercased()
        let base: [String: Any] = ["format": 1, "revision": 1, "workspaceId": workspace, "tenantId": tenant,
                                   "principalId": principal, "intent": NSNull(), "jobId": NSNull(), "fileName": NSNull()]
        let decoder = JSONDecoder(), encoder = JSONEncoder()
        var state = try decoder.decode(NativeUploadState.self, from: JSONSerialization.data(withJSONObject: base))
        XCTAssertNil(state.gpuVariant)
        let input = try NativeGPUModelPreparationRequest(revision: 1, assetId: UUID().uuidString.lowercased(), sha256: String(repeating: "a", count: 64), bytes: 512, idempotencyKey: "op-1", generation: "gen-1")
        state.gpuVariant = try NativeGPUModelPreparationTransfer(workspaceId: workspace, tenantId: tenant, principalId: principal, artworkId: artwork, request: input)
        let reopened = try decoder.decode(NativeUploadState.self, from: encoder.encode(state))
        XCTAssertEqual(reopened.gpuVariant, state.gpuVariant)
        XCTAssertTrue(reopened.holdsRequiredModelMetadata(for: artwork))
        XCTAssertFalse(reopened.permitsOrdinaryJobAction(UUID().uuidString, selectedArtworkId: artwork))
    }
}

private final class GPUMockProtocol: URLProtocol, @unchecked Sendable {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var handler: (@Sendable (URLRequest) -> (Int, Data))?
    nonisolated(unsafe) private static var methods: [String] = []
    static func configure(_ value: (@Sendable (URLRequest) -> (Int, Data))?) {
        lock.lock(); defer { lock.unlock() }; handler = value; methods = []
    }
    static func observed() -> [String] { lock.lock(); defer { lock.unlock() }; return methods }
    private static func call(_ request: URLRequest) -> (Int, Data)? {
        lock.lock(); defer { lock.unlock() }; methods.append(request.httpMethod ?? ""); return handler?(request)
    }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        guard let url = request.url, let (status, body) = Self.call(request),
              let response = HTTPURLResponse(url: url, statusCode: status, httpVersion: "HTTP/1.1", headerFields: ["Content-Type": "application/json"]) else {
            client?.urlProtocol(self, didFailWithError: URLError(.badServerResponse)); return
        }
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: body); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
