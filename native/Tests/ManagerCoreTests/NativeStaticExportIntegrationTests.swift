// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

@MainActor
final class NativeStaticExportIntegrationTests: XCTestCase {
    func testOwnedAsyncExportCancelExistingOutputRefusalAndLostCompletionRecovery() async throws {
        let env = ProcessInfo.processInfo.environment
        guard let helper = env["EXHIBITOS_NATIVE_STATIC_TEST_HELPER"], let path = env["EXHIBITOS_NATIVE_STATIC_TEST_PROFILE"], let directory = env["EXHIBITOS_NATIVE_STATIC_TEST_ROOT"] else { throw XCTSkip("Explicit owned native r10 personal workspace only") }
        let root = URL(fileURLWithPath: directory), profile = URL(fileURLWithPath: path)
        var bridge = try NativeBridge(helper: URL(fileURLWithPath: helper), profile: profile), context = try await bridge.context()
        func idle(ready: Bool = false) async throws -> NativeHostStatus {
            let end = ContinuousClock.now.advanced(by: .seconds(120))
            while ContinuousClock.now < end {
                let value = try await bridge.nativeStatus(context)
                if !value.busy { if let code = value.errorCode { throw BridgeFailure(code) }; if !ready || value.runtime?.readiness.ready == true { return value } }
                try await Task.sleep(for: .milliseconds(100))
            }
            throw BridgeFailure("NATIVE_TIMEOUT")
        }
        func finished() async throws -> NativeStaticExportStatus {
            let end = ContinuousClock.now.advanced(by: .seconds(60))
            while ContinuousClock.now < end {
                if let value = try await bridge.staticExportStatus(context), !value.running { return value }
                try await Task.sleep(for: .milliseconds(50))
            }
            throw BridgeFailure("NATIVE_TIMEOUT")
        }
        do {
            let initialExport = try await bridge.staticExportStatus(context)
            XCTAssertNil(initialExport)
            let document = try await bridge.documentState(context), draft = try XCTUnwrap(document.draft), etag = try XCTUnwrap(document.etag)
            XCTAssertFalse(document.dirty)
            _ = try await bridge.start(context); let runtime = try await idle(ready: true)
            let credentials = try await bridge.authoringSession(context)
            let api = try AuthoringAPI(port: runtime.configuration!.ports.web, tenantId: credentials.tenantId)
            _ = try await api.connect(credentials)
            var journal = try await bridge.publicationState(context)
            XCTAssertNil(journal.pending); XCTAssertNil(journal.completed)
            var pending = try NativePublicationPending(source: draft, etag: etag)
            journal.pending = pending; journal = try await bridge.savePublicationState(context, state: journal)
            let copy = try await api.saveExhibition(pending.snapshot, requestId: pending.saveRequestId, etag: nil)
            pending.snapshotEtag = copy.etag; journal.pending = pending; journal = try await bridge.savePublicationState(context, state: journal)
            let publication = try await api.preparePublication(exhibitionId: copy.draft.id, etag: copy.etag, requestId: pending.publishRequestId)
            journal.completed = NativePublicationCompleted(pending: pending, publication: publication); journal.pending = nil
            _ = try await bridge.savePublicationState(context, state: journal)
            let output = root.appendingPathComponent("native-static-exhibition")
            let start = ContinuousClock.now
            let running = try await bridge.startStaticExport(context, publicationId: publication.publicationId, output: output)
            XCTAssertTrue(running.running); XCTAssertLessThan(start.duration(to: .now), .seconds(1))
            // The helper's pipe remains available while its child builds files.
            let probe = ContinuousClock.now; _ = try await bridge.presentation()
            XCTAssertLessThan(probe.duration(to: .now), .seconds(1))
            let completed = try await finished()
            XCTAssertEqual(completed.state, "completed", completed.errorCode ?? "")
            let receipt = try XCTUnwrap(completed.receipt)
            XCTAssertEqual(receipt.publicationId, publication.publicationId); XCTAssertEqual(receipt.revisionSha256, publication.revisionSha256)
            XCTAssertGreaterThan(receipt.files, 10); XCTAssertEqual(receipt.output, output.path)
            let manifestURL = output.appendingPathComponent("static-export-manifest.json"), manifest = try Data(contentsOf: manifestURL)
            do { _ = try await bridge.startStaticExport(context, publicationId: publication.publicationId, output: output); XCTFail("Existing output must never be replaced") }
            catch let error as BridgeFailure { XCTAssertEqual(error.code, "NATIVE_STATIC_OUTPUT_EXISTS") }
            XCTAssertEqual(try Data(contentsOf: manifestURL), manifest)
            let files = try FileManager.default.contentsOfDirectory(at: profile, includingPropertiesForKeys: nil)
            let jobFile = try XCTUnwrap(files.first(where: { $0.lastPathComponent.hasPrefix("native-static-export-") && $0.pathExtension == "json" }))
            let completedJournal = try Data(contentsOf: jobFile)
            XCTAssertEqual((try FileManager.default.attributesOfItem(atPath: jobFile.path)[.posixPermissions] as? NSNumber)?.intValue, 0o600)
            let cancelledOutput = root.appendingPathComponent("cancelled-static-exhibition")
            _ = try await bridge.startStaticExport(context, publicationId: publication.publicationId, output: cancelledOutput)
            _ = try await bridge.cancelStaticExport(context)
            let cancelled = try await finished()
            XCTAssertEqual(cancelled.state, "cancelled", cancelled.errorCode ?? "")
            XCTAssertEqual(try Data(contentsOf: manifestURL), manifest)
            let unchanged = try await bridge.documentState(context)
            XCTAssertEqual(unchanged.draft, document.draft); XCTAssertEqual(unchanged.etag, document.etag)
            _ = try await bridge.stop(context); let stopped = try await idle(); try await bridge.close()
            // Isolated fault injection: preserve both real receipts, then model a
            // successful final rename whose completion journal was lost. No
            // live process or user's source is modified.
            try completedJournal.write(to: root.appendingPathComponent("completed-job-before-fault.json"), options: .atomic)
            try Data(contentsOf: jobFile).write(to: root.appendingPathComponent("cancelled-job-before-fault.json"), options: .atomic)
            var lost = try XCTUnwrap(JSONSerialization.jsonObject(with: completedJournal) as? [String: Any]); lost["state"] = "running"; lost["receipt"] = NSNull(); lost["errorCode"] = NSNull()
            try JSONSerialization.data(withJSONObject: lost).write(to: jobFile, options: .atomic)
            try FileManager.default.setAttributes([.posixPermissions:0o600], ofItemAtPath:jobFile.path)
            bridge = try NativeBridge(helper: URL(fileURLWithPath: helper), profile: profile); context = try await bridge.context()
            let recovered = try await finished()
            XCTAssertEqual(recovered.state, "completed", recovered.errorCode ?? ""); XCTAssertEqual(recovered.receipt?.manifestSha256, receipt.manifestSha256)
            let remainsStopped = try await bridge.nativeStatus(context); XCTAssertFalse(remainsStopped.runtime?.readiness.ready == true)
            try await bridge.close()
            let report: [String: Any] = ["scope":"ACTUAL_NATIVE_ASYNC_STATIC_EXPORT_NOT_GUI_OR_HOSTING","publicationId":publication.publicationId,"files":receipt.files,"bytes":receipt.bytes,"output":receipt.output,"manifestSHA256":receipt.manifestSha256,"pipeResponsive":true,"existingOutputRefusedAndPreserved":true,"cancelledOwnedChild":true,"privateJournal0600":true,"lostCompletionRecoveredWithServicesStopped":true,"sourceDocumentPreserved":true,"nodeExit":stopped.runtime?.exits["node"] ?? -999,"postgresExit":stopped.runtime?.exits["postgres"] ?? -999]
            try JSONSerialization.data(withJSONObject:report,options:[.prettyPrinted,.sortedKeys]).write(to:root.appendingPathComponent("native-static-result.json"),options:.atomic)
        } catch { _ = try? await bridge.cancelStaticExport(context); _ = try? await bridge.stop(context); _ = try? await idle(); try? await bridge.close(); throw error }
    }
}
