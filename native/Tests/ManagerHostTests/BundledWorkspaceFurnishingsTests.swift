// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
@testable import ManagerApp

final class BundledWorkspaceFurnishingsTests: XCTestCase {
    func testPinnedCatalogAndOriginalsRejectChanges() async throws {
        guard let input = ProcessInfo.processInfo.environment["EXHIBITOS_DEFAULT_FURNISHINGS_FIXTURE"] else {
            throw XCTSkip("Set EXHIBITOS_DEFAULT_FURNISHINGS_FIXTURE to the pinned 1.2.0 source checkout")
        }
        let source = URL(fileURLWithPath: input, isDirectory: true)
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        let files = FileManager.default
        try files.createDirectory(at: root.appendingPathComponent("assets/furnishings"), withIntermediateDirectories: true)
        defer { try? files.removeItem(at: root) }
        try files.copyItem(at: source.appendingPathComponent("manifest.json"), to: root.appendingPathComponent("assets/furnishings/default-manifest.json"))
        for id in ["freestanding-info-stand", "gallery-stool", "upholstered-gallery-bench"] {
            let path = "assets/furnishings/user-authored-20261027/\(id)"
            try files.createDirectory(at: root.appendingPathComponent(path), withIntermediateDirectories: true)
            for name in ["model.glb", "LICENSE-NOTICE.txt"] {
                try files.copyItem(at: source.appendingPathComponent(path).appendingPathComponent(name), to: root.appendingPathComponent(path).appendingPathComponent(name))
            }
        }
        let (catalogRoot, items) = try BundledWorkspaceFurnishings.catalog(root: root)
        XCTAssertEqual(Set(items.map(\.id)), Set(["freestanding-info-stand", "gallery-stool", "upholstered-gallery-bench"]))
        let relativeRoot = try XCTUnwrap(URL(string: root.lastPathComponent + "/", relativeTo: root.deletingLastPathComponent()))
        XCTAssertNotNil(relativeRoot.baseURL)
        let (resolvedRoot, relativeItems) = try BundledWorkspaceFurnishings.catalog(root: relativeRoot)
        XCTAssertEqual(resolvedRoot.path, root.path)
        XCTAssertEqual(Set(relativeItems.map(\.id)), Set(items.map(\.id)))
        for item in items {
            let snapshot = try await BundledWorkspaceFurnishings.load(item, root: catalogRoot)
            XCTAssertEqual(snapshot.sha256, item.model.sha256)
        }
        let item = try XCTUnwrap(items.first)
        try Data("changed".utf8).write(to: root.appendingPathComponent(item.rights.notice.path))
        do { _ = try await BundledWorkspaceFurnishings.load(item, root: root); XCTFail("Altered notice accepted") } catch { }
        try Data("changed".utf8).write(to: root.appendingPathComponent("assets/furnishings/default-manifest.json"))
        XCTAssertThrowsError(try BundledWorkspaceFurnishings.catalog(root: root))
    }
}
