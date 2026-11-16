// SPDX-License-Identifier: Apache-2.0
import Foundation
import ManagerCore
import XCTest
@testable import ManagerApp

final class RectangleTrackCatalogTests: XCTestCase {
    func testExact118KeepsLightVisualsOutOfWorkspaceCards() throws {
        guard let input = ProcessInfo.processInfo.environment["EXHIBITOS_DEFAULT_118_RUNTIME_PLATFORM"] else {
            throw XCTSkip("Set EXHIBITOS_DEFAULT_118_RUNTIME_PLATFORM to the qualified runtime platform directory")
        }
        let source = URL(fileURLWithPath: input, isDirectory: true)
        let catalog = try BundledWorkspaceFurnishings.catalog(root: source)
        XCTAssertEqual(catalog.version, "1.18.0")
        XCTAssertEqual(Set(catalog.items.map(\.id)), Set([
            "freestanding-info-stand", "gallery-stool", "upholstered-gallery-bench",
            "closed-exit-door", "exit-pictogram-sign", "sculpture-gallery-bench",
            "portal-pale-mineral", "portal-warm-oak", "compact-caption-stand",
            "linear-track", "hayes-clear-protective-hood", "sculpture-bench-refined-v6",
            "dark-track-network", "painting-rectangular-dark-track",
        ]))
        XCTAssertEqual(catalog.items.count, 14)
        let rail = try XCTUnwrap(catalog.items.first { $0.id == "painting-rectangular-dark-track" })
        XCTAssertEqual(rail.kind, .ceilingRail)
        XCTAssertEqual(rail.model.sha256, "aca126489bf21adcc637e9a3c3bb805bb57f93111c3333a8fdefab9d371d1c42")

        let files = FileManager.default
        let root = files.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent(UUID().uuidString, isDirectory: true)
        defer { try? files.removeItem(at: root) }
        let relative = "assets/furnishings/default-manifest.json"
        let path = root.appendingPathComponent(relative)
        try files.createDirectory(at: path.deletingLastPathComponent(), withIntermediateDirectories: true)
        var changed = try Data(contentsOf: source.appendingPathComponent(relative))
        XCTAssertEqual(changed.count, 25_957)
        changed[changed.startIndex] ^= 1
        try changed.write(to: path)
        XCTAssertThrowsError(try BundledWorkspaceFurnishings.catalog(root: root))
    }

    func testExact117RectangleDiscoveryAndSourceNoticeIntegrity() async throws {
        guard let input = ProcessInfo.processInfo.environment["EXHIBITOS_DEFAULT_117_RUNTIME_PLATFORM"] else {
            throw XCTSkip("Set EXHIBITOS_DEFAULT_117_RUNTIME_PLATFORM to the qualified runtime platform directory")
        }
        let source = URL(fileURLWithPath: input, isDirectory: true)
        let catalog = try BundledWorkspaceFurnishings.catalog(root: source)
        XCTAssertEqual(catalog.version, "1.17.0")
        XCTAssertEqual(catalog.items.count, 14)
        let track = try XCTUnwrap(catalog.items.first { $0.id == "painting-rectangular-dark-track" })
        XCTAssertEqual(track.kind, .ceilingRail)
        XCTAssertEqual(track.catalogVersion, 1)
        XCTAssertEqual(track.provenance.recipe, "original-v1")
        XCTAssertEqual(track.model.bytes, 268820)
        XCTAssertEqual(track.model.sha256, "aca126489bf21adcc637e9a3c3bb805bb57f93111c3333a8fdefab9d371d1c42")
        XCTAssertEqual(track.rights.notice.sha256, "45b428e505df8eef13d0f6db84004fe0f87cc00571d69bc6a55e2089ea7b4bf0")
        let oldTrack = try XCTUnwrap(catalog.items.first { $0.id == "dark-track-network" })
        XCTAssertEqual(oldTrack.kind, .ceilingRail)
        XCTAssertEqual(oldTrack.model.sha256, "5cc5d772acd4087c5d7d26b5f24e2fe38626b1315e377210410954abf7d4ff44")
        let white = try XCTUnwrap(catalog.items.first { $0.id == "linear-track" })
        XCTAssertEqual(white.kind, .ceilingRail)
        XCTAssertEqual(white.model.sha256, "1ba64842de525166018e941074852a3e041d07adf605674c7d7fadb02693a09e")
        XCTAssertFalse(catalog.items.contains { $0.id == "museum-track-spot-fixed-64p1" })

        // Keep the immutable qualified distribution untouched; only this model,
        // its notice and the exact manifest are needed for isolated load tests.
        let files = FileManager.default
        let root = files.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        defer { try? files.removeItem(at: root) }
        let manifestPath = "assets/furnishings/default-manifest.json"
        for relative in [manifestPath, track.model.path, track.rights.notice.path] {
            let destination = root.appendingPathComponent(relative)
            try files.createDirectory(at: destination.deletingLastPathComponent(), withIntermediateDirectories: true)
            try files.copyItem(at: source.appendingPathComponent(relative), to: destination)
            try files.setAttributes([.posixPermissions: 0o600], ofItemAtPath: destination.path)
        }
        let copied = try BundledWorkspaceFurnishings.catalog(root: root)
        let model = try await BundledWorkspaceFurnishings.load(track.id, from: copied)
        XCTAssertEqual(model.data.count, 268820)
        XCTAssertEqual(model.sha256, track.model.sha256)
        for relative in [track.rights.notice.path, track.model.path] {
            let url = root.appendingPathComponent(relative), original = try Data(contentsOf: url)
            var altered = original; altered[altered.startIndex] ^= 1
            try altered.write(to: url)
            do { _ = try await BundledWorkspaceFurnishings.load(track.id, from: copied); XCTFail("Changed model or notice accepted") }
            catch let error as AuthoringFailure { XCTAssertEqual(error.code, "FURNISHING_CATALOG_INVALID") }
            try original.write(to: url)
        }
        let manifest = root.appendingPathComponent(manifestPath)
        var altered = try Data(contentsOf: manifest); altered[altered.startIndex] ^= 1
        try altered.write(to: manifest)
        XCTAssertThrowsError(try BundledWorkspaceFurnishings.catalog(root: root))
    }
}
