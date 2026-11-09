// SPDX-License-Identifier: Apache-2.0
import Foundation
import Darwin
import XCTest
@testable import ManagerCore

final class SharedBenchAutomationInputTests: XCTestCase {
    func testReplyRequiresPrivateRegularFileInsideOutputRoot() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: root) }
        let good = root.appendingPathComponent("reply.json")
        try Data("{\"version\":1}".utf8).write(to: good)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: good.path)
        XCTAssertEqual(try NativeAutomationSession.sharedBenchReply(root: root, path: good.path)["version"], .integer(1))
        let link = root.appendingPathComponent("link")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: good)
        XCTAssertThrowsError(try NativeAutomationSession.sharedBenchReply(root: root, path: link.path))
        let fifo = root.appendingPathComponent("fifo")
        XCTAssertEqual(mkfifo(fifo.path, 0o600), 0)
        XCTAssertThrowsError(try NativeAutomationSession.sharedBenchReply(root: root, path: fifo.path))
        let big = root.appendingPathComponent("large")
        try Data(repeating: 0, count: 1_100_001).write(to: big)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: big.path)
        XCTAssertThrowsError(try NativeAutomationSession.sharedBenchReply(root: root, path: big.path))
        XCTAssertThrowsError(try NativeAutomationSession.sharedBenchReply(root: root, path: root.deletingLastPathComponent().appendingPathComponent("outside").path))
    }
}
