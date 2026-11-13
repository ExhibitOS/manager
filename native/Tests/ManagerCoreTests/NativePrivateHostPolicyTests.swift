// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class NativePrivateHostPolicyTests: XCTestCase {
    private let owner = "11111111-1111-4111-8111-111111111111"
    private let etag = "\"studio-r1-" + String(repeating: "a", count: 64) + "\""
    private func fixture(_ mode: String = "ordinary") async throws -> (AuthoringAPI, NativePrivateHost, NativeDocumentState) {
        let protectedDraft = try NativeExhibitionDraft.new(title: "Protected official")
        let document = NativeDocumentState(format: 1, revision: 304, workspaceId: owner, tenantId: owner,
            principalId: owner, draft: protectedDraft, etag: nil, savedEditVersion: 1, pending: nil)
        var host = try NativePrivateHost(requestId: UUID().uuidString.lowercased(), title: "Private ordinary", protecting: document, roomBaseLight: false)
        let pending = try host.prepareSave(requestId: host.creationRequestId)
        let remote = NativeStudioResponse(revision: 1, etag: etag, draft: pending.draft)
        try host.acceptSaved(remote)
        let encoder = JSONEncoder()
        let remoteWire: DocumentValue = .object(["revision": .integer(1), "etag": .string(etag),
            "draft": try JSONDecoder().decode(DocumentValue.self, from: encoder.encode(remote.draft))])
        PrivateHostPolicyProtocol.configure(remote: remoteWire, id: host.document.draft!.id, etag: etag, mode: mode, owner: owner)
        let api = try AuthoringAPI(port: 18091, tenantId: owner, protocolClasses: [PrivateHostPolicyProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: owner, principalId: owner,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        return (api, host, document)
    }
    private func command(_ host: NativePrivateHost) -> DocumentValue {
        .object(["type": .string("room-update"), "id": host.document.draft!.candidate["rooms"].items![0]["id"],
            "name": .string("Ordinary edit"), "width": .integer(12), "height": .integer(4), "depth": .integer(8),
            "position": .vector([0,0,0])])
    }
    func testPositiveOrdinaryPolicyStagesWithoutDedicatedViewportPost() async throws {
        let (api, host, _) = try await fixture()
        let next = try await api.preparePrivateHostArchitecture(command(host), in: host)
        XCTAssertEqual(next.candidate["rooms"].items![0]["name"], .string("Ordinary edit"))
        XCTAssertEqual(PrivateHostPolicyProtocol.posts(), 0)
        XCTAssertEqual(host.document.savedEditVersion, host.document.draft!.document["editVersion"].integer)
    }
    func testBoundedPolicyRetainsDedicatedValidation() async throws {
        let (api, host, _) = try await fixture("bounded")
        _ = try await api.preparePrivateHostArchitecture(command(host), in: host)
        XCTAssertEqual(PrivateHostPolicyProtocol.posts(), 1)
    }
    func testUnknownReadOnlyStaleAndMismatchedPoliciesDoNotStage() async throws {
        for mode in ["unknown", "missing-policy", "read-only", "stale-policy", "wrong-policy-id"] {
            let (api, host, _) = try await fixture(mode)
            let before = try NativePrivateHost.bytes(host.document)
            do { _ = try await api.preparePrivateHostArchitecture(command(host), in: host); XCTFail(mode) } catch {}
            XCTAssertEqual(try NativePrivateHost.bytes(host.document), before)
            XCTAssertEqual(PrivateHostPolicyProtocol.posts(), 0)
        }
    }
    func testLateHeadOrPolicyDriftDoNotStage() async throws {
        for mode in ["head-drift", "policy-drift"] {
            let (api, host, _) = try await fixture(mode)
            let before = try NativePrivateHost.bytes(host.document)
            do { _ = try await api.preparePrivateHostArchitecture(command(host), in: host); XCTFail(mode) } catch {}
            XCTAssertEqual(try NativePrivateHost.bytes(host.document), before)
        }
    }
    func testComparedOrdinaryRemoteOpenPreservesProtectedContext() async throws {
        let (api, host, document) = try await fixture()
        let opened = try await api.openPrivateHost(exhibitionId: host.document.draft!.id, revision: 1, etag: etag,
            requestId: UUID().uuidString.lowercased(), protecting: document)
        XCTAssertEqual(opened.document.draft, host.document.draft)
        XCTAssertEqual(opened.document.savedEditVersion, 1)
        XCTAssertNoThrow(try opened.checkProtected(document))
        var changed = document; changed.revision += 1
        XCTAssertThrowsError(try opened.checkProtected(changed))
        XCTAssertEqual(PrivateHostPolicyProtocol.posts(), 0)
    }
    func testRemoteOpenRejectsWrongNamespaceAndSelectedOfficialId() async throws {
        let (api, host, document) = try await fixture()
        let foreign = NativeDocumentState(format: 1, revision: document.revision, workspaceId: document.workspaceId,
            tenantId: document.tenantId, principalId: "22222222-2222-4222-8222-222222222222", draft: document.draft,
            etag: document.etag, savedEditVersion: document.savedEditVersion, pending: document.pending)
        for (id, state) in [(host.document.draft!.id, foreign), (document.draft!.id, document)] {
            do { _ = try await api.openPrivateHost(exhibitionId: id, revision: 1, etag: etag,
                requestId: UUID().uuidString.lowercased(), protecting: state); XCTFail() } catch {}
        }
        XCTAssertEqual(PrivateHostPolicyProtocol.resourceReads(), 0)
    }
    func testRemoteOpenRejectsWrongKindHeadAndUnavailablePolicy() async throws {
        for mode in ["wrong-kind", "foreign-resource", "bounded", "unknown", "missing-policy", "read-only", "head-drift", "policy-drift", "wrong-policy-id"] {
            let (api, host, document) = try await fixture(mode)
            do { _ = try await api.openPrivateHost(exhibitionId: host.document.draft!.id, revision: 1, etag: etag,
                requestId: UUID().uuidString.lowercased(), protecting: document); XCTFail(mode) } catch {}
            XCTAssertEqual(PrivateHostPolicyProtocol.posts(), 0)
        }
        let (api, host, document) = try await fixture()
        do { _ = try await api.openPrivateHost(exhibitionId: host.document.draft!.id, revision: 2, etag: etag,
            requestId: UUID().uuidString.lowercased(), protecting: document); XCTFail() } catch {}
    }
}
private final class PrivateHostPolicyProtocol: URLProtocol, @unchecked Sendable {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var remote: DocumentValue = .null
    nonisolated(unsafe) private static var id = "", etag = "", mode = "", owner = "", reads = 0, policies = 0, writes = 0
    static func configure(remote: DocumentValue, id: String, etag: String, mode: String, owner: String) {
        lock.lock(); defer { lock.unlock() }; Self.remote = remote; Self.id = id; Self.etag = etag; Self.mode = mode; Self.owner = owner
        reads = 0; policies = 0; writes = 0
    }
    static func posts() -> Int { lock.lock(); defer { lock.unlock() }; return writes }
    static func resourceReads() -> Int { lock.lock(); defer { lock.unlock() }; return reads }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        Self.lock.lock()
        var body = Self.remote, status = 200
        if (Self.mode == "missing-policy" && request.url!.path.hasSuffix("/native-viewport-policy"))
            || (Self.mode == "foreign-resource" && request.url!.path.contains("/exhibitions/")) {
            status = Self.mode == "foreign-resource" ? 403 : 404
            body = .object(["code": .string(Self.mode == "foreign-resource" ? "FORBIDDEN" : "NOT_FOUND")])
        } else if request.url!.path == "/api/v1/auth/session" {
            body = .object(["tenantId": .string(Self.owner), "userId": .string(Self.owner), "role": .string("admin"),
                "csrfToken": .string("synthetic-csrf"), "expiresAt": .string("2099-01-01T00:00:00Z")])
        } else if request.httpMethod == "POST" {
            Self.writes += 1; body = .object(["valid": .bool(true)])
        } else if request.url!.path.hasSuffix("/native-viewport-policy") {
            Self.policies += 1
            let bounded = Self.mode == "bounded" || (Self.mode == "policy-drift" && Self.policies > 1)
            body = .object(["exhibitionId": .string(Self.mode == "wrong-policy-id" ? Self.owner : Self.id),
                "revision": .integer(1), "etag": .string(Self.mode == "stale-policy" ? "stale" : Self.etag),
                "boundedReadOnly": .bool(Self.mode == "read-only"), "boundedEditRequired": .bool(false)])
            if Self.mode != "unknown" { body["boundedProfile"] = .bool(bounded) }
        } else {
            Self.reads += 1
            if Self.mode == "head-drift" && Self.reads > 1 {
                body["revision"] = .integer(2)
                body["etag"] = .string("\"studio-r2-" + String(repeating: "b", count: 64) + "\"")
            }
            if Self.mode == "wrong-kind" { body["draft"]["candidate"]["kind"] = .string("wrong-kind") }
        }
        let data = try! JSONEncoder().encode(body)
        let responseETag = body["etag"].string ?? Self.etag
        Self.lock.unlock()
        let response = HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: nil,
            headerFields: ["Content-Type": "application/json", "ETag": responseETag])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
