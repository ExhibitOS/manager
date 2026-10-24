// SPDX-License-Identifier: Apache-2.0
import XCTest
@testable import ManagerCore

@MainActor
final class NativeBridgeCancellationTests: XCTestCase {
    private func fixture() throws -> (URL, URL, NativeBridge) {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString).resolvingSymlinksInPath()
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let helper = root.appendingPathComponent("helper"), profile = root.appendingPathComponent("profile")
        try FileManager.default.createDirectory(at: profile, withIntermediateDirectories: true)
        // Deterministic test-only reply gate. No product delay or alternate
        // process is introduced into the app's production bridge.
        let script = #"""
        #!/bin/sh
        count=0
        while IFS= read -r frame; do
          count=$((count+1))
          if [ "$count" = 1 ]; then
            /usr/bin/touch "$2/entered"
            while [ ! -f "$2/release" ]; do /bin/sleep 0.01; done
          fi
          /usr/bin/printf '{"version":1,"id":%s,"result":{"format":1,"revision":0,"mode":"beginner","step":"prepare","textScale":100}}\n' "$count"
        done
        """#
        try Data(script.utf8).write(to: helper)
        try FileManager.default.setAttributes([.posixPermissions:0o700], ofItemAtPath: helper.path)
        return (root, profile, try NativeBridge(helper: helper, profile: profile))
    }
    func testViewCancellationAfterSendDrainsReplyAndKeepsSharedHelperUsable() async throws {
        let (root, profile, bridge) = try fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let reply = Task { try await bridge.presentation() }
        let end = ContinuousClock.now.advanced(by: .seconds(5))
        while !FileManager.default.fileExists(atPath: profile.appendingPathComponent("entered").path), ContinuousClock.now < end { try await Task.sleep(for: .milliseconds(10)) }
        XCTAssertTrue(FileManager.default.fileExists(atPath: profile.appendingPathComponent("entered").path))
        reply.cancel()
        // Hold the gate across the old bridge's 100ms poll interval to reproduce
        // its helper-termination bug; this is fault injection only.
        try await Task.sleep(for: .milliseconds(200))
        try Data().write(to: profile.appendingPathComponent("release"))
        let first = try await reply.value
        XCTAssertEqual(first.step, .prepare)
        let next = try await bridge.presentation()
        XCTAssertEqual(next.step, .prepare) // ID2 must receive ID2, not a stale reply.
        try await bridge.close()
    }
    func testAlreadyCancelledViewDoesNotSendAFrameOrTerminateHelper() async throws {
        let (root, profile, bridge) = try fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let cancelled = Task {
            withUnsafeCurrentTask { $0?.cancel() }
            return try await bridge.presentation()
        }
        do { _ = try await cancelled.value; XCTFail("Cancelled unsent request must be refused") }
        catch is CancellationError { }
        XCTAssertFalse(FileManager.default.fileExists(atPath: profile.appendingPathComponent("entered").path))
        try Data().write(to: profile.appendingPathComponent("release"))
        let first = try await bridge.presentation()
        XCTAssertEqual(first.step, .prepare) // No skipped request ID or dead helper.
        try await bridge.close()
    }
}
