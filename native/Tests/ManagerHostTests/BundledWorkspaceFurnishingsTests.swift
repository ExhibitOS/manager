// SPDX-License-Identifier: Apache-2.0
import Foundation
import ManagerCore
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
        let catalog = try BundledWorkspaceFurnishings.catalog(root: root)
        XCTAssertEqual(Set(catalog.items.map(\.id)), Set(["freestanding-info-stand", "gallery-stool", "upholstered-gallery-bench"]))
        let relativeRoot = try XCTUnwrap(URL(string: root.lastPathComponent + "/", relativeTo: root.deletingLastPathComponent()))
        XCTAssertNotNil(relativeRoot.baseURL)
        let relativeCatalog = try BundledWorkspaceFurnishings.catalog(root: relativeRoot)
        XCTAssertEqual(relativeCatalog.root.path, root.path)
        XCTAssertEqual(Set(relativeCatalog.items.map(\.id)), Set(catalog.items.map(\.id)))
        for item in catalog.items {
            let snapshot = try await BundledWorkspaceFurnishings.load(item.id, from: catalog)
            XCTAssertEqual(snapshot.sha256, item.model.sha256)
        }
        let item = try XCTUnwrap(catalog.items.first)
        try Data("changed".utf8).write(to: root.appendingPathComponent(item.rights.notice.path))
        do { _ = try await BundledWorkspaceFurnishings.load(item.id, from: catalog); XCTFail("Altered notice accepted") } catch { }
        try Data("changed".utf8).write(to: root.appendingPathComponent("assets/furnishings/default-manifest.json"))
        XCTAssertThrowsError(try BundledWorkspaceFurnishings.catalog(root: root))
    }

    func testPreparedSignAndIndependentDoorFromExactPackagedCatalog() async throws {
        guard let input = ProcessInfo.processInfo.environment["EXHIBITOS_PREPARED_SIGN_PACKAGE"] else {
            throw XCTSkip("Set EXHIBITOS_PREPARED_SIGN_PACKAGE to the pinned 1.4.0 platform resource root")
        }
        let root = URL(fileURLWithPath: input, isDirectory: true)
        let catalog = try BundledWorkspaceFurnishings.catalog(root: root)
        XCTAssertEqual(catalog.version, "1.4.0")
        XCTAssertEqual(catalog.items.count, 5)
        let sign = try XCTUnwrap(catalog.items.first { $0.id == "exit-pictogram-sign" })
        let door = try XCTUnwrap(catalog.items.first { $0.id == "closed-exit-door" })
        XCTAssertEqual(sign.kind, .infoStand)
        XCTAssertEqual(door.kind, .barrier)
        let render = try await BundledWorkspaceFurnishings.load(sign.id, from: catalog)
        XCTAssertEqual(render.sha256, "d8cf449146e1cc8b9174d1a550a44bc41735487ec1fe68a9deb363faba02e44c")
        XCTAssertNotEqual(render.sha256, sign.provenance.sourceSha256)
        let doorSnapshot = try await BundledWorkspaceFurnishings.load(door.id, from: catalog)
        XCTAssertEqual(doorSnapshot.sha256, "e4f9a701214e980dacf127a009847e4fd0c46ca98f53e880e03ce2b9d4c55e25")
        XCTAssertTrue(catalog.provenance(for: sign).contains("normalize-legacy-sign-texture-r1"))
        XCTAssertTrue(catalog.provenance(for: sign).contains(sign.provenance.sourceSha256))
    }

    func testSculptureBenchCanBeSelectedFromExactCatalog15() async throws {
        guard let input = ProcessInfo.processInfo.environment["EXHIBITOS_SCULPTURE_BENCH_CATALOG_FIXTURE"] else {
            throw XCTSkip("Set EXHIBITOS_SCULPTURE_BENCH_CATALOG_FIXTURE to the pinned 1.5.0 source checkout")
        }
        let source = URL(fileURLWithPath: input, isDirectory: true)
        let files = FileManager.default
        let root = files.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        let benchDirectory = "assets/furnishings/generated-20261110/sculpture-gallery-bench"
        try files.createDirectory(at: root.appendingPathComponent(benchDirectory), withIntermediateDirectories: true)
        defer { try? files.removeItem(at: root) }
        try files.copyItem(at: source.appendingPathComponent("manifest.json"),
                           to: root.appendingPathComponent("assets/furnishings/default-manifest.json"))
        for name in ["model.glb", "LICENSE-NOTICE.txt"] {
            let relative = benchDirectory + "/" + name
            try files.copyItem(at: source.appendingPathComponent(relative), to: root.appendingPathComponent(relative))
        }
        let catalog = try BundledWorkspaceFurnishings.catalog(root: root)
        XCTAssertEqual(catalog.version, "1.5.0")
        XCTAssertEqual(catalog.items.count, 6)
        let bench = try XCTUnwrap(catalog.items.first { $0.id == "sculpture-gallery-bench" })
        XCTAssertEqual(bench.kind, .bench)
        let snapshot = try await BundledWorkspaceFurnishings.load(bench.id, from: catalog)
        XCTAssertEqual(snapshot.sha256, "6cb111d9baf50d355c18b8e94b9cff78eeeb9679f02ebf490d7093e2c32c59d0")
        XCTAssertEqual(snapshot.data.count, 85_876)
        let sign = try XCTUnwrap(catalog.items.first { $0.id == "exit-pictogram-sign" })
        XCTAssertEqual(sign.catalogVersion, 2)
        XCTAssertTrue(catalog.provenance(for: sign).contains("normalize-legacy-sign-texture-r1"))
        try Data(count: bench.rights.notice.bytes).write(to: root.appendingPathComponent(bench.rights.notice.path))
        do {
            _ = try await BundledWorkspaceFurnishings.load(bench.id, from: catalog)
            XCTFail("Altered notice accepted")
        } catch let error as AuthoringFailure {
            XCTAssertEqual(error.code, "FURNISHING_CATALOG_INVALID")
        }
    }
}
