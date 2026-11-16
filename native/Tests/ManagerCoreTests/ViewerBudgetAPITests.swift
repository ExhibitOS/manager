// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class ViewerBudgetAPITests: XCTestCase {
    private func document(dirty: Bool = false) throws -> NativeDocumentState {
        var draft = try NativeExhibitionDraft.new()
        var candidate = draft.candidate
        candidate["extensions"] = .object(["org.example/opaque": .object(["preserve": .string("exact")])])
        try draft.replaceCandidate(candidate)
        return NativeDocumentState(format: 1, revision: 648,
            workspaceId: "11111111-1111-4111-8111-111111111111", tenantId: "22222222-2222-4222-8222-222222222222",
            principalId: "33333333-3333-4333-8333-333333333333", draft: draft,
            etag: "\"studio-r213-" + String(repeating: "a", count: 64) + "\"",
            savedEditVersion: draft.document["editVersion"].integer! - (dirty ? 1 : 0), pending: nil)
    }
    private var command: DocumentValue {
        .object(["type": .string("set-viewer-budget-policy"), "policy": .object([
            "version": .integer(1), "policyVersion": .integer(1), "targetProfile": .string("compact"),
            "profiles": .object(["compact": .object(["admission": .object(["residentBytes": .integer(123456789)])])])])])
    }
    private func connect(_ state: NativeDocumentState, mode: String = "valid") async throws -> AuthoringAPI {
        ViewerBudgetAPIProtocol.configure(state, mode: mode)
        let api = try AuthoringAPI(port: 18091, tenantId: state.tenantId,
            nativeRuntimeManifestSHA256: String(repeating: "a", count: 64), protocolClasses: [ViewerBudgetAPIProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: state.tenantId, principalId: state.principalId,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        return api
    }
    func testPreparedInspectionUsesExactAuthenticatedReadAndRejectsStaleOrUnnamedAnalysis() async throws {
        let input = try ViewerBudgetPublicationRequest(publicationId: "44444444-4444-4444-8444-444444444444",
                                                        revisionSha256: String(repeating: "a", count: 64))
        let state = try document()
        let api = try await connect(state)
        let analysis = try await api.publicationViewerBudget(input)
        XCTAssertEqual(analysis["estimateTarget"]["selectionBudget"]["maxTriangles"], .integer(150_000))
        XCTAssertEqual(analysis["binding"]["projectionAssetsSHA256"], .string(String(repeating: "d", count: 64)))
        XCTAssertEqual(analysis["estimates"]["wholeExhibition"]["evaluation"]["status"], .string("admission-exceeded"))
        let call = try XCTUnwrap(ViewerBudgetAPIProtocol.calls().last)
        XCTAssertEqual(call.httpMethod, "GET")
        XCTAssertEqual(call.url?.path, "/api/v1/tenants/\(state.tenantId)/studio/publications/\(input.publicationId)/viewer-budget")
        XCTAssertEqual(call.url?.query, "revisionSha256=" + input.revisionSha256)
        XCTAssertNil(call.httpBody)
        for mode in ["wrong-publication", "wrong-output-revision", "draft-binding", "bad-input-hash", "bad-assets-hash",
                     "unnamed-target", "no-model-budget", "wrong-scenario-hints"] {
            let rejected = try await connect(state, mode: mode)
            do { _ = try await rejected.publicationViewerBudget(input); XCTFail(mode) }
            catch let failure as AuthoringFailure {
                XCTAssertEqual(failure.code, ["unnamed-target", "no-model-budget", "wrong-scenario-hints"].contains(mode)
                    ? "VIEWER_BUDGET_PRODUCER_UNAVAILABLE" : "VIEWER_BUDGET_ANALYSIS_STALE")
            }
        }
    }

    func testDirtyAnalysisPostsExactDraftAndServerCASWithoutSaveOrViewport() async throws {
        let state = try document(dirty: true), api = try await connect(state)
        let response = try await api.viewerBudgetAnalysis(in: state, requestedProfile: "compact")
        XCTAssertEqual(response["analysisComplete"], .bool(false))
        let calls = ViewerBudgetAPIProtocol.calls()
        let call = try XCTUnwrap(calls.first { $0.url?.path.hasSuffix("/viewer-budget-analysis") == true })
        XCTAssertEqual(call.httpMethod, "POST")
        XCTAssertEqual(call.value(forHTTPHeaderField: "If-Match"), state.etag)
        let body = try JSONDecoder().decode(DocumentValue.self, from: XCTUnwrap(call.httpBody))
        XCTAssertEqual(body["expectedRevision"], .integer(213))
        XCTAssertEqual(body["draft"], state.draft?.document)
        XCTAssertFalse(calls.contains { $0.httpMethod == "PUT" || $0.url?.path.hasSuffix("/native-viewport-check") == true })
    }
    func testPolicyRejectsMissingCapabilityForeignOwnerDirtyAndPendingBeforeProposal() async throws {
        for mode in ["missing-profile", "unknown-profile", "foreign-owner", "dirty", "pending"] {
            var state = try document(dirty: mode == "dirty")
            let api = try await connect(state, mode: mode)
            if mode == "foreign-owner" {
                state = NativeDocumentState(format: state.format, revision: state.revision, workspaceId: state.workspaceId,
                    tenantId: state.tenantId, principalId: UUID().uuidString.lowercased(), draft: state.draft,
                    etag: state.etag, savedEditVersion: state.savedEditVersion, pending: nil)
            }
            if mode == "pending" { state.pending = .init(etag: state.etag, draft: try XCTUnwrap(state.draft)) }
            do { _ = try await api.prepareViewerBudgetPolicy(command, in: state); XCTFail("Expected refusal: \(mode)") } catch { }
            XCTAssertFalse(ViewerBudgetAPIProtocol.calls().contains { $0.httpMethod == "POST" }, mode)
        }
    }
    func testProtectedPolicyPreparationChangesOnlyNamespaceAndNeverSaves() async throws {
        let state = try document(), api = try await connect(state)
        let request = try await api.prepareViewerBudgetPolicy(command, in: state)
        let changed = try request.apply(to: XCTUnwrap(state.draft))
        XCTAssertEqual(changed.candidate["extensions"]["org.example/opaque"], state.draft?.candidate["extensions"]["org.example/opaque"])
        XCTAssertEqual(changed.candidate["extensions"][ViewerBudgetSettings.namespace], command["policy"])
        let calls = ViewerBudgetAPIProtocol.calls()
        XCTAssertEqual(calls.filter { $0.url?.path.hasSuffix("/native-authoring-proposal") == true }.count, 1)
        XCTAssertFalse(calls.contains { $0.httpMethod == "PUT" || $0.url?.path.hasSuffix("/native-viewport-check") == true })
        let post = try XCTUnwrap(calls.first { $0.httpMethod == "POST" })
        XCTAssertEqual(post.value(forHTTPHeaderField: "If-Match"), state.etag)
        let body = try JSONDecoder().decode(DocumentValue.self, from: XCTUnwrap(post.httpBody))
        XCTAssertEqual(body["expectedRevision"], .integer(213))
        XCTAssertEqual(body["request"]["type"], .string("set-viewer-budget-policy"))
    }
    func testStaleHeadsTamperingAndReplyETagCannotBecomeAcceptedAnalysisOrPolicy() async throws {
        for mode in ["bad-etag", "stale-analysis"] {
            let state = try document(dirty: true), api = try await connect(state, mode: mode)
            do { _ = try await api.viewerBudgetAnalysis(in: state, requestedProfile: "compact"); XCTFail(mode) } catch { }
            XCTAssertFalse(ViewerBudgetAPIProtocol.calls().contains { $0.httpMethod == "PUT" })
        }
        for mode in ["bad-etag", "tampered-policy", "stale-final-head"] {
            let state = try document(), api = try await connect(state, mode: mode)
            do { _ = try await api.prepareViewerBudgetPolicy(command, in: state); XCTFail(mode) } catch { }
            XCTAssertFalse(ViewerBudgetAPIProtocol.calls().contains { $0.httpMethod == "PUT" })
        }
    }

    func testPolicyPendingInspectionRetainsOriginalIntentBeforeOrAfterCommitAndRejectsConflict() async throws {
        let source = try document(), api = try await connect(source)
        let proposal = try await api.prepareViewerBudgetPolicy(command, in: source)
        let changed = try proposal.apply(to: XCTUnwrap(source.draft))
        var pending = source; pending.draft = changed
        let requestId = UUID().uuidString.lowercased()
        pending.pending = .init(requestId: requestId, etag: source.etag, draft: changed, origin: ViewerBudgetPolicyRequest.pendingOrigin)
        try await api.validatePendingViewerBudgetPolicy(in: pending)
        var committed = pending; committed.pending = nil
        committed.savedEditVersion = changed.document["editVersion"].integer
        committed.etag = "\"studio-r214-" + String(repeating: "d", count: 64) + "\""
        ViewerBudgetAPIProtocol.configure(committed, mode: "valid")
        try await api.validatePendingViewerBudgetPolicy(in: pending)
        XCTAssertEqual(pending.pending?.requestId, requestId)
        XCTAssertEqual(pending.pending?.etag, source.etag)
        XCTAssertFalse(ViewerBudgetAPIProtocol.calls().contains { $0.httpMethod != "GET" })
        var foreign = changed.document; foreign["candidate"]["title"] = .string("Another editor")
        committed.draft = try NativeExhibitionDraft(document: foreign)
        ViewerBudgetAPIProtocol.configure(committed, mode: "valid")
        do { try await api.validatePendingViewerBudgetPolicy(in: pending); XCTFail("Changed remote head must conflict") } catch { }
        XCTAssertEqual(pending.pending?.requestId, requestId)
        XCTAssertFalse(ViewerBudgetAPIProtocol.calls().contains { $0.httpMethod != "GET" })
    }
}

private final class ViewerBudgetAPIProtocol: URLProtocol, @unchecked Sendable {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var source: NativeDocumentState?
    nonisolated(unsafe) private static var mode = "valid"
    nonisolated(unsafe) private static var requests: [URLRequest] = []
    nonisolated(unsafe) private static var savedReads = 0
    static func configure(_ state: NativeDocumentState, mode: String) {
        lock.lock(); defer { lock.unlock() }
        source = state; Self.mode = mode; requests = []; savedReads = 0
    }
    static func calls() -> [URLRequest] { lock.lock(); defer { lock.unlock() }; return requests }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func stopLoading() {}
    override func startLoading() {
        do {
            let (data, etag) = try Self.reply(to: request)
            var headers = ["Content-Type": "application/json", "Cache-Control": "no-store"]
            if let etag { headers["ETag"] = etag }
            let response = HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: "HTTP/1.1", headerFields: headers)!
            client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
            client?.urlProtocol(self, didLoad: data)
            client?.urlProtocolDidFinishLoading(self)
        } catch { client?.urlProtocol(self, didFailWithError: error) }
    }
    private static func reply(to request: URLRequest) throws -> (Data, String?) {
        lock.lock(); defer { lock.unlock() }
        let state = try XCTUnwrap(source), draft = try XCTUnwrap(state.draft)
        var bytes = request.httpBody
        if bytes == nil, let stream = request.httpBodyStream {
            stream.open(); defer { stream.close() }
            var collected = Data(), buffer = [UInt8](repeating: 0, count: 4096)
            while true {
                let count = stream.read(&buffer, maxLength: buffer.count)
                if count <= 0 { break }
                collected.append(contentsOf: buffer.prefix(count))
            }
            bytes = collected
        }
        var recorded = request; recorded.httpBody = bytes; requests.append(recorded)
        let path = request.url!.path
        if path == "/api/v1/auth/session" {
            var session: DocumentValue = .object(["tenantId": .string(state.tenantId), "userId": .string(state.principalId),
                "role": .string("admin"), "csrfToken": .string("synthetic-csrf"), "expiresAt": .string("2099-01-01T00:00:00Z")])
            if mode != "missing-profile" { session["nativeViewerBudgetPolicyProfile"] = .string(mode == "unknown-profile" ? "unknown" : "viewer-budget-policy-v1") }
            return (try JSONEncoder().encode(session), nil)
        }
        if path.contains("/studio/publications/"), path.hasSuffix("/viewer-budget") {
            let publicationId = "44444444-4444-4444-8444-444444444444"
            let revision = String(repeating: "a", count: 64)
            var answer: DocumentValue = .object(["version": .integer(1), "policyVersion": .integer(1),
                "accountingVersion": .integer(1), "analysisComplete": .bool(true), "policySHA256": .string(String(repeating: "b", count: 64)),
                "binding": .object(["source": .string("publication"), "publicationId": .string(publicationId), "revisionSha256": .string(revision),
                    "inputSHA256": .string(revision), "projectionAssetsSHA256": .string(String(repeating: "d", count: 64))]),
                "selection": .object(["effectiveProfile": .string("standard")]),
                "resolvedPolicy": .object(["profile": .string("standard"), "policyVersion": .integer(1), "accountingVersion": .integer(1)]),
                "exportConfiguration": .object(["performanceAdvisoryOnly": .bool(true)]),
                "estimates": .object(["wholeExhibition": .object(["evaluation": .object(["profile": .string("standard"),
                    "scope": .string("wholeExhibition"), "status": .string("admission-exceeded")])])]),
                "estimateTarget": .object(["scenario": .string("fine-pointer-no-memory-hint-auto-v1"), "requestedMode": .string("auto"),
                    "hints": .object(["finePointer": .bool(true), "coarsePointer": .bool(false)]),
                    "selectionBudget": .object(["maxTriangles": .integer(150_000), "maxTextureSize": .integer(2048)])])])
            switch mode {
            case "wrong-publication": answer["binding"]["publicationId"] = .string(UUID().uuidString.lowercased())
            case "wrong-output-revision": answer["binding"]["revisionSha256"] = .string(String(repeating: "b", count: 64))
            case "draft-binding": answer["binding"]["source"] = .string("draft")
            case "bad-input-hash": answer["binding"]["inputSHA256"] = .string(String(repeating: "e", count: 64))
            case "bad-assets-hash": answer["binding"]["projectionAssetsSHA256"] = .string("invalid")
            case "unnamed-target": answer["estimateTarget"] = .null
            case "no-model-budget": answer["estimateTarget"]["selectionBudget"]["maxTextureSize"] = .null
            case "wrong-scenario-hints": answer["estimateTarget"]["hints"]["finePointer"] = .bool(false)
            default: break
            }
            return (try JSONEncoder().encode(answer), nil)
        }
        let responseETag = mode == "bad-etag" ? "\"unexpected\"" : state.etag
        if path.hasSuffix("/viewer-budget-analysis") {
            let input = try JSONDecoder().decode(DocumentValue.self, from: XCTUnwrap(bytes))
            let supplied = input["draft"], profile = input["options"]["profile"].string ?? "standard"
            var response: DocumentValue = .object(["version": .integer(1), "policyVersion": .integer(1), "accountingVersion": .integer(1),
                "binding": .object(["exhibitionId": .string(draft.id), "savedRevision": .integer(213), "etag": .string(state.etag!),
                    "draftId": supplied["id"], "draftEditVersion": supplied["editVersion"], "source": .string("draft"),
                    "inputSHA256": .string(String(repeating: "b", count: 64))]),
                "selection": .object(["effectiveProfile": .string(profile)]), "policySHA256": .string(String(repeating: "c", count: 64)),
                "resolvedPolicy": .object(["profile": .string(profile), "policyVersion": .integer(1), "accountingVersion": .integer(1)]),
                "estimates": .object(["wholeExhibition": .object(["evaluation": .object(["profile": .string(profile),
                    "scope": .string("wholeExhibition"), "status": .string("unknown")])])]), "analysisComplete": .bool(false)])
            if mode == "stale-analysis" { response["binding"]["draftEditVersion"] = .integer(-1) }
            return (try JSONEncoder().encode(response), responseETag)
        }
        if path.hasSuffix("/native-authoring-proposal") {
            let input = try JSONDecoder().decode(DocumentValue.self, from: XCTUnwrap(bytes))["request"]
            var candidate = draft.candidate
            candidate["extensions"][ViewerBudgetSettings.namespace] = input["policy"]
            if mode == "tampered-policy" { candidate["title"] = .string("Unrequested mutation") }
            var response: [String: DocumentValue] = [:]
            for key in ["version", "epoch", "sequence", "tenantId", "principalId", "requestId"] { response[key] = input[key] }
            response["candidate"] = candidate
            return (try JSONEncoder().encode(DocumentValue.object(response)), responseETag)
        }
        if path.hasSuffix("/" + draft.id) {
            savedReads += 1
            let etag = mode == "stale-final-head" && savedReads > 1 ? "\"studio-r214-" + String(repeating: "d", count: 64) + "\"" : state.etag!
            let response: DocumentValue = .object(["revision": .integer(etag.hasPrefix("\"studio-r214-") ? 214 : 213),
                "etag": .string(etag), "draft": draft.document])
            return (try JSONEncoder().encode(response), mode == "bad-etag" ? responseETag : etag)
        }
        throw URLError(.unsupportedURL)
    }
}
