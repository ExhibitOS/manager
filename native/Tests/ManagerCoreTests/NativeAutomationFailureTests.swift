// SPDX-License-Identifier: Apache-2.0
import XCTest
import Darwin
@testable import ManagerCore

final class NativeAutomationFailureTests: XCTestCase {
    #if DEBUG
    func testPlacedExitRuntimeUsesPinnedStartupDrain() {
        XCTAssertTrue(AuthoringAPI.supportsPinnedStartupDrain(
            manifest: "1245b842efb55525ebf0c307f984391431552ce19d6f59874dbcd8435b8e39d2"
        ))
        XCTAssertFalse(AuthoringAPI.supportsPinnedStartupDrain(manifest: String(repeating: "0", count: 64)))
    }
    #endif

    func testPreReadyPreservationAutomationRejectsSaveAndOtherCommands() throws {
        for manifest in [
            "baf33be8865ee1c558874702ca8a772db90a9c59c3ab28759e7fc6ae8d608c86",
            "5cb2431e561c21e1463f78faf938a5cfc3807c9dd690a545f3474769c1c7f4a9"
        ] {
            XCTAssertNoThrow(try NativeAutomationSession.checkPreservationOperation("read", manifest: manifest, allowed: "read"))
            XCTAssertNoThrow(try NativeAutomationSession.checkPreservationOperation("close", manifest: manifest, allowed: "close"))
            for operation in ["save", "load-copy", "reopen-saved", "export-static", "close"] {
                XCTAssertThrowsError(try NativeAutomationSession.checkPreservationOperation(operation, manifest: manifest, allowed: "read")) {
                    XCTAssertEqual(($0 as? BridgeFailure)?.code, "AUTOMATION_OPERATION_UNSUPPORTED")
                }
            }
            for operation in ["cancel", "status", "read"] {
                XCTAssertThrowsError(try NativeAutomationSession.checkPreservationOperation(operation, manifest: manifest, allowed: "close")) {
                    XCTAssertEqual(($0 as? BridgeFailure)?.code, "AUTOMATION_OPERATION_UNSUPPORTED")
                }
            }
        }
        let ordinary = "cbf4214bbf755216e8173190ce98ee32fb3f3708bd9c6a25e394eb3ce30e0a0a"
        XCTAssertNoThrow(try NativeAutomationSession.checkPreservationOperation("save", manifest: ordinary, allowed: "read"))
    }

    func testInstalledExportCommandsRequireExactScopeAndSavedRevisionFields() throws {
        let scope: Set<String> = ["version", "requestId", "operation", "capability", "workspaceId", "tenantId", "principalId"]
        let export = scope.union(["expectedRevision", "expectedEditVersion", "outputPath"])
        XCTAssertNoThrow(try NativeAutomationSession.checkInstalledExportRequest("read", fields: scope))
        for operation in ["export-oex", "export-static", "export-static-status"] {
            XCTAssertNoThrow(try NativeAutomationSession.checkInstalledExportRequest(operation, fields: export))
            XCTAssertThrowsError(try NativeAutomationSession.checkInstalledExportRequest(operation, fields: export.subtracting(["expectedEditVersion"]))) {
                XCTAssertEqual(($0 as? BridgeFailure)?.code, "AUTOMATION_PROTOCOL_INVALID")
            }
        }
        XCTAssertThrowsError(try NativeAutomationSession.checkInstalledExportRequest("read", fields: export)) {
            XCTAssertEqual(($0 as? BridgeFailure)?.code, "AUTOMATION_PROTOCOL_INVALID")
        }
        for operation in ["save", "import-image", "export-static-native-original", "export-static-saved-head"] {
            XCTAssertThrowsError(try NativeAutomationSession.checkInstalledExportRequest(operation, fields: scope)) {
                XCTAssertEqual(($0 as? BridgeFailure)?.code, "AUTOMATION_OPERATION_UNSUPPORTED")
            }
        }
    }

    func testInstalledExportOutputRootMustBePrivateCanonicalAndOutsideProfile() throws {
        let files = FileManager.default
        let parent = files.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent(UUID().uuidString, isDirectory: true)
        let profile = parent.appendingPathComponent("profile", isDirectory: true)
        let output = parent.appendingPathComponent("output", isDirectory: true)
        let nested = profile.appendingPathComponent("nested", isDirectory: true)
        let link = parent.appendingPathComponent("link", isDirectory: true)
        try files.createDirectory(at: parent, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? files.removeItem(at: parent) }
        for path in [profile, output, nested] {
            try files.createDirectory(at: path, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        }
        try files.createSymbolicLink(at: link, withDestinationURL: output)
        XCTAssertNoThrow(try NativeAutomationSession.exportRootIdentity(output, profile: profile))
        for invalid in [profile, nested, parent, link] {
            XCTAssertThrowsError(try NativeAutomationSession.exportRootIdentity(invalid, profile: profile)) {
                XCTAssertEqual(($0 as? BridgeFailure)?.code, "AUTOMATION_OUTPUT_INVALID")
            }
        }
        try files.setAttributes([.posixPermissions: 0o755], ofItemAtPath: output.path)
        XCTAssertThrowsError(try NativeAutomationSession.exportRootIdentity(output, profile: profile))
    }

    func testBoundedOEXRefusesReplacementDirectoryAtWrite() async throws {
        let files = FileManager.default
        let base = files.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent(UUID().uuidString)
        let profile = base.appendingPathComponent("profile", isDirectory: true)
        let root = base.appendingPathComponent("output", isDirectory: true)
        let moved = base.appendingPathComponent("moved", isDirectory: true)
        try files.createDirectory(at: base, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? files.removeItem(at: base) }
        for path in [profile, root] {
            try files.createDirectory(at: path, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        }
        let identity = try NativeAutomationSession.exportRootIdentity(root, profile: profile)
        try files.moveItem(at: root, to: moved)
        try files.createDirectory(at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        let data = Data([80, 75, 3, 4] + Array(repeating: 0, count: 22))
        let archive = try NativeOEXArchive(data: data)
        let destination = root.appendingPathComponent("saved.oex")
        do {
            try await archive.writeBoundedVerified(to: destination, expectedParent: identity)
            XCTFail("Accepted a replacement output directory")
        } catch let error as BridgeFailure {
            XCTAssertEqual(error.code, "AUTOMATION_OUTPUT_INVALID")
        }
        XCTAssertTrue(try files.contentsOfDirectory(atPath: root.path).isEmpty)
        XCTAssertTrue(try files.contentsOfDirectory(atPath: moved.path).isEmpty)
        try files.removeItem(at: root)
        try files.moveItem(at: moved, to: root)
        let held = Darwin.open(root.path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        XCTAssertGreaterThanOrEqual(held, 0)
        defer { if held >= 0 { Darwin.close(held) } }
        let confirmed = try await archive.writeBoundedVerified(to: destination, expectedParent: identity)
        XCTAssertTrue(confirmed)
        try files.moveItem(at: root, to: moved)
        try files.createDirectory(at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        XCTAssertFalse(NativeOEXArchive.confirmsCommittedParent(held, path: root.path, expected: identity))
        XCTAssertTrue(try files.contentsOfDirectory(atPath: root.path).isEmpty)
        XCTAssertEqual(try Data(contentsOf: moved.appendingPathComponent(destination.lastPathComponent)), data)
    }

    func testArtworkStatusBypassesCachedReadyResponseWhileMutationReplayStaysBound() throws {
        let request = Data("same request".utf8)
        let stale: DocumentValue = .object(["job": .object(["state": .string("approved")])])
        let current: DocumentValue = .object(["job": .object(["state": .string("failed")])])
        let prior = (request, stale)
        let status = try NativeAutomationSession.replayResponse(for: "artwork-status", prior: prior, matching: request)
        XCTAssertEqual(status ?? current, current)
        XCTAssertNil(try NativeAutomationSession.replayResponse(for: "export-static-saved-head", prior: prior, matching: request))
        XCTAssertEqual(try NativeAutomationSession.replayResponse(for: "artwork-approve", prior: prior, matching: request), stale)
        XCTAssertThrowsError(try NativeAutomationSession.replayResponse(
            for: "artwork-approve", prior: prior, matching: Data("changed request".utf8))) {
            XCTAssertEqual(($0 as? BridgeFailure)?.code, "AUTOMATION_REQUEST_CHANGED")
        }
    }

    func testStaticExportReusesMatchingStatusAndAllowsNewExportAfterTerminalStatus() throws {
        let id = UUID().uuidString.lowercased()
        func status(_ state: String) -> NativeStaticExportStatus {
            .init(format: 1, jobId: id, workspaceId: id, tenantId: id, principalId: id,
                  publicationId: id, revisionSha256: String(repeating: "a", count: 64),
                  output: "/exports/first", state: state, receipt: nil, errorCode: nil, privateEditor: true)
        }
        for state in ["completed", "failed", "cancelled", "interrupted"] {
            XCTAssertTrue(try status(state).canReuse(publicationId: id, output: "/exports/first"))
            XCTAssertFalse(try status(state).canReuse(publicationId: id, output: "/exports/second"))
        }
        XCTAssertTrue(try status("running").canReuse(publicationId: id, output: "/exports/first"))
        XCTAssertThrowsError(try status("running").canReuse(publicationId: id, output: "/exports/second")) {
            XCTAssertEqual(($0 as? BridgeFailure)?.code, "AUTOMATION_EXPORT_CHANGED")
        }
        XCTAssertFalse(try status("completed").canReuse(publicationId: UUID().uuidString.lowercased(), output: "/exports/first"))
    }

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

    #if DEBUG
    func testAdoptedOriginalOutputAdmitsOnlyTheExactReviewedOrdinaryRuntime() throws {
        let manifest = "49f05729bfe39c7e2a228888c6541f5e22e08057f1e07bf04a66055a2016e861"
        let owner = UUID().uuidString.lowercased()
        let missing = URL(fileURLWithPath: "/missing/exhibitos-native")
        for (pin, expected) in [(manifest, "AUTOMATION_SCOPE_CHANGED"),
                                ("e41df146d7097fd6964564de3d2f82b4bb766dba5377ec7f938896955fea39b0", "AUTOMATION_SCOPE_CHANGED"),
                                ("3e05e41286c517d5304299bdd987b8e5b6fe08ab8396dee3804feeb0f0c4f670", "AUTOMATION_SCOPE_CHANGED"),
                                ("eac64ed106bdac283c1aa0c6c3dee5541c98919ed4a67bd9e5509b21496b5bce", "AUTOMATION_SCOPE_CHANGED"),
                                (String(repeating: "a", count: 64), "AUTOMATION_CONFIGURATION_INVALID")] {
            let data = Data("""
                {"workspaceId":"\(owner)","tenantId":"\(owner)","principalId":"\(owner)","manifestSHA256":"\(pin)"}
                """.utf8)
            let identity = try JSONDecoder().decode(NativeAutomationSession.ExistingWorkspaceV5.self, from: data)
            XCTAssertThrowsError(try NativeAutomationSession(helper: missing, profile: missing,
                ports: [:], outputRoot: missing, temporaryRoot: missing, inputs: [:], existingWorkspaceV5: identity)) {
                XCTAssertEqual(($0 as? BridgeFailure)?.code, expected)
            }
        }
    }

    func testOrdinaryExistingThumbnailOexRequiresExactReviewedRuntime() throws {
        let previous = "f21b1378b39d11fd6ea8f9c89636912f233f884af176e2deb9856dea3ae77b53"
        let corrected = "0efbf042ad20b7cd8963751e5cfe5f343ead93d6ced6164e8bbd08566e378c99"
        let owner = UUID().uuidString.lowercased()
        let missing = URL(fileURLWithPath: "/missing/exhibitos-native")
        for (manifest, expected) in [(previous, "AUTOMATION_CONFIGURATION_INVALID"),
                                     (corrected, "AUTOMATION_SCOPE_CHANGED")] {
            let data = Data("""
                {"workspaceId":"\(owner)","tenantId":"\(owner)","principalId":"\(owner)","manifestSHA256":"\(manifest)"}
                """.utf8)
            let identity = try JSONDecoder().decode(NativeAutomationSession.ExistingWorkspaceV5.self, from: data)
            XCTAssertThrowsError(try NativeAutomationSession(helper: missing, profile: missing,
                ports: [:], outputRoot: missing, temporaryRoot: missing, inputs: [:], existingWorkspaceV5: identity)) {
                XCTAssertEqual(($0 as? BridgeFailure)?.code, expected)
            }
        }
    }
    #endif

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

    func testExplicitImageProfileRefusesDebugRuntimeBeforeStart() async throws {
        let folder = FileManager.default.temporaryDirectory.resolvingSymlinksInPath()
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        let profile = folder.appendingPathComponent("profile", isDirectory: true)
        try FileManager.default.createDirectory(at: profile, withIntermediateDirectories: true,
                                                attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: folder) }
        let helper = folder.appendingPathComponent("helper")
        try #"""
        #!/bin/sh
        [ "$#" = 3 ] && [ "$1" = --profile ] && [ "$2" = "${0%/*}/profile" ] &&
            [ "$3" = --existing-managed-profile ] || exit 8
        IFS= read -r request || exit 9
        printf '%s\n' '{"version":1,"id":1,"result":{"selectionToken":"fixture","activeId":"fixture","installations":[]}}'
        IFS= read -r request || exit 9
        printf '%s\n' '{"version":1,"id":2,"result":{"busy":false,"operation":"idle","accountRequired":false,"configuration":{"kind":"personal","ports":{"web":13200,"api":13201,"database":15432},"manifestSha256":"257ea9d3d0ef1064494da5c68c2d9b9dd2430ac9f7882ae85e3399c49420ac96"}}}'
        if IFS= read -r request; then exit 9; fi
        exit 0
        """#.write(to: helper, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: helper.path)
        let session = try NativeAutomationSession(installedImageCLIHelper: helper, profile: profile,
                                                  requireExistingManagedProfile: true)
        do {
            _ = try await session.open()
            XCTFail("An explicit profile must require release admission")
        } catch let error as AuthoringFailure {
            XCTAssertEqual(error.code, "IMAGE_PREPARATION_UNAVAILABLE")
        }
        try await session.close()
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

    #if DEBUG
    func testF738CancellationKeepsStatusChannelUntilStartupSettles() async throws {
        try await checkExistingStartupCancellation(
            manifest: "f738f33c777de39629b6a0ab2dd3a477b45d078b738189e173d00d2cd7ab652a")
    }

    func testThumbnailOexCancellationKeepsStatusChannelUntilStartupSettles() async throws {
        try await checkExistingStartupCancellation(
            manifest: "0efbf042ad20b7cd8963751e5cfe5f343ead93d6ced6164e8bbd08566e378c99")
    }

    private func checkExistingStartupCancellation(manifest: String) async throws {
        let folder = FileManager.default.temporaryDirectory.resolvingSymlinksInPath()
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        let profile = folder.appendingPathComponent("profile", isDirectory: true)
        let output = folder.appendingPathComponent("output", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        for path in [profile, output] {
            try FileManager.default.createDirectory(at: path, withIntermediateDirectories: true,
                                                    attributes: [.posixPermissions: 0o700])
        }
        let scope = UUID().uuidString.lowercased()
        let owner = try JSONDecoder().decode(NativeAutomationSession.ExistingWorkspaceV5.self, from: Data("""
            {"workspaceId":"\(scope)","tenantId":"\(scope)","principalId":"\(scope)","manifestSHA256":"\(manifest)"}
            """.utf8))
        let draft = String(decoding: try JSONEncoder().encode(NativeExhibitionDraft.new()), as: UTF8.self)
        let helper = folder.appendingPathComponent("helper")
        try """
        #!/bin/sh
        root=${0%/*}
        IFS= read -r request || exit 9
        printf '%s\\n' '{"version":1,"id":1,"result":{"selectionToken":"fixture","activeId":"fixture","installations":[]}}'
        IFS= read -r request || exit 9
        printf '%s\\n' '{"version":1,"id":2,"result":{"busy":false,"operation":"idle","accountRequired":false,"configuration":{"kind":"personal","tenantId":"\(scope)","ports":{"web":13200,"api":13201,"database":15432},"manifestSha256":"\(manifest)"}}}'
        IFS= read -r request || exit 9
        printf '%s\\n' '{"version":1,"id":3,"result":{"format":1,"revision":1,"workspaceId":"\(scope)","tenantId":"\(scope)","principalId":"\(scope)","savedEditVersion":1,"etag":"saved","draft":\(draft)}}'
        IFS= read -r request || exit 9
        printf '%s\\n' '{"version":1,"id":4,"result":{"busy":true,"operation":"start","accountRequired":false}}'
        IFS= read -r request || exit 9
        touch "$root/busy"
        while [ ! -f "$root/cancelled" ]; do sleep .02; done
        printf '%s\\n' '{"version":1,"id":5,"result":{"busy":true,"operation":"start","accountRequired":false}}'
        IFS= read -r request || exit 9
        touch "$root/terminal"
        printf '%s\\n' '{"version":1,"id":6,"result":{"busy":false,"operation":"start","accountRequired":false}}'
        exit 0
        """.write(to: helper, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: helper.path)
        let session = try NativeAutomationSession(helper: helper, profile: profile,
            ports: ["web": 13200, "api": 13201, "database": 15432], outputRoot: output,
            temporaryRoot: folder, inputs: [:], workspaceV5ValidationRuntime: folder, existingWorkspaceV5: owner)
        let task = Task { try await session.open() }
        let busy = folder.appendingPathComponent("busy").path
        for _ in 0..<100 where !FileManager.default.fileExists(atPath: busy) {
            try await Task.sleep(for: .milliseconds(20))
        }
        XCTAssertTrue(FileManager.default.fileExists(atPath: busy))
        task.cancel()
        try Data().write(to: folder.appendingPathComponent("cancelled"))
        do {
            _ = try await task.value
            XCTFail("Startup cancellation must propagate after terminal status")
        } catch is CancellationError { }
        XCTAssertTrue(FileManager.default.fileExists(atPath: folder.appendingPathComponent("terminal").path))
    }
    #endif
}
