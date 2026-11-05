// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

final class NativeAutomationFailureTests: XCTestCase {
    func testSelectedCreateLossAndKnownJobPersistenceUncertaintyKeepSafeScopedIDs() throws {
        let scope = (0..<4).map { _ in UUID().uuidString.lowercased() }, job = UUID().uuidString.lowercased()
        for jobID in [nil, job] {
            let error = NativeBoundedOEXUncertain(workspaceId: scope[0], tenantId: scope[1], principalId: scope[2], requestId: scope[3], jobId: jobID)
            let result = NativeAutomationFailure.value(error)
            XCTAssertEqual(result["code"], .string("AUTOMATION_OEX_UNCERTAIN"))
            XCTAssertEqual(result["retryAllowed"], .bool(false))
            XCTAssertEqual(result["retained"]["requestId"], .string(scope[3]))
            XCTAssertEqual(result["retained"]["jobId"], jobID.map(DocumentValue.string) ?? .null)
            guard case .object(let fields) = result["retained"] else { return XCTFail("retained IDs required") }
            XCTAssertEqual(Set(fields.keys), ["workspaceId", "tenantId", "principalId", "requestId", "jobId"])
        }
    }
    func testArbitraryFailureDescriptionIsNeverReported() {
        let result = NativeAutomationFailure.value(NSError(domain: "private-token-not-for-output", code: 9))
        XCTAssertEqual(result, .object(["code": .string("AUTOMATION_FAILED")]))
    }

    func testCloseOnlyReplaysConfirmedHelperExit() async throws {
        let folder = FileManager.default.temporaryDirectory.resolvingSymlinksInPath()
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: false,
                                                attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: folder) }
        for exitCode in [0, 9] {
            let helper = folder.appendingPathComponent("helper-\(exitCode)")
            try "#!/bin/sh\ncat >/dev/null\nexit \(exitCode)\n".write(to: helper, atomically: true, encoding: .utf8)
            try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: helper.path)
            let session = try NativeAutomationSession(helper: helper, profile: folder.appendingPathComponent("profile"),
                ports: ["web": 13200, "api": 13201, "database": 15432], outputRoot: folder,
                temporaryRoot: folder, inputs: [:])
            for _ in 0..<2 {
                do {
                    try await session.close()
                    XCTAssertEqual(exitCode, 0)
                } catch {
                    XCTAssertEqual(exitCode, 9)
                    XCTAssertEqual((error as? BridgeFailure)?.code, "NATIVE_HELPER_EXITED")
                }
            }
        }
    }
}
