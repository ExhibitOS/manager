// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

@MainActor
final class ExhibitionIntegrationTests: XCTestCase {
    func testActualNativeExhibitionSaveReceiptReopenApprovedPlacementAndConflict() async throws {
        let env = ProcessInfo.processInfo.environment
        guard let helper = env["EXHIBITOS_EXHIBITION_TEST_HELPER"], let directory = env["EXHIBITOS_EXHIBITION_TEST_ROOT"], let fixture = env["EXHIBITOS_EXHIBITION_TEST_FIXTURE"] else { throw XCTSkip("Requires the explicitly owned native exhibition runtime fixture") }
        let root = URL(fileURLWithPath: directory), profile = root.appendingPathComponent("profile")
        XCTAssertFalse(FileManager.default.fileExists(atPath: profile.path))
        var bridge = try NativeBridge(helper: URL(fileURLWithPath: helper), profile: profile)
        var context = try await bridge.context()
        func idle(_ bridge: NativeBridge, _ context: Context, ready: Bool = false) async throws -> NativeHostStatus {
            let end = ContinuousClock.now.advanced(by: .seconds(120))
            while ContinuousClock.now < end {
                let state = try await bridge.nativeStatus(context)
                if !state.busy { if let code = state.errorCode { throw BridgeFailure(code) }; if !ready || state.runtime?.readiness.ready == true { return state } }
                try await Task.sleep(for: .milliseconds(100))
            }
            throw BridgeFailure("NATIVE_TIMEOUT")
        }
        do {
            _ = try await bridge.prepare(context); _ = try await idle(bridge, context)
            _ = try await bridge.start(context); let started = try await idle(bridge, context, ready: true)
            let credentials = try await bridge.authoringSession(context)
            let api = try AuthoringAPI(port: started.configuration!.ports.web, tenantId: credentials.tenantId)
            _ = try await api.connect(credentials)
            let emptyPage = try await api.exhibitions(); XCTAssertTrue(emptyPage.items.isEmpty)
            let draft = try NativeExhibitionDraft.new(title: "개인 네이티브 전시")
            var local = try await bridge.documentState(context)
            local.draft = draft; local.pending = .init(etag: nil, draft: draft)
            local = try await bridge.saveDocumentState(context, state: local)
            let request = try XCTUnwrap(local.pending)
            let first = try await api.saveExhibition(request.draft, requestId: request.requestId, etag: nil)
            XCTAssertEqual(first.revision, 1)
            // The server accepted it but its reply was not acknowledged locally.
            _ = try await bridge.stop(context); _ = try await idle(bridge, context); try await bridge.close()
            bridge = try NativeBridge(helper: URL(fileURLWithPath: helper), profile: profile); context = try await bridge.context()
            local = try await bridge.documentState(context)
            XCTAssertEqual(local.pending?.requestId, request.requestId); XCTAssertEqual(local.draft, draft)
            _ = try await bridge.start(context); _ = try await idle(bridge, context, ready: true)
            let renewed = try await bridge.authoringSession(context); _ = try await api.connect(renewed)
            let replay = try await api.saveExhibition(local.pending!.draft, requestId: request.requestId, etag: nil)
            XCTAssertEqual(replay.revision, 1); XCTAssertEqual(replay.etag, first.etag)
            local.pending = nil; local.etag = replay.etag; local.savedEditVersion = draft.document["editVersion"].integer
            local = try await bridge.saveDocumentState(context, state: local)
            XCTAssertFalse(local.dirty)
            let artist = try await api.createArtist(name: "전시 합성 작가", bio: "Synthetic native document fixture")
            var m = ArtworkMetadata(); m.title = "배치할 실제 GLB"; m.rights.holder = artist.name; m.rights.licenseId = "CC0-1.0"; m.rights.licenseText = "합성 자료. 전시 및 OEX 내보내기 허용."; m.rights.permissions.export = true; m.provenance.source = "synthetic"
            let artwork = try await api.saveArtwork(id: nil, revision: nil, artistId: artist.id, metadata: m)
            let file = try await ImportFileSnapshot.load(URL(fileURLWithPath: fixture))
            let intent = NativeImportIntent(artworkId: artwork.id, mime: file.mime, sha256: file.sha256, bytes: file.data.count, scaleMeters: 1, rights: m.rights)
            let job = try await api.createImport(intent); try await api.uploadImport(job.id, bytes: file.data); _ = try await api.importAction(job.id, action: .complete)
            var validated: NativeImportJob?
            let deadline = ContinuousClock.now.advanced(by: .seconds(45))
            while ContinuousClock.now < deadline {
                let current = try await api.importJob(job.id)
                if current.state == "approved" { validated = current; break }
                if current.state == "failed" { throw AuthoringFailure(current.errorCode ?? "IMPORT_FAILED") }
                try await Task.sleep(for: .milliseconds(100))
            }
            let asset = try XCTUnwrap(validated?.assetId)
            _ = try await api.approveArtwork(artwork.id, revision: artwork.revision, assetId: asset)
            let projected = try await api.studioArtwork(artwork.id)
            var changed = draft
            try changed.rename("저장 후 다시 연 전시")
            try changed.addApprovedArtwork(projected.artwork, roomId: changed.rooms[0]["id"].string!, position: [0, 0.5, 0])
            local.draft = changed; local.pending = .init(etag: first.etag, draft: changed)
            local = try await bridge.saveDocumentState(context, state: local)
            let pending = local.pending!
            let second = try await api.saveExhibition(pending.draft, requestId: pending.requestId, etag: pending.etag)
            XCTAssertEqual(second.revision, 2); XCTAssertEqual(second.draft.artworks, [projected.artwork])
            do { _ = try await api.saveExhibition(changed, requestId: UUID().uuidString, etag: first.etag); XCTFail("Must preserve optimistic conflict") }
            catch let failure as AuthoringFailure { XCTAssertEqual(failure.status, 412); XCTAssertEqual(failure.code, "REMOTE_CONFLICT") }
            var different = changed; try different.rename("같은 요청으로 바꾸면 안 됨")
            do { _ = try await api.saveExhibition(different, requestId: pending.requestId, etag: pending.etag); XCTFail("Must not change uncertain request payload") }
            catch let failure as AuthoringFailure { XCTAssertEqual(failure.code, "REQUEST_CONFLICT") }
            let actual = try await api.exhibition(draft.id)
            XCTAssertEqual(actual.etag, second.etag); XCTAssertEqual(actual.draft, changed)
            let page = try await api.exhibitions(query: "다시 연")
            XCTAssertEqual(page.items.count, 1); XCTAssertEqual(page.items.first?.id, draft.id)
            local.pending = nil; local.etag = second.etag; local.savedEditVersion = changed.document["editVersion"].integer
            local = try await bridge.saveDocumentState(context, state: local)
            // Unsent valid edits are preserved separately from the saved ETag.
            var unsent = changed; try unsent.rename("이 Mac에 보관한 미완료 전시")
            local.draft = unsent; local = try await bridge.saveDocumentState(context, state: local); XCTAssertTrue(local.dirty)
            _ = try await bridge.stop(context); _ = try await idle(bridge, context); try await bridge.close()
            bridge = try NativeBridge(helper: URL(fileURLWithPath: helper), profile: profile); context = try await bridge.context()
            let restored = try await bridge.documentState(context)
            XCTAssertEqual(restored.draft, unsent); XCTAssertEqual(restored.etag, second.etag); XCTAssertTrue(restored.dirty)
            _ = try await bridge.start(context); _ = try await idle(bridge, context, ready: true)
            let renewedAgain = try await bridge.authoringSession(context); _ = try await api.connect(renewedAgain)
            let reopenedServer = try await api.exhibition(draft.id); XCTAssertEqual(reopenedServer.draft, changed)
            _ = try await bridge.stop(context); let final = try await idle(bridge, context); try await bridge.close()
            let receipt: [String: Any] = ["result":"PASS", "scope":"ACTUAL_NATIVE_EXHIBITION_SAVE_REOPEN_APPROVED_GLB_PLACEMENT_NOT_VIEWPORT_OEX", "exhibitionId":draft.id, "artworkId":artwork.id, "assetId":asset, "serverRevision":second.revision, "placementCount":changed.placements.count, "sourceBytes":file.data.count, "sourceSHA256":file.sha256, "responseLossRecovered":true, "staleETagRejected":true, "changedRequestRejected":true, "unsentDraftRestored":true, "nodeExit":final.runtime?.exits["node"] ?? -999, "postgresExit":final.runtime?.exits["postgres"] ?? -999]
            try JSONSerialization.data(withJSONObject: receipt, options: [.prettyPrinted, .sortedKeys]).write(to: root.appendingPathComponent("result.json"), options: .atomic)
        } catch {
            _ = try? await bridge.stop(context); _ = try? await idle(bridge, context); try? await bridge.close(); throw error
        }
    }
}
