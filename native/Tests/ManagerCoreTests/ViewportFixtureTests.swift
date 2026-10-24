// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

/// Build one fresh owned real-asset exhibition for Computer Use qualification.
/// Prior lifecycle/replay/conflict acceptance is reused rather than repeated.
@MainActor
final class ViewportFixtureTests: XCTestCase {
    func testPrepareOwnedViewportExhibition() async throws {
        let env = ProcessInfo.processInfo.environment
        guard let helper = env["EXHIBITOS_VIEWPORT_TEST_HELPER"], let directory = env["EXHIBITOS_VIEWPORT_TEST_ROOT"], let source = env["EXHIBITOS_VIEWPORT_TEST_FIXTURE"] else { throw XCTSkip("Explicit isolated viewport fixture only") }
        let root = URL(fileURLWithPath: directory), profile = root.appendingPathComponent("profile")
        XCTAssertFalse(FileManager.default.fileExists(atPath: profile.path))
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
            _ = try await bridge.prepare(context); _ = try await idle()
            _ = try await bridge.start(context); let status = try await idle(ready: true)
            let credentials = try await bridge.authoringSession(context)
            let api = try AuthoringAPI(port: status.configuration!.ports.web, tenantId: credentials.tenantId)
            _ = try await api.connect(credentials)
            let artist = try await api.createArtist(name: "3D 합성 작가", bio: "Computer Use fixture")
            var metadata = ArtworkMetadata(); metadata.title = "3D 합성 조각"; metadata.dimensions.depth = 1; metadata.rights.holder = artist.name; metadata.rights.licenseId = "CC0-1.0"; metadata.rights.licenseText = "합성 자료. 전시와 OEX 내보내기 허용."; metadata.rights.permissions.export = true; metadata.provenance.source = "synthetic"
            let artwork = try await api.saveArtwork(id: nil, revision: nil, artistId: artist.id, metadata: metadata)
            let file = try await ImportFileSnapshot.load(URL(fileURLWithPath: source))
            let job = try await api.createImport(NativeImportIntent(artworkId: artwork.id, mime: file.mime, sha256: file.sha256, bytes: file.data.count, scaleMeters: 1, rights: metadata.rights))
            try await api.uploadImport(job.id, bytes: file.data); _ = try await api.importAction(job.id, action: .complete)
            var asset: String?
            let end = ContinuousClock.now.advanced(by: .seconds(45))
            while ContinuousClock.now < end {
                let status = try await api.importJob(job.id)
                if status.state == "approved" { asset = status.assetId; break }
                if status.state == "failed" { throw AuthoringFailure(status.errorCode ?? "IMPORT_FAILED") }
                try await Task.sleep(for: .milliseconds(100))
            }
            _ = try await api.approveArtwork(artwork.id, revision: artwork.revision, assetId: XCTUnwrap(asset))
            let projection = try await api.studioArtwork(artwork.id)
            var draft = try NativeExhibitionDraft.new(title: "네이티브 3D 검증 전시")
            try draft.addApprovedArtwork(projection.artwork, roomId: draft.rooms[0]["id"].string!, position: [0, 0.5, 0])
            let saved = try await api.saveExhibition(draft, requestId: UUID().uuidString, etag: nil)
            var local = try await bridge.documentState(context); local.draft = saved.draft; local.etag = saved.etag; local.savedEditVersion = saved.draft.document["editVersion"].integer
            _ = try await bridge.saveDocumentState(context, state: local)
            _ = try await bridge.stop(context); let stopped = try await idle(); try await bridge.close()
            let receipt: [String:Any] = ["scope":"SYNTHETIC_VIEWPORT_GUI_FIXTURE_NOT_GUI_PASS", "exhibitionId":draft.id, "artworkId":artwork.id, "assetId":asset!, "bytes":file.data.count, "sha256":file.sha256, "nodeExit":stopped.runtime?.exits["node"] ?? -999, "postgresExit":stopped.runtime?.exits["postgres"] ?? -999]
            try JSONSerialization.data(withJSONObject: receipt, options: [.prettyPrinted,.sortedKeys]).write(to: root.appendingPathComponent("fixture.json"), options: .atomic)
        } catch { _ = try? await bridge.stop(context); _ = try? await idle(); try? await bridge.close(); throw error }
    }
}
