// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

@MainActor
final class PublicationIntegrationTests: XCTestCase {
    func testDurablePublicationCopyReplaysBothLostRepliesAndPreservesEditableSource() async throws {
        let env = ProcessInfo.processInfo.environment
        guard let helper = env["EXHIBITOS_PUBLICATION_TEST_HELPER"], let path = env["EXHIBITOS_PUBLICATION_TEST_PROFILE"], let output = env["EXHIBITOS_PUBLICATION_TEST_RECEIPT"] else { throw XCTSkip("Explicit owned native personal-workspace fixture only") }
        let profile = URL(fileURLWithPath: path)
        var bridge = try NativeBridge(helper: URL(fileURLWithPath: helper), profile: profile)
        var context = try await bridge.context()
        func idle(ready: Bool = false) async throws -> NativeHostStatus {
            let end = ContinuousClock.now.advanced(by: .seconds(120))
            while ContinuousClock.now < end {
                let status = try await bridge.nativeStatus(context)
                if !status.busy {
                    if let code = status.errorCode { throw BridgeFailure(code) }
                    if !ready || status.runtime?.readiness.ready == true { return status }
                }
                try await Task.sleep(for: .milliseconds(100))
            }
            throw BridgeFailure("NATIVE_TIMEOUT")
        }
        func api() async throws -> AuthoringAPI {
            _ = try await bridge.start(context)
            let status = try await idle(ready: true), credentials = try await bridge.authoringSession(context)
            let value = try AuthoringAPI(port: status.configuration!.ports.web, tenantId: credentials.tenantId)
            _ = try await value.connect(credentials); return value
        }
        func restart() async throws {
            _ = try await bridge.stop(context); let status = try await idle()
            XCTAssertEqual(status.runtime?.exits["node"], 0); XCTAssertEqual(status.runtime?.exits["postgres"], 0)
            try await bridge.close()
            bridge = try NativeBridge(helper: URL(fileURLWithPath: helper), profile: profile)
            context = try await bridge.context()
        }
        do {
            let original = try await bridge.documentState(context), draft = try XCTUnwrap(original.draft)
            XCTAssertFalse(original.dirty)
            var client = try await api()
            // Every mutation belongs to a new additive synthetic source. The
            // fixture's current document, artwork and approval stay unchanged.
            var source = try draft.copyForEditing()
            let savedSource = try await client.saveExhibition(source, requestId: UUID().uuidString, etag: nil)
            let ready = try await client.publicationReady(exhibitionId: source.id)
            XCTAssertEqual(ready.status, "READY"); XCTAssertEqual(ready.etag, savedSource.etag)
            var journal = try await bridge.publicationState(context)
            XCTAssertNil(journal.pending); XCTAssertNil(journal.completed)
            let intent = try NativePublicationPending(source: source, etag: savedSource.etag)
            journal.pending = intent; journal = try await bridge.savePublicationState(context, state: journal)
            // The server accepts the copy but its response is deliberately not
            // written to the private journal: equivalent to a lost response.
            let firstCopy = try await client.saveExhibition(intent.snapshot, requestId: intent.saveRequestId, etag: nil)
            try await restart(); client = try await api()
            journal = try await bridge.publicationState(context)
            var pending = try XCTUnwrap(journal.pending)
            XCTAssertEqual(pending, intent); XCTAssertNil(pending.snapshotEtag)
            let replayedCopy = try await client.saveExhibition(pending.snapshot, requestId: pending.saveRequestId, etag: nil)
            XCTAssertEqual(replayedCopy.draft, firstCopy.draft); XCTAssertEqual(replayedCopy.etag, firstCopy.etag)
            pending.snapshotEtag = replayedCopy.etag; journal.pending = pending
            journal = try await bridge.savePublicationState(context, state: journal)
            let publication = try await client.preparePublication(exhibitionId: pending.snapshot.id, etag: replayedCopy.etag, requestId: pending.publishRequestId)
            // Lose the successful publish reply, close all owned services, then
            // reload and replay exactly the durable request after reopening.
            try await restart(); client = try await api()
            journal = try await bridge.publicationState(context)
            let resumed = try XCTUnwrap(journal.pending)
            XCTAssertEqual(resumed, pending)
            let replayed = try await client.preparePublication(exhibitionId: resumed.snapshot.id, etag: XCTUnwrap(resumed.snapshotEtag), requestId: resumed.publishRequestId)
            XCTAssertEqual(replayed, publication)
            let list = try await client.publications(exhibitionId: resumed.snapshot.id)
            XCTAssertEqual(list.count, 1); XCTAssertEqual(list.first, publication)
            journal.completed = NativePublicationCompleted(pending: resumed, publication: replayed); journal.pending = nil
            journal = try await bridge.savePublicationState(context, state: journal)
            XCTAssertNil(journal.pending); XCTAssertEqual(journal.completed?.publication, publication)
            // Immutable preparation must never freeze the editable source.
            try source.rename(source.title + " 수정")
            let edited = try await client.saveExhibition(source, requestId: UUID().uuidString, etag: savedSource.etag)
            XCTAssertEqual(edited.draft.title, source.title)
            var frozen = resumed.snapshot
            try frozen.rename(frozen.title + " 거부")
            do { _ = try await client.saveExhibition(frozen, requestId: UUID().uuidString, etag: replayedCopy.etag); XCTFail("Prepared copy must stay immutable") }
            catch let error as AuthoringFailure { XCTAssertEqual(error.status, 409); XCTAssertEqual(error.code, "PUBLISHED_DRAFT_IMMUTABLE") }
            let unchanged = try await bridge.documentState(context)
            XCTAssertEqual(unchanged.draft, original.draft); XCTAssertEqual(unchanged.etag, original.etag)
            _ = try await bridge.stop(context); let stopped = try await idle(); try await bridge.close()
            let receipt: [String: Any] = ["scope":"ACTUAL_LOCAL_PUBLICATION_API_NOT_EXTERNAL_DEPLOYMENT_OR_GUI", "originalDocumentPreserved":true,"editableSourceExhibition":source.id,"snapshotExhibition":resumed.snapshot.id,"publicationId":publication.publicationId,"revisionSha256":publication.revisionSha256,"publicUrl":publication.publicUrl,"saveLostReplyReplay":true,"publishLostReplyReplay":true,"publicationCount":list.count,"sourceRemainsEditable":true,"copyImmutableStatus":409,"nodeExit":stopped.runtime?.exits["node"] ?? -999,"postgresExit":stopped.runtime?.exits["postgres"] ?? -999]
            try JSONSerialization.data(withJSONObject:receipt,options:[.prettyPrinted,.sortedKeys]).write(to:URL(fileURLWithPath:output),options:.atomic)
        } catch { _ = try? await bridge.stop(context); _ = try? await idle(); try? await bridge.close(); throw error }
    }
}
