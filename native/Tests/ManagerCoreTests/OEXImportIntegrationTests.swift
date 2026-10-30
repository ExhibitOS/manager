// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

@MainActor
final class OEXImportIntegrationTests: XCTestCase {
    func testActualOwnedOEXImportAndPendingBackupGuard() async throws {
        let env = ProcessInfo.processInfo.environment
        guard let helper = env["EXHIBITOS_OEX_TEST_HELPER"], let directory = env["EXHIBITOS_OEX_TEST_ROOT"], let input = env["EXHIBITOS_OEX_TEST_FILE"] else {
            throw XCTSkip("Requires a new isolated native profile, helper and explicit OEX fixture")
        }
        let root = URL(fileURLWithPath: directory, isDirectory: true), profile = root.appendingPathComponent("profile")
        XCTAssertFalse(FileManager.default.fileExists(atPath: profile.path))
        let bridge = try NativeBridge(helper: URL(fileURLWithPath: helper), profile: profile)
        let context = try await bridge.context()
        func idle(ready: Bool = false) async throws -> NativeHostStatus {
            let deadline = ContinuousClock.now.advanced(by: .seconds(120))
            while ContinuousClock.now < deadline {
                let value = try await bridge.nativeStatus(context)
                if !value.busy && (!ready || value.runtime?.readiness.ready == true) { return value }
                if !value.busy, let code = value.errorCode { throw BridgeFailure(code) }
                try await Task.sleep(for: .milliseconds(150))
            }
            throw BridgeFailure("NATIVE_TIMEOUT")
        }
        do {
            _ = try await bridge.prepare(context); _ = try await idle()
            let key = root.appendingPathComponent("synthetic-backup.key"), destination = root.appendingPathComponent("must-not-exist-backup")
            try await bridge.createBackupKey(context, at: key)
            _ = try await bridge.start(context); let started = try await idle(ready: true)
            let credentials = try await bridge.authoringSession(context)
            let api = try AuthoringAPI(port: started.configuration!.ports.web, tenantId: credentials.tenantId)
            _ = try await api.connect(credentials)
            let file = try await NativeOEXImportFile.load(URL(fileURLWithPath: input))
            let initial = try await bridge.oexImportState(context)
            let result = try await NativeOEXImportTransfer.run(state: initial, file: file, transport: api, persist: { value in
                try await bridge.saveOEXImportState(context, state: value)
            }, isCurrent: { true })
            let held = try await bridge.oexImportState(context)
            XCTAssertEqual(held.intent?.sha256, file.archive.sha256); XCTAssertEqual(held.jobId, result.1.id)
            do { _ = try await bridge.backup(context, key: key, destination: destination); XCTFail("pending import must refuse backup before stopping runtime") }
            catch let failure as BridgeFailure { XCTAssertEqual(failure.code, "NATIVE_OEX_IMPORT_PENDING") }
            let refused = try await bridge.nativeStatus(context)
            XCTAssertTrue(refused.runtime?.readiness.ready == true)
            XCTAssertFalse(FileManager.default.fileExists(atPath: destination.path))
            var job = result.1; let deadline = ContinuousClock.now.advanced(by: .seconds(120))
            while job.state == "queued" || job.state == "processing" {
                guard ContinuousClock.now < deadline else { throw BridgeFailure("NATIVE_TIMEOUT") }
                try await Task.sleep(for: .milliseconds(250)); job = try await api.oexImportJob(job.id)
            }
            XCTAssertEqual(job.state, "complete", job.errorCode ?? "")
            let id = try XCTUnwrap(job.result?.exhibitionId), reopened = try await api.exhibition(id)
            XCTAssertEqual(reopened.draft.id.lowercased(), id.lowercased())
            let output = try await api.exportOEX(exhibitionId: id, etag: reopened.etag)
            XCTAssertGreaterThan(output.data.count, 22)
            _ = try await bridge.stop(context); let stopped = try await idle()
            XCTAssertFalse(stopped.needsShutdown)
            XCTAssertEqual(stopped.runtime?.exits["postgres"], 0); XCTAssertEqual(stopped.runtime?.exits["node"], 0)
            let summary: [String: Any] = ["result":"PASS", "scope":"OWNED_NATIVE_HELPER_SWIFT_OEX_IMPORT_AND_BACKUP_PENDING_GUARD_NOT_GUI", "inputSHA256":file.archive.sha256, "outputSHA256":output.sha256, "sameJobId":held.jobId == job.id, "backupPendingRefused":true, "originalDestinationAbsent":true, "postgresExit":stopped.runtime?.exits["postgres"] ?? -999, "nodeExit":stopped.runtime?.exits["node"] ?? -999]
            try JSONSerialization.data(withJSONObject: summary, options: [.prettyPrinted, .sortedKeys]).write(to: root.appendingPathComponent("result.json"), options: .atomic)
            try await bridge.close()
        } catch {
            _ = try? await bridge.stop(context); _ = try? await idle(); try? await bridge.close(); throw error
        }
    }
}
