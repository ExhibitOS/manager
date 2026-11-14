// SPDX-License-Identifier: Apache-2.0
import Foundation
import ManagerCore
import XCTest
@testable import ManagerApp

final class PassiveCeilingRailCatalogTests: XCTestCase {
    func testExact113RailDiscoveryAndSourceNoticeIntegrity() async throws {
        guard let input = ProcessInfo.processInfo.environment["EXHIBITOS_DEFAULT_113_RUNTIME_PLATFORM"] else {
            throw XCTSkip("Set EXHIBITOS_DEFAULT_113_RUNTIME_PLATFORM to the qualified runtime platform directory")
        }
        let source = URL(fileURLWithPath: input, isDirectory: true)
        let catalog = try BundledWorkspaceFurnishings.catalog(root: source)
        XCTAssertEqual(catalog.version, "1.13.0")
        XCTAssertEqual(catalog.items.count, 10)
        let rail = try XCTUnwrap(catalog.items.first { $0.id == "linear-track" })
        XCTAssertEqual(rail.kind, .ceilingRail)
        let model = try await BundledWorkspaceFurnishings.load(rail.id, from: catalog)
        XCTAssertEqual(model.data.count, 187_688)
        XCTAssertEqual(model.sha256, "1ba64842de525166018e941074852a3e041d07adf605674c7d7fadb02693a09e")
        XCTAssertFalse(catalog.items.contains { $0.id == "thin-brass-frame" || $0.id == "black-aluminum-frame" })
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        defer { try? FileManager.default.removeItem(at: root) }
        for relative in ["assets/furnishings/default-manifest.json", rail.model.path, rail.rights.notice.path] {
            let destination = root.appendingPathComponent(relative)
            try FileManager.default.createDirectory(at: destination.deletingLastPathComponent(), withIntermediateDirectories: true)
            try FileManager.default.copyItem(at: source.appendingPathComponent(relative), to: destination)
        }
        let copied = try BundledWorkspaceFurnishings.catalog(root: root)
        let notice = root.appendingPathComponent(rail.rights.notice.path)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: notice.path)
        var bytes = try Data(contentsOf: notice); bytes[0] ^= 1; try bytes.write(to: notice)
        do { _ = try await BundledWorkspaceFurnishings.load(rail.id, from: copied); XCTFail("Changed notice accepted") }
        catch let error as AuthoringFailure { XCTAssertEqual(error.code, "FURNISHING_CATALOG_INVALID") }
    }
}
