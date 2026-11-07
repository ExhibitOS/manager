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

    func testExistingWorkspaceV5RejectsFreshSelectionAndInvalidScopeBeforeLaunchingHelper() throws {
        let workspace = UUID().uuidString.lowercased()
        let identity = try JSONDecoder().decode(NativeAutomationSession.ExistingWorkspaceV5.self, from: Data("""
            {"workspaceId":"\(workspace)","tenantId":"\(workspace)","principalId":"\(workspace)","manifestSHA256":"\(String(repeating: "a", count: 64))"}
            """.utf8))
        let missing = URL(fileURLWithPath: "/missing/exhibitos-native")
        for (selected, runtime) in [(true, missing), (false, nil)] {
            XCTAssertThrowsError(try NativeAutomationSession(helper: missing, profile: missing,
                ports: [:], outputRoot: missing, temporaryRoot: missing, inputs: [:],
                selectedOEX10: selected, workspaceV5ValidationRuntime: runtime, existingWorkspaceV5: identity)) {
                XCTAssertEqual(($0 as? BridgeFailure)?.code, "AUTOMATION_CONFIGURATION_INVALID")
            }
        }
        XCTAssertThrowsError(try NativeAutomationSession(helper: missing, profile: missing,
            ports: [:], outputRoot: missing, temporaryRoot: missing, inputs: [:],
            existingWorkspaceV5: identity, workspaceV6ValidationRuntime: missing)) {
            XCTAssertEqual(($0 as? BridgeFailure)?.code, "AUTOMATION_CONFIGURATION_INVALID")
        }
        XCTAssertThrowsError(try NativeAutomationSession(helper: missing, profile: missing,
            ports: [:], outputRoot: missing, temporaryRoot: missing, inputs: [:],
            workspaceV5ValidationRuntime: missing, existingWorkspaceV5: identity, workspaceV7ValidationRuntime: missing)) {
            XCTAssertEqual(($0 as? BridgeFailure)?.code, "AUTOMATION_CONFIGURATION_INVALID")
        }
        let invalid = try JSONDecoder().decode(NativeAutomationSession.ExistingWorkspaceV5.self, from: Data("""
            {"workspaceId":"wrong","tenantId":"\(workspace)","principalId":"\(workspace)","manifestSHA256":"\(String(repeating: "a", count: 64))"}
            """.utf8))
        XCTAssertThrowsError(try invalid.check())
    }

    func testDraft12CompletionReceiptRequiresSuccessfulReopen() async throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: folder) }
        let output = folder.appendingPathComponent("receipt.json")
        let receipt: [String: DocumentValue] = ["archiveFormat": .string("1.0.0-draft.12")]
        let record: (DocumentValue) throws -> DocumentValue = { value in
            try JSONEncoder().encode(value).write(to: output, options: .withoutOverwriting)
            return value
        }

        do {
            _ = try await NativeAutomationSession.recordCompletedDraft12Import(receipt: receipt, reopen: {
                throw BridgeFailure("AUTOMATION_DRAIN_UNCERTAIN")
            }, record: record)
            XCTFail("A failed reopen must propagate the error")
        } catch let error as BridgeFailure {
            XCTAssertEqual(error.code, "AUTOMATION_DRAIN_UNCERTAIN")
        }
        XCTAssertFalse(FileManager.default.fileExists(atPath: output.path))

        let reopen: DocumentValue = .object([
            "previousRuntimeJobId": .string(UUID().uuidString),
            "runtimeJobId": .string(UUID().uuidString),
            "studioRevision": .integer(2)
        ])
        let result = try await NativeAutomationSession.recordCompletedDraft12Import(receipt: receipt, reopen: {
            XCTAssertFalse(FileManager.default.fileExists(atPath: output.path))
            return reopen
        }, record: record)
        XCTAssertEqual(result["reopen"], reopen)
        XCTAssertEqual(try JSONDecoder().decode(DocumentValue.self, from: Data(contentsOf: output)), result)
    }

    func testCloseOnlyReplaysConfirmedHelperExit() async throws {
        let folder = FileManager.default.temporaryDirectory.resolvingSymlinksInPath()
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: false,
                                                attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: folder) }
        let output = folder.appendingPathComponent("output", isDirectory: true)
        try FileManager.default.createDirectory(at: output, withIntermediateDirectories: false,
                                                attributes: [.posixPermissions: 0o700])
        for exitCode in [0, 9] {
            let helper = folder.appendingPathComponent("helper-\(exitCode)")
            try "#!/bin/sh\ncat >/dev/null\nexit \(exitCode)\n".write(to: helper, atomically: true, encoding: .utf8)
            try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: helper.path)
            let session = try NativeAutomationSession(helper: helper, profile: folder.appendingPathComponent("profile"),
                ports: ["web": 13200, "api": 13201, "database": 15432], outputRoot: output,
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
