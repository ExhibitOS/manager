// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

@MainActor
final class GLBImportTests: XCTestCase {
    func testActualGLBIntentRecoveryWorkerApprovalBytesAndFailureRetry() async throws {
        let env = ProcessInfo.processInfo.environment
        guard let helper = env["EXHIBITOS_GLB_TEST_HELPER"], let directory = env["EXHIBITOS_GLB_TEST_ROOT"], let fixture = env["EXHIBITOS_GLB_TEST_FIXTURE"] else { throw XCTSkip("Requires the explicit isolated GLB runtime harness") }
        let root = URL(fileURLWithPath: directory), profile = root.appendingPathComponent("profile")
        XCTAssertFalse(FileManager.default.fileExists(atPath: profile.path))
        let source = root.appendingPathComponent("공백 포함 작품.glb")
        try FileManager.default.copyItem(at: URL(fileURLWithPath: fixture), to: source)
        let file = try await ImportFileSnapshot.load(source)
        let bridge = try NativeBridge(helper: URL(fileURLWithPath: helper), profile: profile)
        let context = try await bridge.context()
        func idle(_ bridge: NativeBridge, _ context: Context, ready: Bool = false) async throws -> NativeHostStatus {
            let deadline = ContinuousClock.now.advanced(by: .seconds(120))
            while ContinuousClock.now < deadline {
                let state = try await bridge.nativeStatus(context)
                if !state.busy && (!ready || state.runtime?.readiness.ready == true) { return state }
                if !state.busy, let code = state.errorCode { throw BridgeFailure(code) }
                try await Task.sleep(for: .milliseconds(100))
            }
            throw BridgeFailure("NATIVE_TIMEOUT")
        }
        func job(_ api: AuthoringAPI, _ id: String, expecting: String) async throws -> NativeImportJob {
            let deadline = ContinuousClock.now.advanced(by: .seconds(45))
            while ContinuousClock.now < deadline {
                let value = try await api.importJob(id)
                if value.state == expecting { return value }
                if value.state == "failed" && expecting != "failed" { throw AuthoringFailure(value.errorCode ?? "WORKER_FAILED") }
                try await Task.sleep(for: .milliseconds(100))
            }
            throw AuthoringFailure("IMPORT_TIMEOUT")
        }
        var active = bridge, selectedContext = context
        do {
            _ = try await bridge.prepare(context)
            _ = try await idle(bridge, context)
            _ = try await bridge.start(context)
            let started = try await idle(bridge, context, ready: true)
            let credentials = try await bridge.authoringSession(context)
            let api = try AuthoringAPI(port: started.configuration!.ports.web, tenantId: credentials.tenantId)
            _ = try await api.connect(credentials)
            let artist = try await api.createArtist(name: "GLB 합성 작가", bio: "Native import test")
            var metadata = ArtworkMetadata(); metadata.title = "실제 GLB 파일 시험"; metadata.rights.holder = artist.name; metadata.rights.licenseId = "CC0-1.0"; metadata.rights.licenseText = String(repeating: "합성 권리 문구 ", count: 1400); metadata.rights.permissions.export = true; metadata.provenance.source = "synthetic"
            let artwork = try await api.saveArtwork(id: nil, revision: nil, artistId: artist.id, metadata: metadata)
            let intent = NativeImportIntent(artworkId: artwork.id, mime: file.mime, sha256: file.sha256, bytes: file.data.count, scaleMeters: 1, rights: metadata.rights)
            var state = try await bridge.uploadState(context)
            let initial = state
            state.intent = intent; state.fileName = file.name
            state = try await bridge.saveUploadState(context, state: state)
            do { _ = try await bridge.saveUploadState(context, state: initial); XCTFail("Stale import state must fail") }
            catch let error as BridgeFailure { XCTAssertEqual(error.code, "NATIVE_UPLOAD_STATE_CHANGED") }
            let first = try await api.createImport(intent)
            XCTAssertEqual(first.state, "uploading")
            // Simulate loss of POST response before jobId is persisted; key was
            // already durable, so no second upload job may appear on reopen.
            _ = try await bridge.stop(context); _ = try await idle(bridge, context); try await bridge.close()
            let reopened = try NativeBridge(helper: URL(fileURLWithPath: helper), profile: profile)
            active = reopened; selectedContext = try await reopened.context()
            let recovered = try await reopened.uploadState(selectedContext)
            XCTAssertEqual(recovered.intent, intent); XCTAssertNil(recovered.jobId)
            _ = try await reopened.start(selectedContext); _ = try await idle(reopened, selectedContext, ready: true)
            let renewed = try await reopened.authoringSession(selectedContext)
            _ = try await api.connect(renewed)
            let same = try await api.createImport(recovered.intent!)
            XCTAssertEqual(first.id, same.id)
            let changed = NativeImportIntent(artworkId: artwork.id, idempotencyKey: intent.idempotencyKey, mime: intent.mime, sha256: intent.sha256, bytes: intent.bytes, scaleMeters: 2, rights: intent.rights)
            do { _ = try await api.createImport(changed); XCTFail("Different payload must not reuse key") }
            catch let error as AuthoringFailure { XCTAssertEqual(error.code, "IDEMPOTENCY_CONFLICT") }
            var saved = recovered; saved.jobId = same.id
            saved = try await reopened.saveUploadState(selectedContext, state: saved)
            do { _ = try await api.importAction(same.id, action: .complete); XCTFail("Missing bytes must refuse completion") }
            catch let error as AuthoringFailure { XCTAssertEqual(error.code, "UPLOAD_INCOMPLETE") }
            try await api.uploadImport(same.id, bytes: file.data)
            _ = try await api.importAction(same.id, action: .complete)
            let valid = try await job(api, same.id, expecting: "approved")
            let asset = try XCTUnwrap(valid.assetId)
            let approved = try await api.approveArtwork(artwork.id, revision: artwork.revision, assetId: asset)
            XCTAssertEqual(approved.approvedRevision, artwork.revision)
            XCTAssertEqual(approved.assets.first(where: { $0.id == asset })?.sha256, file.sha256)
            XCTAssertEqual(approved.assets.first(where: { $0.id == asset })?.bytes, Int64(file.data.count))
            _ = try await reopened.stop(selectedContext); let stop = try await idle(reopened, selectedContext)
            XCTAssertFalse(stop.needsShutdown)
            let installation = selectedContext.installations.first(where: { $0.id == selectedContext.activeId })!
            let blob = URL(fileURLWithPath: installation.path).appendingPathComponent("blobs/\(credentials.tenantId)/approved/\(file.sha256)")
            XCTAssertTrue(try Data(contentsOf: blob) == file.data)
            _ = try await reopened.start(selectedContext); _ = try await idle(reopened, selectedContext, ready: true)
            let renewedAgain = try await reopened.authoringSession(selectedContext)
            _ = try await api.connect(renewedAgain)
            let durable = try await api.artwork(artwork.id)
            XCTAssertEqual(durable.approvedAssetId, asset)
            let durableTransfer = try await reopened.uploadState(selectedContext)
            XCTAssertEqual(durableTransfer.jobId, same.id)
            // A structurally invalid GLB still has a correct transport hash.
            let badSource = root.appendingPathComponent("invalid.glb"); try Data("not a GLB".utf8).write(to: badSource)
            let badFile = try await ImportFileSnapshot.load(badSource)
            let badIntent = NativeImportIntent(artworkId: artwork.id, mime: badFile.mime, sha256: badFile.sha256, bytes: badFile.data.count, scaleMeters: 1, rights: metadata.rights)
            let invalid = try await api.createImport(badIntent); try await api.uploadImport(invalid.id, bytes: badFile.data)
            _ = try await api.importAction(invalid.id, action: .complete)
            let failed = try await job(api, invalid.id, expecting: "failed")
            XCTAssertEqual(failed.errorCode, "FORMAT_REJECTED")
            var retryable = failed
            while !retryable.canRetry { try await Task.sleep(for: .milliseconds(100)); retryable = try await api.importJob(invalid.id) }
            _ = try await api.importAction(invalid.id, action: .retry)
            let failedAgain = try await job(api, invalid.id, expecting: "failed")
            XCTAssertGreaterThan(failedAgain.attempts, failed.attempts)
            let cancelled = try await api.importAction(invalid.id, action: .cancel)
            XCTAssertEqual(cancelled.state, "cancelled")
            let preserved = try await api.artwork(artwork.id)
            XCTAssertEqual(preserved.approvedAssetId, asset); XCTAssertEqual(preserved.assets.count, 1)
            _ = try await reopened.stop(selectedContext); let final = try await idle(reopened, selectedContext); try await reopened.close()
            let receipt: [String: Any] = ["result":"PASS", "scope":"ACTUAL_NATIVE_GLB_IMPORT_APPROVAL_AND_RECOVERY_NOT_EXHIBITION_OEX", "artworkId":artwork.id, "jobId":same.id, "assetId":asset, "sha256":file.sha256, "bytes":file.data.count, "malformedJobId":invalid.id, "failedAttempts":failedAgain.attempts, "postgresExit":final.runtime?.exits["postgres"] ?? -999, "nodeExit":final.runtime?.exits["node"] ?? -999]
            try JSONSerialization.data(withJSONObject: receipt, options: [.prettyPrinted,.sortedKeys]).write(to: root.appendingPathComponent("result.json"), options: .atomic)
        } catch {
            _ = try? await active.stop(selectedContext); _ = try? await idle(active, selectedContext); try? await active.close()
            throw error
        }
    }
}
