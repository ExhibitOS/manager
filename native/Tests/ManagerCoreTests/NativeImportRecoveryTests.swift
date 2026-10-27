// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

@MainActor
final class NativeImportRecoveryTests: XCTestCase {
    func testActualOriginalUploadSurvivesRestartAndResumesWithoutLocalSelection() async throws {
        let env = ProcessInfo.processInfo.environment
        guard let helper = env["EXHIBITOS_GLB_TEST_HELPER"], let directory = env["EXHIBITOS_GLB_TEST_ROOT"],
              let fixture = env["EXHIBITOS_GLB_TEST_FIXTURE"] else {
            throw XCTSkip("Requires the explicitly owned isolated native original-custody harness")
        }
        let root = URL(fileURLWithPath: directory), profile = root.appendingPathComponent("profile")
        XCTAssertFalse(FileManager.default.fileExists(atPath: profile.path))
        let original = try await ImportFileSnapshot.load(URL(fileURLWithPath: fixture))
        func idle(_ bridge: NativeBridge, _ context: Context, ready: Bool = false) async throws -> NativeHostStatus {
            let deadline = ContinuousClock.now.advanced(by: .seconds(120))
            while ContinuousClock.now < deadline {
                let status = try await bridge.nativeStatus(context)
                if !status.busy && (!ready || status.runtime?.readiness.ready == true) { return status }
                if !status.busy, let code = status.errorCode { throw BridgeFailure(code) }
                try await Task.sleep(for: .milliseconds(100))
            }
            throw BridgeFailure("NATIVE_TIMEOUT")
        }
        var active: NativeBridge?, activeContext: Context?
        do {
            let bridge = try NativeBridge(helper: URL(fileURLWithPath: helper), profile: profile)
            active = bridge
            let context = try await bridge.context(); activeContext = context
            _ = try await bridge.prepare(context); _ = try await idle(bridge, context)
            _ = try await bridge.start(context)
            let started = try await idle(bridge, context, ready: true)
            let credentials = try await bridge.authoringSession(context)
            let api = try AuthoringAPI(port: started.configuration!.ports.web, tenantId: credentials.tenantId)
            _ = try await api.connect(credentials)
            let artist = try await api.createArtist(name: "원본 복구 합성 작가", bio: "Native same-intent recovery")
            var metadata = ArtworkMetadata()
            metadata.title = "원본 재선택 없는 검사 복구"
            metadata.rights.holder = artist.name; metadata.rights.licenseId = "CC0-1.0"
            metadata.rights.permissions.export = true; metadata.provenance.source = "synthetic"
            let artwork = try await api.saveArtwork(id: nil, revision: nil, artistId: artist.id, metadata: metadata)
            let intent = NativeImportIntent(artworkId: artwork.id, mime: original.mime, sha256: original.sha256,
                bytes: original.data.count, scaleMeters: 1, rights: metadata.rights)
            var state = try await bridge.uploadState(context)
            state.intent = intent; state.fileName = original.name
            state = try await bridge.saveUploadState(context, state: state)
            let job = try await api.createImport(intent)
            state.jobId = job.id; state = try await bridge.saveUploadState(context, state: state)
            // Interrupt after original upload but before completion/worker queue.
            // No derivative or asset is registered by the transfer acknowledgement.
            try await api.uploadImport(job.id, bytes: original.data)
            let before = try await api.importJob(job.id)
            XCTAssertEqual(before.state, "uploading"); XCTAssertNil(before.assetId)
            _ = try await bridge.stop(context); _ = try await idle(bridge, context); try await bridge.close()
            active = nil; activeContext = nil

            let reopened = try NativeBridge(helper: URL(fileURLWithPath: helper), profile: profile)
            active = reopened
            let next = try await reopened.context(); activeContext = next
            let recovered = try await reopened.uploadState(next)
            XCTAssertEqual(recovered.intent, intent); XCTAssertEqual(recovered.jobId, job.id)
            _ = try await reopened.start(next); _ = try await idle(reopened, next, ready: true)
            _ = try await api.connect(try await reopened.authoringSession(next))
            let resumed = try await NativeImportTransfer().run(intent: recovered.intent!, jobID: recovered.jobId,
                file: nil, transport: api, persistJob: { observed in
                    var saved = try await reopened.uploadState(next)
                    saved.jobId = observed.id
                    _ = try await reopened.saveUploadState(next, state: saved)
                }, isCurrent: { true })
            XCTAssertEqual(resumed.id, job.id)
            let deadline = ContinuousClock.now.advanced(by: .seconds(45))
            var observed = resumed
            while observed.state != "approved", ContinuousClock.now < deadline {
                if observed.state == "failed" { throw AuthoringFailure(observed.errorCode ?? "WORKER_FAILED") }
                try await Task.sleep(for: .milliseconds(100)); observed = try await api.importJob(job.id)
            }
            XCTAssertEqual(observed.state, "approved")
            let assetID = try XCTUnwrap(observed.assetId)
            let fresh = try await api.artwork(artwork.id)
            let asset = try XCTUnwrap(fresh.assets.first(where: { $0.id == assetID }))
            XCTAssertEqual(asset.sha256, original.sha256); XCTAssertEqual(asset.bytes, Int64(original.data.count))
            XCTAssertNil(fresh.approvedRevision) // file validation is not human artwork/rights approval
            _ = try await reopened.stop(next)
            let stopped = try await idle(reopened, next); try await reopened.close()
            active = nil; activeContext = nil
            let installation = try XCTUnwrap(next.installations.first(where: { $0.id == next.activeId }))
            let approved = URL(fileURLWithPath: installation.path)
                .appendingPathComponent("blobs/\(credentials.tenantId)/approved/\(original.sha256)")
            XCTAssertEqual(try Data(contentsOf: approved), original.data)
            let result: [String: Any] = ["status": "PASS_ACTUAL_SAME_ORIGINAL_RESTART_RESUME", "jobId": job.id,
                "assetId": assetID, "originalSHA256": original.sha256, "originalBytes": original.data.count,
                "localFileSuppliedForResume": false, "sameIntentAndJob": true, "approvedDiskBytesExact": true,
                "humanArtworkApprovalClaimed": false, "derivationClaimed": false,
                "nodeExit": stopped.runtime?.exits["node"] ?? -999,
                "postgresExit": stopped.runtime?.exits["postgres"] ?? -999]
            try JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys])
                .write(to: root.appendingPathComponent("original-resume-result.json"), options: .atomic)
        } catch {
            if let active, let activeContext {
                _ = try? await active.stop(activeContext); _ = try? await idle(active, activeContext); try? await active.close()
            }
            throw error
        }
    }
}
