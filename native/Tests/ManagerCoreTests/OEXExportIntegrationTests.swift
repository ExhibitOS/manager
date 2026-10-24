// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

@MainActor
final class OEXExportIntegrationTests: XCTestCase {
    func testActualApprovedGLBExportExactETagRightsRefusalAndAtomicFile() async throws {
        let env = ProcessInfo.processInfo.environment
        guard let helper = env["EXHIBITOS_OEX_TEST_HELPER"], let profilePath = env["EXHIBITOS_OEX_TEST_PROFILE"], let output = env["EXHIBITOS_OEX_TEST_OUTPUT"], let fixture = env["EXHIBITOS_OEX_TEST_FIXTURE"] else { throw XCTSkip("Explicit owned existing native GLB/exhibition fixture only") }
        let profile = URL(fileURLWithPath: profilePath), destination = URL(fileURLWithPath: output)
        XCTAssertFalse(FileManager.default.fileExists(atPath: destination.path))
        let bridge = try NativeBridge(helper: URL(fileURLWithPath: helper), profile: profile), context = try await bridge.context()
        func idle(ready: Bool = false) async throws -> NativeHostStatus {
            let end = ContinuousClock.now.advanced(by: .seconds(120))
            while ContinuousClock.now < end {
                let status = try await bridge.nativeStatus(context)
                if !status.busy { if let code = status.errorCode { throw BridgeFailure(code) }; if !ready || status.runtime?.readiness.ready == true { return status } }
                try await Task.sleep(for: .milliseconds(100))
            }
            throw BridgeFailure("NATIVE_TIMEOUT")
        }
        do {
            let local = try await bridge.documentState(context), draft = try XCTUnwrap(local.draft), etag = try XCTUnwrap(local.etag)
            XCTAssertFalse(local.dirty); XCTAssertFalse(draft.artworks.isEmpty)
            _ = try await bridge.start(context); let status = try await idle(ready: true)
            let credentials = try await bridge.authoringSession(context)
            let api = try AuthoringAPI(port: status.configuration!.ports.web, tenantId: credentials.tenantId)
            do { _ = try await api.exportOEX(exhibitionId: draft.id, etag: etag); XCTFail("No session must be refused") }
            catch let error as AuthoringFailure { XCTAssertEqual(error.status, 401) }
            _ = try await api.connect(credentials)
            let saved = try await api.exhibition(draft.id); XCTAssertEqual(saved.etag, etag); XCTAssertEqual(saved.draft, draft)
            let archive = try await api.exportOEX(exhibitionId: draft.id, etag: etag)
            try await archive.write(to: destination)
            XCTAssertEqual(try Data(contentsOf: destination), archive.data)
            do { try await archive.write(to: destination.deletingLastPathComponent().appendingPathComponent("missing-parent/exhibition.oex")); XCTFail("Missing destination must preserve original output") }
            catch let error as AuthoringFailure { XCTAssertEqual(error.code, "EXPORT_WRITE_FAILED") }
            XCTAssertEqual(try Data(contentsOf: destination), archive.data)
            let falseETag = "\"studio-r1-" + String(repeating: "0", count: 64) + "\""
            do { _ = try await api.exportOEX(exhibitionId: draft.id, etag: falseETag); XCTFail("A changed ETag must not export") }
            catch let error as AuthoringFailure { XCTAssertEqual(error.status, 412); XCTAssertEqual(error.code, "REMOTE_CONFLICT") }
            // Add a separate owned no-export artwork/exhibition; never mutate the
            // already-qualified source artwork, draft, approval or asset bytes.
            let artist = try await api.createArtist(name: "OEX 제한 합성 작가", bio: "Synthetic export refusal fixture")
            var metadata = ArtworkMetadata(); metadata.title = "내보내기 제한 합성 작품"; metadata.dimensions.depth = 1; metadata.rights.holder = artist.name; metadata.rights.licenseId = "CC0-1.0"; metadata.rights.licenseText = "전시 허용; 이 검증에서는 내보내기 비허용"; metadata.rights.permissions.export = false; metadata.provenance.source = "synthetic"
            let artwork = try await api.saveArtwork(id: nil, revision: nil, artistId: artist.id, metadata: metadata)
            let source = try await ImportFileSnapshot.load(URL(fileURLWithPath: fixture))
            let job = try await api.createImport(NativeImportIntent(artworkId: artwork.id, mime: source.mime, sha256: source.sha256, bytes: source.data.count, scaleMeters: 1, rights: metadata.rights))
            try await api.uploadImport(job.id, bytes: source.data); _ = try await api.importAction(job.id, action: .complete)
            var asset: String?; let end = ContinuousClock.now.advanced(by: .seconds(45))
            while ContinuousClock.now < end {
                let current = try await api.importJob(job.id)
                if current.state == "approved" { asset = current.assetId; break }
                if current.state == "failed" { throw AuthoringFailure(current.errorCode ?? "IMPORT_FAILED") }
                try await Task.sleep(for: .milliseconds(100))
            }
            _ = try await api.approveArtwork(artwork.id, revision: artwork.revision, assetId: XCTUnwrap(asset))
            let projection = try await api.studioArtwork(artwork.id)
            var denied = try NativeExhibitionDraft.new(title: "OEX 제한 합성 전시")
            try denied.addApprovedArtwork(projection.artwork, roomId: denied.rooms[0]["id"].string!, position: [0,0.5,0])
            let deniedSaved = try await api.saveExhibition(denied, requestId: UUID().uuidString, etag: nil)
            do { _ = try await api.exportOEX(exhibitionId: denied.id, etag: deniedSaved.etag); XCTFail("Current approved bytes alone must not bypass rights") }
            catch let error as AuthoringFailure { XCTAssertEqual(error.status, 403); XCTAssertEqual(error.code, "RIGHTS_DENIED") }
            let unchanged = try await bridge.documentState(context); XCTAssertEqual(unchanged.draft, local.draft); XCTAssertEqual(unchanged.etag, etag)
            _ = try await bridge.stop(context); let stopped = try await idle(); try await bridge.close()
            let receipt: [String:Any] = ["scope":"ACTUAL_NATIVE_OEX_API_AND_FILE_NOT_GUI_PASS", "sourceExhibition":draft.id, "sourceETag":etag, "archiveBytes":archive.data.count, "archiveSHA256":archive.sha256, "sourceGLBBytes":source.data.count, "sourceGLBSHA256":source.sha256, "staleETagStatus":412, "noExportRightsStatus":403, "sourceDocumentPreserved":true, "nodeExit":stopped.runtime?.exits["node"] ?? -999, "postgresExit":stopped.runtime?.exits["postgres"] ?? -999]
            try JSONSerialization.data(withJSONObject: receipt, options: [.prettyPrinted,.sortedKeys]).write(to: destination.deletingLastPathComponent().appendingPathComponent("native-oex-result.json"), options: .atomic)
        } catch { _ = try? await bridge.stop(context); _ = try? await idle(); try? await bridge.close(); throw error }
    }
}
