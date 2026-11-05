// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

@MainActor
final class NativeBridgeTests: XCTestCase {
    func testRejectsMissingHelperBeforeCreatingProfile() throws {
        let profile = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        XCTAssertThrowsError(try NativeBridge(helper: URL(fileURLWithPath: "/missing/exhibitos-native"), profile: profile))
        XCTAssertFalse(FileManager.default.fileExists(atPath: profile.path))
    }
    #if DEBUG
    func testWorkspaceV5ValidationPassesTheRuntimeToSelectedAndReopenHelpers() async throws {
        let root = FileManager.default.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let helper = root.appendingPathComponent("helper")
        let runtime = root.appendingPathComponent("runtime")
        let profile = root.appendingPathComponent("profile")
        try """
        #!/bin/sh
        [ "$1" = --profile ] || exit 2
        case "$3" in
          --fresh-workspace-ports)
            [ "$#" = 6 ] && [ "$5" = --selected-oex-v5-validation ] && [ "$6" = "${0%/*}/runtime" ];;
          --reopen-workspace-v5-validation)
            [ "$#" = 4 ] && [ "$4" = "${0%/*}/runtime" ];;
          *) exit 2;;
        esac
        """.write(to: helper, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: helper.path)
        let ports: [String: UInt16] = ["web": 19101, "api": 19102, "database": 19103]
        let selected = try NativeBridge(helper: helper, profile: profile, freshSelectedOEX: true, freshWorkspacePorts: ports,
                                        workspaceV5ValidationRuntime: runtime)
        try await selected.close()
        let reopened = try NativeBridge(helper: helper, profile: profile, workspaceV5ValidationRuntime: runtime)
        try await reopened.close()
        XCTAssertThrowsError(try NativeBridge(helper: helper, profile: profile, freshWorkspacePorts: ports,
                                              workspaceV5ValidationRuntime: runtime))
    }
    #endif
    func testActualRustStateRoundtripAndSharedSelection() async throws {
        guard let helper = ProcessInfo.processInfo.environment["EXHIBITOS_NATIVE_TEST_HELPER"] else {
            throw XCTSkip("Set the built Rust helper for the native pipe integration test")
        }
        let parent = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: parent, withIntermediateDirectories: true)
        let canonical = parent.resolvingSymlinksInPath()
        let profile = canonical.appendingPathComponent("profile")
        let bridge = try NativeBridge(helper: URL(fileURLWithPath: helper), profile: profile)
        let original = try await bridge.presentation()
        XCTAssertEqual(original.mode, .beginner)
        let context = try await bridge.context()
        let originalJobs = try await bridge.jobs(context)
        XCTAssertTrue(originalJobs.isEmpty)
        var next = original; next.mode = .expert; next.step = .preview; next.textScale = 200
        let saved = try await bridge.save(next)
        let restored = try await bridge.presentation()
        XCTAssertEqual(restored, saved)
        let sameContext = try await bridge.context()
        XCTAssertEqual(context.activeId, sameContext.activeId)
        let status = try await bridge.status(context)
        XCTAssertFalse(status.installed)
        let native = try await bridge.nativeStatus(context)
        XCTAssertFalse(native.busy)
        XCTAssertFalse(native.accountRequired)
        XCTAssertNil(native.configuration)
        XCTAssertNil(native.runtime)
        XCTAssertFalse(native.needsShutdown)
        do { _ = try await bridge.save(original); XCTFail("Stale settings must fail") }
        catch let error as BridgeFailure { XCTAssertEqual(error.code, "NATIVE_UI_STATE_CHANGED") }
        // Only this synthetic directory is removed; the session ends by pipe EOF.
        try FileManager.default.removeItem(at: parent)
    }
}
