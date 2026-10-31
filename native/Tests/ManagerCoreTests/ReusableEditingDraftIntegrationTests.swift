// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

/// Explicit isolated local-service fixture. This is an actual native API
/// client against the retained Platform producer, not a GUI or release test.
@MainActor
final class ReusableEditingDraftIntegrationTests: XCTestCase {
    private struct Fixture: Decodable {
        let port: UInt16
        let publicationId: String
        let session: LocalAuthoringSession
    }

    func testActualReusableDraftAndPublishPreservePreviousPublic() async throws {
        let env = ProcessInfo.processInfo.environment
        guard let input = env["EXHIBITOS_REUSABLE_TEST_SESSION"], let output = env["EXHIBITOS_REUSABLE_TEST_RECEIPT"] else {
            throw XCTSkip("Explicit isolated local fixture required")
        }
        let fixture = try JSONDecoder().decode(Fixture.self, from: Data(contentsOf: URL(fileURLWithPath: input)))
        let api = try AuthoringAPI(port: fixture.port, tenantId: fixture.session.tenantId)
        _ = try await api.connect(fixture.session)
        let publicURL = try XCTUnwrap(URL(string: "http://127.0.0.1:\(fixture.port)/api/v1/publications/\(fixture.publicationId)?footsteps=1"))
        func publicBytes() async throws -> Data {
            let (data, response) = try await URLSession.shared.data(from: publicURL)
            guard (response as? HTTPURLResponse)?.statusCode == 200 else { throw AuthoringFailure("PUBLICATION_RESPONSE_INVALID") }
            return data
        }
        let oldPublic = try await publicBytes()
        let opened = try await api.reusableEditingDraft(publicationId: fixture.publicationId)
        let originalID = opened.draft.id
        var edited = opened.draft
        try edited.rename(edited.title + " · native integration")
        let saved = try await api.saveExhibition(edited, requestId: UUID().uuidString, etag: opened.etag)
        XCTAssertEqual(saved.draft.id, originalID)
        let afterSave = try await publicBytes(); XCTAssertEqual(afterSave, oldPublic)
        let intent = try NativePublicationPending(reusableSource: saved.draft, etag: saved.etag, capabilityPublicationId: fixture.publicationId)
        let published = try await api.preparePublication(exhibitionId: saved.draft.id, etag: saved.etag, requestId: intent.publishRequestId)
        let afterPublish = try await publicBytes(); XCTAssertEqual(afterPublish, oldPublic)
        var advanced = saved.draft
        try advanced.rename(advanced.title + " 2")
        _ = try await api.saveExhibition(advanced, requestId: UUID().uuidString, etag: saved.etag)
        let replay = try await api.preparePublication(exhibitionId: saved.draft.id, etag: saved.etag, requestId: intent.publishRequestId)
        XCTAssertEqual(replay, published)
        let reopened = try await api.reusableEditingDraft(publicationId: fixture.publicationId)
        XCTAssertEqual(reopened.authoringExhibitionId, originalID)
        XCTAssertEqual(reopened.currentPublication?.publicationId, published.publicationId)
        XCTAssertEqual(reopened.previousPublication?.publicationId, fixture.publicationId)
        let afterReplay = try await publicBytes(); XCTAssertEqual(afterReplay, oldPublic)
        let receipt: [String: Any] = ["scope":"actual Swift AuthoringAPI and isolated loopback Platform; not GUI or native runtime admission",
            "sourcePublicationId":fixture.publicationId, "authoringExhibitionId":originalID, "newPublicationId":published.publicationId,
            "oldPublicBytesPreserved":true, "sameRequestReplayAfterAdvance":true, "reopenedSameAuthoringId":true]
        try JSONSerialization.data(withJSONObject: receipt, options: [.prettyPrinted, .sortedKeys]).write(to: URL(fileURLWithPath: output), options: .atomic)
    }
}
