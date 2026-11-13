// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerCore

@MainActor
final class InstalledFurnishingThumbnailTests: XCTestCase {
    // Public CC0 furnishing descriptor; its digest also checks canonical JSON
    // interoperability with the issued distribution binding.
    private let descriptorJSON = #"{"id":"af88109b-27e2-4d10-a08c-3ddc0224451a","version":1,"kind":"artwork-frame","name":"얇은 황동 액자","model":{"mime":"model/gltf-binary","bytes":7892,"sha256":"4b14e45ccf14ed38eea90c50553733a0b97b1671b9ec377d900e5a947545f8b4"},"rights":{"holder":"ExhibitOS","ownership":"owner","licenseId":"CC0-1.0","creditLine":"ExhibitOS","permissions":{"display":true,"download":true,"export":true,"commercial":true}},"provenance":"ExhibitOS default furnishings, thin-brass-frame v1.","fit":{"version":1,"sourceSha256":"4b14e45ccf14ed38eea90c50553733a0b97b1671b9ec377d900e5a947545f8b4","sourceBytes":7892,"geometry":{"profile":"straight-miter-ring-v1","openingWidth":0.6,"openingHeight":0.8,"imagePlaneZ":-0.012}}}"#
    private let pngBase64 = "iVBORw0KGgoAAAANSUhEUgAAAQAAAAEACAYAAABccqhmAAABFUlEQVR4nO3BMQEAAADCoPVP7WsIoAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAeAMBPAABPO1TCQAAAABJRU5ErkJggg=="
    private struct Fixture {
        var descriptor: DocumentValue, metadata: DocumentValue, summary: WorkspaceFurnishingSummary
        let document: NativeDocumentState, image: Data
        var item: WorkspaceFurnishing { get throws { try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(descriptor)) } }
    }
    private func fixture() throws -> Fixture {
        let descriptor = try JSONDecoder().decode(DocumentValue.self, from: Data(descriptorJSON.utf8))
        let image = try XCTUnwrap(Data(base64Encoded: pngBase64))
        let metadata: DocumentValue = .object([
            "profile": .string(InstalledFurnishingThumbnail.profile),
            "source": .object(["furnishingId": descriptor["id"], "furnishingVersion": .integer(1),
                "modelSha256": descriptor["model"]["sha256"], "modelBytes": descriptor["model"]["bytes"],
                "descriptorSha256": .string("a4132652f6d7d9b6daf3bd1476bc9f207a81dc0256ef416d88a2f1c962fe974d"),
                "rightsSha256": .string("1d81e6735f0916b11466ab5c01f470b456296ec451f0c71a32ba7e2dc644a51c")]),
            "png": .object(["mime": .string("image/png"), "width": .integer(256), "height": .integer(256),
                "bytes": .integer(Int64(image.count)), "sha256": .string(nativeImportSHA256(image))]),
            "provenance": .object(["recipeId": .string(ViewportFurnishingThumbnailRequest.recipe),
                "toolId": .string(ViewportFurnishingThumbnailRequest.tool),
                "renderRequestId": .string("0d02fc63-52ed-43a1-846e-a87b11edef8d")])])
        let summary: DocumentValue = .object(["id": descriptor["id"], "version": .integer(1), "kind": descriptor["kind"], "name": descriptor["name"],
            "rightsAvailability": .object(["display": .bool(true), "download": .bool(true), "export": .bool(true), "commercial": .bool(true)]),
            "installedThumbnailProfile": .string(InstalledFurnishingThumbnail.profile)])
        let draft = try NativeExhibitionDraft.new()
        let document = NativeDocumentState(format: 1, revision: 4, workspaceId: UUID().uuidString.lowercased(),
            tenantId: UUID().uuidString.lowercased(), principalId: UUID().uuidString.lowercased(), draft: draft,
            etag: "synthetic-etag", savedEditVersion: draft.document["editVersion"].integer, pending: nil)
        return try Fixture(descriptor: descriptor, metadata: metadata,
            summary: JSONDecoder().decode(WorkspaceFurnishingSummary.self, from: JSONEncoder().encode(summary)), document: document, image: image)
    }
    private func plan(_ f: Fixture) throws -> InstalledCardProtocol.Plan {
        let session = ["tenantId": f.document.tenantId, "userId": f.document.principalId, "role": "admin",
            "csrfToken": "synthetic-csrf", "expiresAt": "2099-01-01T00:00:00Z",
            "workspaceFurnishingProfile": "workspace-furnishing-intake-v1", "workspaceFurnishingStoolProfile": "workspace-furnishing-stool-v1",
            "workspaceFurnishingRoomVisualProfile": "workspace-furnishing-room-visual-v1",
            "workspaceArtworkFrameProfile": "workspace-artwork-frame-v1",
            "workspaceArtworkFrameModelsProfile": "workspace-artwork-frame-models-v1", "workspaceFurnishingReusableFitProfile": "source-bound-fit-v1"]
        return try .init(session: JSONSerialization.data(withJSONObject: session), descriptors: [JSONEncoder().encode(f.descriptor)],
            metadata: JSONEncoder().encode(f.metadata), image: f.image, digest: nativeImportSHA256(f.image))
    }
    private func connect(_ f: Fixture, manifest: String = "7e4baad682f01ae4896b051aa24b7fc93cc971fb57d85e689e570c5a808ca902") async throws -> AuthoringAPI {
        let api = try AuthoringAPI(port: 18091, tenantId: f.document.tenantId, nativeRuntimeManifestSHA256: manifest, protocolClasses: [InstalledCardProtocol.self])
        _ = try await api.connect(LocalAuthoringSession(tenantId: f.document.tenantId, principalId: f.document.principalId,
            origin: "http://127.0.0.1:18091", token: String(repeating: "a", count: 43)))
        return api
    }
    func testIssuedDescriptorCanonicalHashesAndClosedBinding() throws {
        let f = try fixture()
        XCTAssertEqual(try InstalledFurnishingThumbnail.canonicalHash(f.descriptor), f.metadata["source"]["descriptorSha256"].string)
        XCTAssertEqual(try InstalledFurnishingThumbnail.canonicalHash(f.descriptor["rights"]), f.metadata["source"]["rightsSha256"].string)
        let binding = try InstalledFurnishingThumbnail(value: f.metadata, descriptor: f.descriptor, furnishing: f.item)
        XCTAssertEqual(try binding.png.checkedData(f.image), f.image)
        for keys in [["extra"], ["source", "actor"], ["png", "path"], ["provenance", "workspaceId"]] {
            var bad = f.metadata
            if keys.count == 1 { bad[keys[0]] = .bool(true) } else { bad[keys[0]][keys[1]] = .bool(true) }
            XCTAssertThrowsError(try InstalledFurnishingThumbnail(value: bad, descriptor: f.descriptor, furnishing: f.item))
        }
    }
    func testRejectsWrongProfileSourceRightsProvenanceAndPixels() throws {
        let f = try fixture()
        let cases: [(String, String?, DocumentValue)] = [
            ("profile", nil, .string("unknown")), ("source", "furnishingId", .string(UUID().uuidString.lowercased())),
            ("source", "modelSha256", .string(String(repeating: "a", count: 64))),
            ("source", "descriptorSha256", .string(String(repeating: "b", count: 64))),
            ("source", "rightsSha256", .string(String(repeating: "c", count: 64))),
            ("provenance", "recipeId", .string("unknown")), ("provenance", "toolId", .string("unknown")),
            ("provenance", "renderRequestId", .string("not-a-job")), ("png", "width", .integer(32))]
        for (key, nested, replacement) in cases {
            var bad = f.metadata
            if let nested { bad[key][nested] = replacement } else { bad[key] = replacement }
            XCTAssertThrowsError(try InstalledFurnishingThumbnail(value: bad, descriptor: f.descriptor, furnishing: f.item), key)
        }
        var changed = f.descriptor; changed["provenance"] = .string("Different installed identity")
        XCTAssertThrowsError(try InstalledFurnishingThumbnail(value: f.metadata, descriptor: changed, furnishing: f.item))
        let binding = try InstalledFurnishingThumbnail(value: f.metadata, descriptor: f.descriptor, furnishing: f.item)
        var bytes = f.image; bytes[bytes.startIndex + 20] ^= 1
        XCTAssertThrowsError(try binding.png.checkedData(bytes))
    }
    func testReadOnlyInstalledPathUsesExplicitMarkerAndNativeHeaders() async throws {
        let f = try fixture(); InstalledCardProtocol.configure(try plan(f)); let api = try await connect(f)
        let result = try await api.installedWorkspaceFurnishingThumbnail(f.summary, in: f.document)
        XCTAssertEqual(result.data, f.image); XCTAssertEqual(result.furnishing, try f.item)
        let calls = InstalledCardProtocol.requests()
        XCTAssertTrue(calls.allSatisfy { $0.httpMethod == "GET" })
        XCTAssertFalse(calls.contains { $0.url!.path.contains("thumbnail-selection") || $0.url!.path.contains("thumbnail-candidates") })
        for request in calls.filter({ $0.url!.path.contains("installed-thumbnail") }) {
            XCTAssertTrue(request.url!.path.hasPrefix("/api/v1/tenants/" + f.document.tenantId + "/workspace/furnishings/" + f.summary.id + "/versions/1/"))
            XCTAssertEqual(request.value(forHTTPHeaderField: "Origin"), "http://127.0.0.1:18091")
            XCTAssertEqual(request.value(forHTTPHeaderField: "x-csrf-token"), "synthetic-csrf")
        }
    }
    func testMarkerOwnerAndRuntimeDenialsPrecedeReads() async throws {
        let f = try fixture()
        var summary = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(f.summary))
        for marker in [DocumentValue.null, .string("unknown")] {
            summary["installedThumbnailProfile"] = marker
            let item = try JSONDecoder().decode(WorkspaceFurnishingSummary.self, from: JSONEncoder().encode(summary))
            InstalledCardProtocol.configure(try plan(f)); let api = try await connect(f)
            do { _ = try await api.installedWorkspaceFurnishingThumbnail(item, in: f.document); XCTFail("marker admitted") } catch { }
            XCTAssertEqual(InstalledCardProtocol.requests().count, 1)
        }
        InstalledCardProtocol.configure(try plan(f)); let api = try await connect(f)
        var doc = try JSONDecoder().decode(DocumentValue.self, from: JSONEncoder().encode(f.document)); doc["principalId"] = .string(UUID().uuidString.lowercased())
        let other = try JSONDecoder().decode(NativeDocumentState.self, from: JSONEncoder().encode(doc))
        do { _ = try await api.installedWorkspaceFurnishingThumbnail(f.summary, in: other); XCTFail("owner admitted") } catch { }
        XCTAssertEqual(InstalledCardProtocol.requests().count, 1)
        InstalledCardProtocol.configure(try plan(f)); let unqualified = try await connect(f, manifest: String(repeating: "f", count: 64))
        do { _ = try await unqualified.installedWorkspaceFurnishingThumbnail(f.summary, in: f.document); XCTFail("runtime admitted") } catch { }
        XCTAssertEqual(InstalledCardProtocol.requests().count, 1)
    }
    func testMissingBindingAndRightsDenialNeverFallBackOrFetchPNG() async throws {
        let f = try fixture()
        var p = try plan(f); p.metadataStatus = 404
        InstalledCardProtocol.configure(p); var api = try await connect(f)
        do { _ = try await api.installedWorkspaceFurnishingThumbnail(f.summary, in: f.document); XCTFail("missing binding admitted") } catch { }
        XCTAssertFalse(InstalledCardProtocol.requests().contains { $0.url!.path.hasSuffix("/image") })
        p = try plan(f); var denied = f.descriptor; denied["rights"]["permissions"]["display"] = .bool(false)
        p.descriptors = [try JSONEncoder().encode(denied)]
        InstalledCardProtocol.configure(p); api = try await connect(f)
        do { _ = try await api.installedWorkspaceFurnishingThumbnail(f.summary, in: f.document); XCTFail("rights admitted") } catch { }
        XCTAssertFalse(InstalledCardProtocol.requests().contains { $0.url!.path.contains("installed-thumbnail") })
    }
    func testFreshDescriptorRightsAreRecheckedAfterImage() async throws {
        let f = try fixture(); var p = try plan(f), changed = f.descriptor
        changed["rights"]["permissions"]["display"] = .bool(false)
        p.descriptors.append(try JSONEncoder().encode(changed)); InstalledCardProtocol.configure(p); let api = try await connect(f)
        do { _ = try await api.installedWorkspaceFurnishingThumbnail(f.summary, in: f.document); XCTFail("stale card returned") } catch { }
        XCTAssertTrue(InstalledCardProtocol.requests().contains { $0.url!.path.hasSuffix("/image") })
    }
    func testDigestHeaderMimeAndImageBytesAreRequired() async throws {
        let f = try fixture()
        for mode in 0..<4 {
            var p = try plan(f)
            switch mode { case 0: p.digest = nil; case 1: p.digest = String(repeating: "f", count: 64)
            case 2: p.mime = "image/jpeg"; default: p.image[p.image.startIndex + 20] ^= 1 }
            InstalledCardProtocol.configure(p); let api = try await connect(f)
            do { _ = try await api.installedWorkspaceFurnishingThumbnail(f.summary, in: f.document); XCTFail("invalid image returned for mode \(mode)") } catch { }
        }
    }
    func testSessionGenerationChangeWhileReadingImageIsRejected() async throws {
        let f = try fixture(); var p = try plan(f); p.holdImage = true; InstalledCardProtocol.configure(p); let api = try await connect(f)
        let task = Task { try await api.installedWorkspaceFurnishingThumbnail(f.summary, in: f.document) }
        for _ in 0..<10_000 { if InstalledCardProtocol.hasHeldResponse() { break }; await Task.yield() }
        guard InstalledCardProtocol.hasHeldResponse() else { task.cancel(); return XCTFail("image barrier not reached") }
        await api.endLocalAuthoringSession(); InstalledCardProtocol.release()
        do { _ = try await task.value; XCTFail("old generation returned") } catch { }
    }
    func testActualPackagedPNGMatchesIssuedDefaultBinding() throws {
        guard let root = ProcessInfo.processInfo.environment["EXHIBITOS_DEFAULT_112_CARD_FIXTURE"] else { throw XCTSkip("Set exact immutable-card default source fixture") }
        let f = try fixture(); let bytes = try Data(contentsOf: URL(fileURLWithPath: root).appendingPathComponent("assets/furnishings/user-authored-20261027/thin-brass-frame/thumbnail.png"))
        var value = f.metadata; value["png"]["bytes"] = .integer(5176)
        value["png"]["sha256"] = .string("6973f99f18bc88f0d0ff6797b43a9549504e41f3d05423354cd88ce36c5f648a")
        XCTAssertEqual(try InstalledFurnishingThumbnail(value: value, descriptor: f.descriptor, furnishing: f.item).png.checkedData(bytes), bytes)
    }
}

private final class InstalledCardProtocol: URLProtocol, @unchecked Sendable {
    struct Plan: Sendable {
        var session: Data, descriptors: [Data], metadata: Data, image: Data
        var digest: String?
        var metadataStatus = 200, mime = "image/png", holdImage = false
    }
    private static let lock = NSLock()
    nonisolated(unsafe) private static var plan: Plan?
    nonisolated(unsafe) private static var observed: [URLRequest] = []
    nonisolated(unsafe) private static var index = 0
    nonisolated(unsafe) private static var held: InstalledCardProtocol?
    static func configure(_ p: Plan) { lock.lock(); defer { lock.unlock() }; plan = p; observed = []; index = 0; held = nil }
    static func requests() -> [URLRequest] { lock.lock(); defer { lock.unlock() }; return observed }
    static func hasHeldResponse() -> Bool { lock.lock(); defer { lock.unlock() }; return held != nil }
    static func release() { lock.lock(); let item = held; held = nil; lock.unlock(); item?.imageResponse() }
    private func response() {
        Self.lock.lock(); guard let p = Self.plan else { Self.lock.unlock(); return }
        Self.observed.append(request); let path = request.url!.path
        if path.hasSuffix("/image"), p.holdImage { Self.held = self; Self.lock.unlock(); return }
        let i = Self.index
        if path.hasSuffix("/versions/1") { Self.index += 1 }
        Self.lock.unlock()
        if path == "/api/v1/auth/session" { finish(200, "application/json", p.session) }
        else if path.hasSuffix("/versions/1") { finish(200, "application/json", p.descriptors[min(i, p.descriptors.count - 1)]) }
        else if path.hasSuffix("/installed-thumbnail") { finish(p.metadataStatus, "application/json", p.metadata) }
        else if path.hasSuffix("/image") { imageResponse() }
        else { finish(404, "application/json", Data("{}".utf8)) }
    }
    private func imageResponse() {
        Self.lock.lock(); let p = Self.plan; Self.lock.unlock()
        if let p { finish(200, p.mime, p.image, digest: p.digest) }
    }
    private func finish(_ status: Int, _ mime: String, _ data: Data, digest: String? = nil) {
        var headers = ["Content-Type": mime, "Content-Length": String(data.count)]
        headers["x-exhibitos-sha256"] = digest
        let response = HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: "HTTP/1.1", headerFields: headers)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data); client?.urlProtocolDidFinishLoading(self)
    }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() { response() }
    override func stopLoading() {}
}
