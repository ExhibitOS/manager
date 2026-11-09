// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

@MainActor
final class AuthoringAPITests: XCTestCase {
    func testUnknownArtworkDepthRemainsAbsentAcrossMetadataRoundTrip() throws {
        var metadata = ArtworkMetadata()
        metadata.dimensions.width = 0.934
        metadata.dimensions.height = 0.732
        XCTAssertNil(metadata.dimensions.depth)
        let bytes = try JSONEncoder().encode(metadata)
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: bytes) as? [String: Any])
        let dimensions = try XCTUnwrap(object["dimensions"] as? [String: Any])
        XCTAssertNil(dimensions["depth"])
        XCTAssertEqual(try JSONDecoder().decode(ArtworkMetadata.self, from: bytes), metadata)
    }
    func testKnownArtworkDepthIsPreservedUntilExplicitlyCleared() throws {
        var metadata = ArtworkMetadata()
        metadata.dimensions.depth = 0.25
        let saved = try JSONDecoder().decode(ArtworkMetadata.self, from: JSONEncoder().encode(metadata))
        XCTAssertEqual(saved.dimensions.depth, 0.25)
        var cleared = saved
        cleared.dimensions.depth = nil
        let bytes = try JSONEncoder().encode(cleared)
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: bytes) as? [String: Any])
        let dimensions = try XCTUnwrap(object["dimensions"] as? [String: Any])
        XCTAssertNil(dimensions["depth"])
        XCTAssertEqual(try JSONDecoder().decode(ArtworkMetadata.self, from: bytes), cleared)
        XCTAssertEqual(cleared.dimensions.width, saved.dimensions.width)
        XCTAssertEqual(cleared.dimensions.height, saved.dimensions.height)
    }
    func testRejectsForeignScopeBeforeAnyConnection() {
        XCTAssertThrowsError(try AuthoringAPI(port: 80, tenantId: UUID().uuidString))
        XCTAssertThrowsError(try AuthoringAPI(port: 13200, tenantId: "../foreign"))
    }
    func testProductImageReplacementAdmitsOnlyLinkedBoundedThumbnailAlongsideReviewedImages() {
        let successor = "003a98c579e69e1b38a7cb96971f0872f15b8afd169d392a96ac94fe87be9f92"
        let prior = "62200d7ccb47e7ede5d1910a65e0087ac171c595d76ca14df6a19290382e4288"
        #if DEBUG
        XCTAssertTrue(AuthoringAPI.supportsProductImageRuntime(manifest: successor))
        XCTAssertTrue(AuthoringAPI.supportsProductImageReplacementThumbnail(manifest: successor))
        #else
        XCTAssertFalse(AuthoringAPI.supportsProductImageReplacementThumbnail(manifest: successor))
        #endif
        XCTAssertFalse(AuthoringAPI.supportsProductImageReplacementThumbnail(manifest: prior))
        XCTAssertFalse(AuthoringAPI.supportsProductImageReplacementThumbnail(manifest: nil))
        let full = NativeProductImagePreparationStatus.Image(assetId: UUID().uuidString, mime: "image/png",
            sha256: String(repeating: "a", count: 64), bytes: 600_000, width: 2048, height: 1536)
        let coarse = NativeProductImagePreparationStatus.Image(assetId: UUID().uuidString, mime: "image/png",
            sha256: String(repeating: "b", count: 64), bytes: 500_000, width: 512, height: 384)
        func asset(_ image: NativeProductImagePreparationStatus.Image) -> DocumentValue {
            .object(["id": .string(image.assetId), "sha256": .string(image.sha256),
                     "bytes": .integer(Int64(image.bytes)), "mime": .string(image.mime)])
        }
        let previewId = UUID().uuidString.lowercased()
        let preview: DocumentValue = .object(["id": .string(previewId), "role": .string("preview"),
            "mime": .string("image/png"), "path": .string("thumbnail.png"), "bytes": .integer(512 * 1024),
            "sha256": .string(String(repeating: "c", count: 64))])
        let thumbnail = "org.exhibitos.viewer/thumbnail"
        let reference: DocumentValue = .object(["version": .integer(1), "assetId": .string(previewId.uppercased())])
        var artwork: DocumentValue = .object(["primaryAssetId": .string(full.assetId),
            "assets": .array([asset(full), asset(coarse)])])
        func validates(_ admitsThumbnail: Bool = true) -> Bool {
            AuthoringAPI.validatesProductImageReplacementInventory(artwork, full: full, coarse: coarse,
                admitsThumbnail: admitsThumbnail)
        }
        XCTAssertTrue(validates(false))
        artwork["extensions"] = .object([thumbnail: reference])
        XCTAssertTrue(validates(false), "Legacy runtime validates only the reviewed inventory")
        XCTAssertFalse(validates(), "Linked preview is missing")
        artwork["extensions"] = .object([:])
        artwork["assets"] = .array([asset(full), asset(coarse), preview])
        XCTAssertFalse(validates(), "Unlinked preview")
        artwork["extensions"] = .object([thumbnail: reference])
        XCTAssertTrue(validates())
        XCTAssertFalse(validates(false), "Runtime has no thumbnail admission")

        artwork["assets"] = .array([asset(full), asset(coarse), preview, preview])
        XCTAssertFalse(validates(), "Duplicate preview")
        artwork["assets"] = .array([asset(full), asset(coarse), preview])
        artwork["extensions"][thumbnail]["assetId"] = .string(coarse.assetId.lowercased())
        XCTAssertFalse(validates(), "Thumbnail points to a reviewed image")
        artwork["extensions"][thumbnail] = reference
        artwork["extensions"][thumbnail]["extra"] = .bool(true)
        XCTAssertFalse(validates(), "Thumbnail marker has an extra field")
        artwork["extensions"][thumbnail] = reference

        for (key, value) in ["role": DocumentValue.string("image"), "mime": .string("image/jpeg"),
                             "path": .string("thumbnail.jpg"), "bytes": .integer(512 * 1024 + 1),
                             "sha256": .string(String(repeating: "C", count: 64))] {
            var invalidPreview = preview
            invalidPreview[key] = value
            artwork["assets"] = .array([asset(full), asset(coarse), invalidPreview])
            XCTAssertFalse(validates(), "Invalid preview \(key)")
        }
        var changedCoarse = asset(coarse)
        changedCoarse["sha256"] = .string(full.sha256)
        artwork["assets"] = .array([asset(full), changedCoarse, preview])
        XCTAssertFalse(validates(), "Reviewed Coarse digest changed")
    }
    func testActualLocalCMSCreateRevisionConflictRightsAndSessionIsolation() async throws {
        let environment = ProcessInfo.processInfo.environment
        guard let tenant = environment["EXHIBITOS_AUTHORING_TEST_TENANT"],
              let secret = environment["EXHIBITOS_AUTHORING_TEST_PASSWORD"] else {
            throw XCTSkip("Requires the explicitly owned synthetic native runtime harness")
        }
        let api = try AuthoringAPI(port: 13200, tenantId: tenant)
        let login = try await api.login(subject: "authoring-synthetic", password: secret)
        XCTAssertEqual(login.tenantId.lowercased(), tenant.lowercased())
        let empty = try await api.artworks()
        XCTAssertTrue(empty.items.isEmpty)
        let artist = try await api.createArtist(name: "합성 테스트 작가", bio: "Native API contract fixture")
        var metadata = ArtworkMetadata()
        metadata.title = "합성 네이티브 작품"
        metadata.description = "개인 자료가 없는 테스트 작품"
        metadata.rights.holder = artist.name
        metadata.rights.creditLine = "Synthetic test fixture"
        metadata.rights.licenseId = "CC0-1.0"
        metadata.rights.licenseText = nil
        metadata.rights.permissions.export = true
        metadata.provenance.source = "synthetic"
        let first = try await api.saveArtwork(id: nil, revision: nil, artistId: artist.id, metadata: metadata)
        XCTAssertEqual(first.metadata, metadata)
        XCTAssertEqual(first.revision, 1)
        metadata.title = "수정된 합성 작품"
        let saved = try await api.saveArtwork(id: first.id, revision: first.revision, artistId: artist.id, metadata: metadata)
        XCTAssertEqual(saved.revision, 2)
        XCTAssertNil(saved.approvedRevision)
        do {
            _ = try await api.saveArtwork(id: first.id, revision: first.revision, artistId: artist.id, metadata: metadata)
            XCTFail("A stale revision must fail without overwriting the newer revision")
        } catch let error as AuthoringFailure { XCTAssertTrue([409, 412].contains(error.status)) }
        let reopened = try await api.artwork(first.id)
        XCTAssertEqual(reopened.metadata, metadata)
        let filtered = try await api.artworks(query: "수정된")
        XCTAssertEqual(filtered.items.map(\.id), [first.id])
        let privateClient = try AuthoringAPI(port: 13200, tenantId: tenant)
        do { _ = try await privateClient.artworks(); XCTFail("A new client must not adopt another cookie jar") }
        catch let error as AuthoringFailure { XCTAssertEqual(error.status, 401) }
        try await api.logout()
        do { _ = try await api.artworks(); XCTFail("Logged out session must be refused") }
        catch let error as AuthoringFailure { XCTAssertEqual(error.status, 401) }
        let relogin = try await api.login(subject: "authoring-synthetic", password: secret)
        XCTAssertEqual(relogin.userId, login.userId)
        let same = try await api.artwork(first.id)
        XCTAssertEqual(same.metadata, metadata)
        try await api.logout()
    }
}
