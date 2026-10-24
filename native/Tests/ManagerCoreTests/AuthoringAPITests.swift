// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

@MainActor
final class AuthoringAPITests: XCTestCase {
    func testRejectsForeignScopeBeforeAnyConnection() {
        XCTAssertThrowsError(try AuthoringAPI(port: 80, tenantId: UUID().uuidString))
        XCTAssertThrowsError(try AuthoringAPI(port: 13200, tenantId: "../foreign"))
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
