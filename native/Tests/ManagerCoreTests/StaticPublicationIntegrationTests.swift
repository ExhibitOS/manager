// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

@MainActor
final class StaticPublicationIntegrationTests: XCTestCase {
    func testPinnedExporterProducesRealStaticFilesFromPreparedPublicProjection() async throws {
        let env = ProcessInfo.processInfo.environment
        guard let helper = env["EXHIBITOS_STATIC_TEST_HELPER"], let path = env["EXHIBITOS_STATIC_TEST_PROFILE"], let payload = env["EXHIBITOS_STATIC_TEST_PAYLOAD"], let output = env["EXHIBITOS_STATIC_TEST_OUTPUT"] else { throw XCTSkip("Explicit owned personal-workspace static publication fixture only") }
        XCTAssertFalse(FileManager.default.fileExists(atPath: output))
        let root = URL(fileURLWithPath: payload), bridge = try NativeBridge(helper: URL(fileURLWithPath: helper), profile: URL(fileURLWithPath: path)), context = try await bridge.context()
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
        do {
            let original = try await bridge.documentState(context)
            let journal = try await bridge.publicationState(context), completed = try XCTUnwrap(journal.completed)
            XCTAssertNil(journal.pending); XCTAssertEqual(completed.publication.status, "published")
            _ = try await bridge.start(context); let status = try await idle(ready: true)
            let process = Process(), stdout = Pipe(), stderr = Pipe()
            process.executableURL = root.appendingPathComponent("bin/node")
            process.arguments = [root.appendingPathComponent("platform/scripts/export-static-publication.mjs").path, "http://127.0.0.1:\(status.configuration!.ports.web)", completed.publication.publicationId, root.appendingPathComponent("platform/apps/web/dist-static").path, output]
            process.environment = ["PATH":"/usr/bin:/bin:/usr/sbin:/sbin", "HOME":NSHomeDirectory(), "LANG":"en_US.UTF-8"]
            process.standardOutput = stdout; process.standardError = stderr
            try process.run()
            let result = stdout.fileHandleForReading.readDataToEndOfFile(), errors = stderr.fileHandleForReading.readDataToEndOfFile()
            process.waitUntilExit()
            XCTAssertEqual(process.terminationStatus, 0, String(data: errors, encoding: .utf8) ?? "export error")
            let receipt = try XCTUnwrap(JSONSerialization.jsonObject(with: result) as? [String:Any])
            XCTAssertEqual(receipt["publicationId"] as? String, completed.publication.publicationId)
            XCTAssertEqual(receipt["revisionSha256"] as? String, completed.publication.revisionSha256)
            XCTAssertEqual(receipt["output"] as? String, output)
            let destination = URL(fileURLWithPath: output)
            XCTAssertTrue(FileManager.default.fileExists(atPath: destination.appendingPathComponent("index.html").path))
            XCTAssertFalse(FileManager.default.fileExists(atPath: destination.appendingPathComponent("studio.html").path))
            let unchanged = try await bridge.documentState(context)
            XCTAssertEqual(unchanged.draft, original.draft); XCTAssertEqual(unchanged.etag, original.etag)
            _ = try await bridge.stop(context); let stopped = try await idle(); try await bridge.close()
            var record = receipt
            record["scope"] = "ACTUAL_BUNDLED_STATIC_EXPORT_NOT_NATIVE_EXPORT_UI_OR_HOSTING"
            record["sourceDocumentPreserved"] = true
            record["nodeExit"] = stopped.runtime?.exits["node"] ?? -999; record["postgresExit"] = stopped.runtime?.exits["postgres"] ?? -999
            try JSONSerialization.data(withJSONObject: record, options: [.prettyPrinted,.sortedKeys]).write(to: destination.deletingLastPathComponent().appendingPathComponent("actual-static-export.json"), options:.atomic)
        } catch { _ = try? await bridge.stop(context); _ = try? await idle(); try? await bridge.close(); throw error }
    }
}
