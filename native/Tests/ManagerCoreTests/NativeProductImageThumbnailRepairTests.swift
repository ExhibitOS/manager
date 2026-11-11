// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class NativeProductImageThumbnailRepairTests: XCTestCase {
    private let artwork = "11111111-1111-4111-8111-111111111111"
    private let asset = "22222222-2222-4222-8222-222222222222"
    private let job = "33333333-3333-4333-8333-333333333333"
    private let tenant = "44444444-4444-4444-8444-444444444444"
    private let requestId = "55555555-5555-4555-8555-555555555555"
    private let revisionId = "66666666-6666-4666-8666-666666666666"
    private let preview = "77777777-7777-4777-8777-777777777777"
    private var hash: String { String(repeating: "a", count: 64) }
    private var etag: String { "\"studio-r4-\(hash)\"" }

    private func command(_ changes: [String: DocumentValue] = [:]) throws -> NativeAutomationReplacementCommand {
        let fields: [String: DocumentValue] = ["kind": .string(NativeProductImageThumbnailRepair.profile),
            "artworkId": .string(artwork), "artworkRevision": .integer(2), "importJobId": .string(job),
            "studioRevision": .integer(4), "etag": .string(etag)]
        return try .init(.object(["command": .object(fields.merging(changes) { _, new in new })]))
    }

    private func fixture() throws -> (NativeExhibitionDraft, NativeExhibitionDraft) {
        var document = try NativeExhibitionDraft.new().document
        let old: DocumentValue = .object(["id": .string(artwork), "artworkType": .string("image"),
            "revision": .integer(2), "revisionId": .string(revisionId), "createdAt": .string("2026-11-01T00:00:00Z"),
            "primaryAssetId": .string(asset), "rights": .object(["unchanged": .bool(true)]),
            "provenance": .object(["events": .array([])]),
            "assets": .array([.object(["id": .string(asset), "role": .string("image"), "mime": .string("image/png"),
                "sha256": .string(hash), "bytes": .integer(900)])]),
            "extensions": .object([NativeProductImageThumbnailRepair.cms: .object([
                "tenantId": .string(tenant), "artworkId": .string(artwork), "revision": .integer(2)]),
                "org.exhibitos.viewer/lod": .object(["preserved": .bool(true)]),
                "org.exhibitos.viewer/display-policy": .object(["preserved": .bool(true)])])])
        document["candidate"]["artworks"] = .array([old])
        document["candidate"]["placements"] = .array([.object(["id": .string(requestId),
            "artworkRevisionId": .string(revisionId), "assetId": .string(asset), "pose": NativeExhibitionDraft.identity])])
        let before = try NativeExhibitionDraft(document: document)
        var next = old
        next["revision"] = .integer(3); next["revisionId"] = .string(job); next["createdAt"] = .string("2026-11-11T00:00:00.001Z")
        next["assets"] = .array(old["assets"].items! + [.object(["id": .string(preview),
            "path": .string("assets/\(preview)/thumbnail.png"), "role": .string("preview"), "mime": .string("image/png"),
            "bytes": .integer(120), "sha256": .string(hash)])])
        next["extensions"][NativeProductImageThumbnailRepair.cms]["revision"] = .integer(3)
        next["extensions"][NativeProductImageThumbnailRepair.thumbnail] = .object(["version": .integer(1), "assetId": .string(preview)])
        next["provenance"]["events"] = .array([.object(["id": .string(job), "type": .string("edited"),
            "at": next["createdAt"], "description": .string("Generated first preview from the currently approved Full display image.")])])
        document["candidate"]["artworks"] = .array([next])
        var placements = before.placements
        placements[0]["artworkRevisionId"] = .string(job)
        document["candidate"]["placements"] = .array(placements)
        return (before, try NativeExhibitionDraft(document: document))
    }

    func testDedicatedClosedCommandAndExactFiveKeyWire() throws {
        let parsed = try command()
        guard case .productImageThumbnailRepair = parsed.target else { return XCTFail("Dedicated repair required") }
        let input = try NativeProductImageThumbnailRepair.input(command: parsed, requestId: requestId)
        guard case .object(let fields) = input else { return XCTFail("Object required") }
        XCTAssertEqual(Set(fields.keys), ["kind", "requestId", "artworkId", "artworkRevision", "importJobId"])
        XCTAssertEqual(input["artworkRevision"], .integer(2))
        for extra in ["receiptSha256", "coarseSha256", "placementId", "sourceHashException"] {
            XCTAssertThrowsError(try command([extra: .string(hash)]))
        }
        XCTAssertThrowsError(try command(["kind": .string("unknown-repair")]))
        XCTAssertThrowsError(try command(["etag": .string("\"studio-r5-\(hash)\"")]))
    }

    func testPersistedRepairAllowsOnlyFirstPreviewAndRevisionReferences() throws {
        let (before, after) = try fixture()
        XCTAssertNoThrow(try NativeProductImageThumbnailRepair.validateChange(before: before, after: after, command: command()))
        XCTAssertTrue(NativeProductImageThumbnailRepair.missingPreview(before.artworks[0]))
        // No rendered selection binding is installed. The embedded preview alone finishes repair.
        XCTAssertFalse(NativeProductImageThumbnailRepair.missingPreview(after.artworks[0]))
        for key in ["rights", "primaryAssetId", "metadata", "transform"] {
            var wire = after.document, arts = after.artworks
            arts[0][key] = .string("unexpected mutation"); wire["candidate"]["artworks"] = .array(arts)
            XCTAssertThrowsError(try NativeProductImageThumbnailRepair.validateChange(before: before,
                after: NativeExhibitionDraft(document: wire), command: command()))
        }
        for key in ["rooms", "lights", "extensions", "title"] {
            var wire = after.document; wire["candidate"][key] = key == "title" ? .string("changed") : .array([])
            if wire == after.document { continue }
            XCTAssertThrowsError(try NativeProductImageThumbnailRepair.validateChange(before: before,
                after: NativeExhibitionDraft(document: wire), command: command()))
        }
        var wire = after.document, placements = after.placements
        placements[0]["pose"] = .object(["position": .vector([1, 2, 3])]); wire["candidate"]["placements"] = .array(placements)
        XCTAssertThrowsError(try NativeProductImageThumbnailRepair.validateChange(before: before,
            after: NativeExhibitionDraft(document: wire), command: command()))
        wire = after.document; wire["editVersion"] = .integer(2)
        XCTAssertThrowsError(try NativeProductImageThumbnailRepair.validateChange(before: before,
            after: NativeExhibitionDraft(document: wire), command: command()))
    }

    func testRepairRequiresPreviouslyAdoptedProducerRevisionAsServerSpecifies() {
        XCTAssertTrue(NativeProductImageThumbnailRepair.acceptsProducerRevision(1, current: 2))
        XCTAssertTrue(NativeProductImageThumbnailRepair.acceptsProducerRevision(1, current: 6))
        for (producer, current) in [(2, 2), (3, 2), (0, 2), (1, 0), (1, 2_147_483_647)] {
            XCTAssertFalse(NativeProductImageThumbnailRepair.acceptsProducerRevision(producer, current: current))
        }
    }

    func testDurableRepairSlotRetainsUncertaintyAndReleasesOnlyExactCompletedRepair() throws {
        let root = FileManager.default.temporaryDirectory.resolvingSymlinksInPath()
            .appendingPathComponent("repair-journal-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false,
                                                attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: root) }
        let active = root.appendingPathComponent(".automation-artwork-replacement.json")
        let completed = root.appendingPathComponent(".automation-artwork-repair-completed-" + requestId + ".json")
        let parsed = try command()
        var attempt: DocumentValue = .object(["requestId": .string(requestId), "operation": .string("artwork-replace-reviewed"),
            "command": .object(["kind": .string(NativeProductImageThumbnailRepair.profile),
                "artworkId": .string(artwork), "artworkRevision": .integer(2), "importJobId": .string(job),
                "studioRevision": .integer(4), "etag": .string(parsed.etag)])])
        func write(_ value: DocumentValue, at url: URL) throws {
            try JSONEncoder().encode(value).write(to: url)
            try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
        }
        func release(_ expected: DocumentValue) throws -> Bool {
            try NativeAutomationSession.releaseCompletedRepairAttempt(at: active.path, expected: expected, completedAt: completed.path)
        }
        try write(attempt, at: active)
        let original = try Data(contentsOf: active)
        XCTAssertFalse(try release(attempt)) // Missing receipt is uncertainty, not permission to replace intent.
        XCTAssertEqual(try Data(contentsOf: active), original)
        var changed = attempt; changed["command"]["etag"] = .string("\"studio-r4-\(String(repeating: "b", count: 64))\"")
        var receipt: DocumentValue = .object(["attempt": changed, "localRevision": .integer(8),
            "etag": .string("\"studio-r5-\(hash)\"")])
        try write(receipt, at: completed)
        XCTAssertFalse(try release(attempt)) // Completion for different original If-Match cannot release this intent.
        changed = attempt; changed["requestId"] = .string(job)
        XCTAssertFalse(try release(changed))
        XCTAssertEqual(try Data(contentsOf: active), original)
        receipt["attempt"] = attempt
        try write(receipt, at: completed)
        let completedBytes = try Data(contentsOf: completed)
        XCTAssertTrue(try release(attempt))
        XCTAssertFalse(FileManager.default.fileExists(atPath: active.path))
        XCTAssertEqual(try Data(contentsOf: completed), completedBytes) // Exact original ID, payload and ETag retained.
        // A new slot may be written only after the completed slot was actually released.
        changed = attempt; changed["requestId"] = .string(job)
        try write(changed, at: active)
        XCTAssertFalse(try release(changed))
        XCTAssertEqual(try JSONDecoder().decode(DocumentValue.self, from: Data(contentsOf: active)), changed)
        // Even a receipt claiming completion must never rotate an ordinary replacement.
        attempt["command"] = .object(["artworkId": .string(artwork), "artworkRevision": .integer(2),
            "importJobId": .string(job), "studioRevision": .integer(4), "etag": .string(etag), "coarseSha256": .string(hash)])
        try write(attempt, at: active)
        receipt["attempt"] = attempt; try write(receipt, at: completed)
        let ordinary = try Data(contentsOf: active)
        XCTAssertFalse(try release(attempt))
        XCTAssertEqual(try Data(contentsOf: active), ordinary)
    }

    #if DEBUG
    func testPersistedPostReadbackAndUncertainReplayKeepOriginalWireWithoutDuplicateSave() async throws {
        let (before, after) = try fixture()
        let nextETag = "\"studio-r5-\(hash)\""
        let response = try JSONEncoder().encode(DocumentValue.object(["revision": .integer(5),
            "etag": .string(nextETag), "draft": after.document]))
        let session = try JSONSerialization.data(withJSONObject: ["tenantId": tenant, "userId": tenant,
            "role": "owner", "csrfToken": "fixture-csrf", "expiresAt": "2099-01-01T00:00:00Z",
            "modelPreparationProfile": "embedded-model-compact-20k-512-v1",
            "workspaceMaterialProfile": "workspace-material-intake-v1",
            "nativeProductImagePreparationProfile": NativeProductImagePreparationIntent.profileID,
            "nativeProductImageThumbnailRepairProfile": NativeProductImageThumbnailRepair.profile])
        RepairProtocol.configure(session: session, response: response, etag: nextETag)
        defer { RepairProtocol.reset() }
        let api = try AuthoringAPI(port: 18091, tenantId: tenant,
            nativeRuntimeManifestSHA256: "003a98c579e69e1b38a7cb96971f0872f15b8afd169d392a96ac94fe87be9f92",
            protocolClasses: [RepairProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: tenant, principalId: tenant,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        // First POST loses its reply; the retained command remains the sole retry input.
        do {
            _ = try await api.repairProductImageThumbnail(draft: before, command: command(), requestId: requestId, onOriginal: true)
            XCTFail("First transport must be uncertain")
        } catch {}
        let result = try await api.repairProductImageThumbnail(draft: before, command: command(), requestId: requestId, onOriginal: true)
        XCTAssertEqual(result.draft, after)
        // Crash after local adoption: original request/If-Match still retrieves the stored receipt.
        _ = try await api.repairProductImageThumbnail(draft: after, command: command(), requestId: requestId, onOriginal: false)
        let posts = RepairProtocol.requests().filter { $0.httpMethod == "POST" }
        XCTAssertEqual(posts.count, 3)
        XCTAssertEqual(Set(posts.compactMap { $0.value(forHTTPHeaderField: "If-Match") }), [etag])
        XCTAssertEqual(Set(posts.compactMap { $0.value(forHTTPHeaderField: "X-CSRF-Token") }), ["fixture-csrf"])
        XCTAssertFalse(RepairProtocol.requests().contains { $0.httpMethod == "PUT" })
        XCTAssertTrue(posts.allSatisfy { $0.url?.path.hasSuffix("/native-artwork-replacement") == true })
        XCTAssertEqual(RepairProtocol.bodies().count, 3)
        XCTAssertTrue(RepairProtocol.bodies().allSatisfy { $0 == RepairProtocol.bodies().first })
    }
    #endif
}

private final class RepairProtocol: URLProtocol, @unchecked Sendable {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var seen: [URLRequest] = []
    nonisolated(unsafe) private static var payloads: [Data] = []
    nonisolated(unsafe) private static var session = Data(), response = Data(), etag = ""
    static func configure(session: Data, response: Data, etag: String) {
        lock.lock(); defer { lock.unlock() }
        self.session = session; self.response = response; self.etag = etag; seen = []; payloads = []
    }
    static func reset() { configure(session: Data(), response: Data(), etag: "") }
    static func requests() -> [URLRequest] { lock.lock(); defer { lock.unlock() }; return seen }
    static func bodies() -> [Data] { lock.lock(); defer { lock.unlock() }; return payloads }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        Self.lock.lock()
        Self.seen.append(request)
        if request.httpMethod == "POST" {
            var bytes = request.httpBody ?? Data()
            if let stream = request.httpBodyStream {
                stream.open(); defer { stream.close() }
                var buffer = [UInt8](repeating: 0, count: 4096)
                while true { let n = stream.read(&buffer, maxLength: buffer.count); if n <= 0 { break }; bytes.append(contentsOf: buffer.prefix(n)) }
            }
            Self.payloads.append(bytes)
        }
        let uncertain = request.httpMethod == "POST" && Self.payloads.count == 1
        let isSession = request.url?.path == "/api/v1/auth/session"
        let data = isSession ? Self.session : Self.response
        let tag = Self.etag
        Self.lock.unlock()
        if uncertain { client?.urlProtocol(self, didFailWithError: URLError(.networkConnectionLost)); return }
        let headers = isSession ? ["Content-Type": "application/json"] : ["Content-Type": "application/json", "ETag": tag]
        let reply = HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: "HTTP/1.1", headerFields: headers)!
        client?.urlProtocol(self, didReceive: reply, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data)
        client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
