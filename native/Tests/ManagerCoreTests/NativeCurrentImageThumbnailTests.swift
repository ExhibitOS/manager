// SPDX-License-Identifier: Apache-2.0
import XCTest
import CryptoKit
import CoreGraphics
import ImageIO
@testable import ManagerCore

final class NativeCurrentImageThumbnailTests: XCTestCase {
    private let artworkID = "11111111-1111-4111-8111-111111111111"
    private let sourceID = "22222222-2222-4222-8222-222222222222"
    private let thumbnailID = "33333333-3333-4333-8333-333333333333"
    private let jobID = "44444444-4444-4444-8444-444444444444"
    private let workspace = "55555555-5555-4555-8555-555555555555"
    private let principal = "66666666-6666-4666-8666-666666666666"
    private let sourceHash = String(repeating: "a", count: 64)
    private let outputHash = String(repeating: "b", count: 64)
    private func hash<T: Encodable>(_ value: T) throws -> String {
        let e = JSONEncoder(); e.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        return try SHA256.hash(data: e.encode(value)).map { String(format: "%02x", $0) }.joined()
    }
    private func artwork() -> NativeArtwork {
        var metadata = ArtworkMetadata(); metadata.rights.permissions.display = true; metadata.rights.permissions.export = true
        return NativeArtwork(id: artworkID, artistId: "77777777-7777-4777-8777-777777777777", revision: 3,
            approvedRevision: 3, approvedAssetId: sourceID, metadata: metadata,
            assets: [NativeArtwork.Asset(id: sourceID, mime: "image/png", state: "approved", sha256: sourceHash, bytes: 1000)], archived: false)
    }
    private func ready() throws -> DocumentValue {
        let source: DocumentValue = .object(["assetId": .string(sourceID), "sha256": .string(sourceHash), "bytes": .integer(1000), "mime": .string("image/png")])
        let thumbnail: DocumentValue = .object(["assetId": .string(thumbnailID), "sha256": .string(outputHash), "bytes": .integer(100), "mime": .string("image/png"), "width": .integer(256), "height": .integer(128)])
        let producer: DocumentValue = .object(["version": .integer(1), "recipeSHA256": .string("d13ad48b1ae8c29818002225fb5ebb4840907646810d90922a4ea3a5448f06c0"),
            "decoder": .string("sharp-0.35.4-vips-8.18.6-mozjpeg-0826579"),
            "source": .object(["mime": .string("image/png"), "sha256": .string(sourceHash), "bytes": .integer(1000), "width": .integer(512), "height": .integer(256), "iccSHA256": .null]),
            "full": .object(["sha256": .string(String(repeating:"c",count:64)), "bytes": .integer(900), "width": .integer(512), "height": .integer(256)])])
        var receipt: DocumentValue = .object(["version": .integer(1), "profile": .string(NativeCurrentImageThumbnailJob.profile),
            "jobId": .string(jobID), "artworkId": .string(artworkID), "artworkRevision": .integer(3),
            "workspaceId": .string(workspace), "principalId": .string(principal),
            "rightsSHA256": .string(try hash(artwork().metadata!.rights)), "source": source,
            "thumbnail": thumbnail, "producer": producer])
        receipt["sha256"] = .string(try hash(receipt))
        let selection: DocumentValue = .object(["expectedRevision": .integer(3), "sourceAssetId": .string(sourceID),
            "sourceSha256": .string(sourceHash), "thumbnailAssetId": .string(thumbnailID),
            "thumbnailSha256": .string(outputHash), "view": .object(["kind": .string("default")])])
        return .object(["id": .string(jobID), "state": .string("approved"), "progress": .integer(100), "attempts": .integer(1),
            "errorCode": .null, "assetId": .string(thumbnailID), "retryAt": .null, "originalAssetId": .null,
            "workerDrainVersion": .integer(1), "workerDrained": .bool(true),
            "thumbnailPreparation": .object(["profile": .string(NativeCurrentImageThumbnailJob.profile), "artworkRevision": .integer(3),
                "source": source, "status": .string("ready"), "workerDrained": .bool(true), "serverReceipt": receipt, "selectionCandidate": selection])])
    }
    private func decode(_ wire: DocumentValue) throws -> NativeCurrentImageThumbnailJob {
        try JSONDecoder().decode(NativeCurrentImageThumbnailJob.self, from: JSONEncoder().encode(wire))
    }
    private func resign(_ value: inout DocumentValue) throws {
        guard case .object(var fields) = value["thumbnailPreparation"]["serverReceipt"] else { XCTFail(); return }
        fields.removeValue(forKey: "sha256"); let body = DocumentValue.object(fields)
        value["thumbnailPreparation"]["serverReceipt"]["sha256"] = .string(try hash(body))
    }
    @MainActor
    func testGenericSelectionResponseAndAuthoritativeReadback() async throws {
        let (api, document, review) = try await selectionFixture(mode: "normal")
        let result = try await api.selectCurrentImageThumbnail(review, requestId: UUID(), expectedSelectionVersion: 0, in: document)
        XCTAssertTrue(result.selected); XCTAssertEqual(result.selectionVersion, 1)
        XCTAssertEqual(ThumbnailSelectionProtocol.posts(), 1)
    }
    @MainActor
    func testLostSelectionReplyRequiresExplicitSameRequestReplay() async throws {
        let (api, document, review) = try await selectionFixture(mode: "lose-once")
        let id = UUID()
        do { _ = try await api.selectCurrentImageThumbnail(review, requestId: id, expectedSelectionVersion: 0, in: document); XCTFail() }
        catch { XCTAssertEqual(ThumbnailSelectionProtocol.posts(), 1) }
        let result = try await api.selectCurrentImageThumbnail(review, requestId: id, expectedSelectionVersion: 0, in: document)
        XCTAssertTrue(result.selected); XCTAssertEqual(ThumbnailSelectionProtocol.posts(), 2)
    }
    @MainActor
    func testWrongSelectedImagePreservesUncertainMutationWithoutAutomaticRetry() async throws {
        let (api, document, review) = try await selectionFixture(mode: "wrong-selected-image")
        do { _ = try await api.selectCurrentImageThumbnail(review, requestId: UUID(), expectedSelectionVersion: 0, in: document); XCTFail() }
        catch { XCTAssertEqual(ThumbnailSelectionProtocol.posts(), 1) }
    }
    @MainActor
    private func selectionFixture(mode: String) async throws -> (AuthoringAPI, NativeDocumentState, NativeCurrentImageThumbnailReview) {
        let pixels = Data(repeating: 128, count: 256 * 128 * 4)
        let provider = try XCTUnwrap(CGDataProvider(data: pixels as CFData))
        let image = try XCTUnwrap(CGImage(width: 256, height: 128, bitsPerComponent: 8, bitsPerPixel: 32,
            bytesPerRow: 1024, space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
        let output = NSMutableData()
        let destination = try XCTUnwrap(CGImageDestinationCreateWithData(output, "public.png" as CFString, 1, nil))
        CGImageDestinationAddImage(destination, image, nil); XCTAssertTrue(CGImageDestinationFinalize(destination))
        let png = output as Data, digest = nativeImportSHA256(png)
        var wire = try ready()
        wire["thumbnailPreparation"]["serverReceipt"]["thumbnail"]["sha256"] = .string(digest)
        wire["thumbnailPreparation"]["serverReceipt"]["thumbnail"]["bytes"] = .integer(Int64(png.count))
        wire["thumbnailPreparation"]["selectionCandidate"]["thumbnailSha256"] = .string(digest)
        try resign(&wire)
        ThumbnailSelectionProtocol.configure(job: try JSONEncoder().encode(wire), artwork: try JSONEncoder().encode(artwork()),
            png: png, tenant: workspace, principal: principal, mode: mode)
        let api = try AuthoringAPI(port: 18091, tenantId: workspace,
            nativeRuntimeManifestSHA256: "0f04520a8742f9774206d12c8a2b56da8ed2a4e07c095b3cb8ad3b3e988dd988",
            protocolClasses: [ThumbnailSelectionProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: workspace, principalId: principal,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        let draft = try NativeExhibitionDraft.new(title: "Synthetic selection")
        let document = NativeDocumentState(format: 1, revision: 304, workspaceId: workspace, tenantId: workspace,
            principalId: principal, draft: draft, etag: nil, savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        let review = try await api.reviewCurrentImageThumbnail(artworkId: artworkID, revision: 3, jobId: jobID, in: document)
        return (api, document, review)
    }
    func testReadyBindsCurrentApprovedSourceAndOwners() throws {
        let job = try decode(ready())
        XCTAssertNoThrow(try job.check(artwork: artwork(), workspaceId: workspace, principalId: principal, jobId: jobID))
        let roundTrip = try JSONDecoder().decode(NativeCurrentImageThumbnailJob.self, from: JSONEncoder().encode(job))
        XCTAssertNoThrow(try roundTrip.check(artwork: artwork(), workspaceId: workspace, principalId: principal, jobId: jobID))
        XCTAssertThrowsError(try job.check(artwork: artwork(), workspaceId: workspace, principalId: "77777777-7777-4777-8777-777777777777", jobId: jobID))
        XCTAssertThrowsError(try job.check(artwork: artwork(), workspaceId: workspace, principalId: principal, jobId: "77777777-7777-4777-8777-777777777777"))
        let a = artwork()
        let stale = NativeArtwork(id:a.id, artistId:a.artistId, revision:4, approvedRevision:3, approvedAssetId:a.approvedAssetId, metadata:a.metadata, assets:a.assets, archived:false)
        XCTAssertThrowsError(try job.check(artwork: stale, workspaceId: workspace, principalId: principal, jobId: jobID))
    }
    func testClosedReceiptTamperingAndContradictoryCandidateFail() throws {
        var value = try ready(); value["thumbnailPreparation"]["serverReceipt"]["sha256"] = .string(String(repeating:"0",count:64))
        XCTAssertThrowsError(try decode(value))
        value = try ready(); value["thumbnailPreparation"]["serverReceipt"]["unexpected"] = .bool(true)
        XCTAssertThrowsError(try decode(value))
        value = try ready(); value["thumbnailPreparation"]["workerDrained"] = .bool(false)
        XCTAssertThrowsError(try decode(value))
        value = try ready(); value["thumbnailPreparation"]["selectionCandidate"]["thumbnailSha256"] = .string(sourceHash)
        let job = try decode(value)
        XCTAssertThrowsError(try job.check(artwork: artwork(), workspaceId: workspace, principalId: principal, jobId: jobID))
        value = try ready(); value["thumbnailPreparation"]["serverReceipt"]["thumbnail"]["width"] = .integer(257); try resign(&value)
        let tooLarge = try decode(value)
        XCTAssertThrowsError(try tooLarge.check(artwork: artwork(), workspaceId: workspace, principalId: principal, jobId: jobID))
        value = try ready(); value["thumbnailPreparation"]["serverReceipt"]["producer"]["source"]["sha256"] = .string(outputHash); try resign(&value)
        let wrongSource = try decode(value)
        XCTAssertThrowsError(try wrongSource.check(artwork: artwork(), workspaceId: workspace, principalId: principal, jobId: jobID))
        value = try ready(); value["thumbnailPreparation"]["serverReceipt"]["workspaceId"] = .string("77777777-7777-4777-8777-777777777777"); try resign(&value)
        let foreign = try decode(value)
        XCTAssertThrowsError(try foreign.check(artwork: artwork(), workspaceId: workspace, principalId: principal, jobId: jobID))
        value = try ready(); value["thumbnailPreparation"]["serverReceipt"]["rightsSHA256"] = .string(sourceHash); try resign(&value)
        let wrongRights = try decode(value)
        XCTAssertThrowsError(try wrongRights.check(artwork: artwork(), workspaceId: workspace, principalId: principal, jobId: jobID))
        value = try ready(); value["thumbnailPreparation"]["serverReceipt"]["thumbnail"]["assetId"] = .string(sourceID); try resign(&value)
        value["assetId"] = .string(sourceID); value["thumbnailPreparation"]["selectionCandidate"]["thumbnailAssetId"] = .string(sourceID)
        let sameAsset = try decode(value)
        XCTAssertThrowsError(try sameAsset.check(artwork: artwork(), workspaceId: workspace, principalId: principal, jobId: jobID))
    }
    func testPendingCannotSmuggleReadyAuthorityAndRequestIsClosed() throws {
        var value = try ready(); value["state"] = .string("queued"); value["progress"] = .integer(0)
        value["assetId"] = .null; value["workerDrained"] = .bool(false)
        value["thumbnailPreparation"]["status"] = .string("pending"); value["thumbnailPreparation"]["workerDrained"] = .bool(false)
        XCTAssertThrowsError(try decode(value))
        guard case .object(var p) = value["thumbnailPreparation"] else { XCTFail(); return }
        p.removeValue(forKey:"serverReceipt"); p.removeValue(forKey:"selectionCandidate")
        value["thumbnailPreparation"] = .object(p)
        let pending = try decode(value)
        XCTAssertNoThrow(try pending.check(artwork: artwork(), workspaceId: workspace, principalId: principal, jobId: jobID))
        value["state"] = .string("failed"); value["thumbnailPreparation"]["status"] = .string("failed")
        XCTAssertNoThrow(try decode(value))
        value["thumbnailPreparation"]["selectionCandidate"] = .object([:])
        XCTAssertThrowsError(try decode(value))
        let request = try NativeCurrentImageThumbnailRequest(requestId: jobID, expectedRevision: 3)
        let requestWire = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(request))
        guard case .object(let fields) = requestWire else { XCTFail(); return }
        XCTAssertEqual(Set(fields.keys), ["requestId","expectedRevision"])
        var forged = requestWire; forged["path"] = .string("/unexpected")
        XCTAssertThrowsError(try JSONDecoder().decode(NativeCurrentImageThumbnailRequest.self, from: JSONEncoder().encode(forged)))
    }
}

private final class ThumbnailSelectionProtocol: URLProtocol, @unchecked Sendable {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var job = Data(), artwork = Data(), png = Data()
    nonisolated(unsafe) private static var tenant = "", principal = "", mode = "", selected = false, count = 0
    static let binding = "88888888-8888-4888-8888-888888888888"
    static func configure(job: Data, artwork: Data, png: Data, tenant: String, principal: String, mode: String) {
        lock.lock(); defer { lock.unlock() }; Self.job = job; Self.artwork = artwork; Self.png = png
        Self.tenant = tenant; Self.principal = principal; Self.mode = mode; selected = false; count = 0
    }
    static func posts() -> Int { lock.lock(); defer { lock.unlock() }; return count }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        Self.lock.lock()
        let path = request.url!.path
        var data = Self.artwork, headers = ["Content-Type": "application/json"]
        var lose = false
        if path == "/api/v1/auth/session" {
            data = try! JSONSerialization.data(withJSONObject: ["tenantId": Self.tenant, "userId": Self.principal,
                "role": "admin", "csrfToken": "synthetic-csrf", "expiresAt": "2099-01-01T00:00:00Z"])
        } else if path.contains("/imports/") { data = Self.job }
        else if request.httpMethod == "POST" {
            Self.count += 1; Self.selected = true
            lose = Self.mode == "lose-once" && Self.count == 1
            data = try! JSONSerialization.data(withJSONObject: ["id": Self.binding, "selectionVersion": 1, "selected": true])
        } else if path.hasSuffix("/thumbnail-selection") {
            data = try! JSONSerialization.data(withJSONObject: ["selectionVersion": Self.selected ? 1 : 0,
                "bindingId": Self.selected ? Self.binding as Any : NSNull()])
        } else if path.contains("/assets/") || path.hasSuffix("/image") {
            data = Self.png
            if path.hasSuffix("/image") && Self.mode == "wrong-selected-image" { data[0] ^= 1 }
            headers = ["Content-Type": path.hasSuffix("/image") ? "image/png" : "application/octet-stream",
                "Content-Length": String(data.count), "x-exhibitos-sha256": nativeImportSHA256(Self.png)]
        }
        Self.lock.unlock()
        if lose { client?.urlProtocol(self, didFailWithError: URLError(.networkConnectionLost)); return }
        let response = HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: nil, headerFields: headers)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data); client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}
