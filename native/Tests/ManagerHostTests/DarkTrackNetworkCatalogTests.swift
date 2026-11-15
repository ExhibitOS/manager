// SPDX-License-Identifier: Apache-2.0
import Foundation
import ManagerCore
import XCTest
@testable import ManagerApp

final class DarkTrackNetworkCatalogTests: XCTestCase {
    func testExact116SourceAndNoticeIntegrityRetainsWhiteRail() async throws {
        guard let input = ProcessInfo.processInfo.environment["EXHIBITOS_DEFAULT_116_FIXTURE"] else {
            throw XCTSkip("Set EXHIBITOS_DEFAULT_116_FIXTURE to the reviewed source checkout")
        }
        let source = URL(fileURLWithPath: input), files = FileManager.default
        let root = files.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        defer { try? files.removeItem(at: root) }
        let prefix = "assets/furnishings/exhibitos-authored-20261115/dark-track-network/"
        try files.createDirectory(at: root.appendingPathComponent(prefix), withIntermediateDirectories: true)
        let manifest = root.appendingPathComponent("assets/furnishings/default-manifest.json")
        try files.copyItem(at: source.appendingPathComponent("manifest.json"), to: manifest)
        for name in ["model.glb", "LICENSE-NOTICE.txt"] {
            try files.copyItem(at: source.appendingPathComponent(prefix + name), to: root.appendingPathComponent(prefix + name))
        }
        let catalog = try BundledWorkspaceFurnishings.catalog(root: root)
        XCTAssertEqual(catalog.version, "1.16.0"); XCTAssertEqual(catalog.items.count, 13)
        let track = try XCTUnwrap(catalog.items.first { $0.id == "dark-track-network" })
        XCTAssertEqual(track.kind, .ceilingRail); XCTAssertEqual(track.provenance.recipe, "original-v1")
        XCTAssertEqual(track.catalogVersion, 1)
        let white = try XCTUnwrap(catalog.items.first { $0.id == "linear-track" })
        XCTAssertEqual(white.kind, .ceilingRail)
        XCTAssertEqual(white.model.sha256, "1ba64842de525166018e941074852a3e041d07adf605674c7d7fadb02693a09e")
        let model = try await BundledWorkspaceFurnishings.load(track.id, from: catalog)
        XCTAssertEqual(model.data.count, 377576)
        XCTAssertEqual(model.sha256, "5cc5d772acd4087c5d7d26b5f24e2fe38626b1315e377210410954abf7d4ff44")
        for name in ["model.glb", "LICENSE-NOTICE.txt"] {
            let url = root.appendingPathComponent(prefix + name), original = try Data(contentsOf: url)
            var altered = original; altered[altered.startIndex] ^= 1; try altered.write(to: url)
            do { _ = try await BundledWorkspaceFurnishings.load(track.id, from: catalog); XCTFail("Changed source or notice accepted") }
            catch { }
            try original.write(to: url)
        }
        var altered = try Data(contentsOf: manifest); altered[altered.startIndex] ^= 1; try altered.write(to: manifest)
        XCTAssertThrowsError(try BundledWorkspaceFurnishings.catalog(root: root))
    }
}
