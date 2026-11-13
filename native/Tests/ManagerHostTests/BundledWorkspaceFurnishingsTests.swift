// SPDX-License-Identifier: Apache-2.0
import Foundation
import ManagerCore
import XCTest
@testable import ManagerApp

final class BundledWorkspaceFurnishingsTests: XCTestCase {
    private func catalog111Fixture() throws -> (source: URL, root: URL) {
        guard let input = ProcessInfo.processInfo.environment["EXHIBITOS_DEFAULT_111_FIXTURE"] else {
            throw XCTSkip("Set EXHIBITOS_DEFAULT_111_FIXTURE to the exact default 1.11.0 source checkout")
        }
        let source = URL(fileURLWithPath: input, isDirectory: true)
        let files = FileManager.default
        let root = files.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        let directory = "assets/furnishings/exhibitos-authored-20261109/portal-pale-mineral"
        try files.createDirectory(at: root.appendingPathComponent(directory), withIntermediateDirectories: true)
        do {
            try files.copyItem(at: source.appendingPathComponent("manifest.json"),
                               to: root.appendingPathComponent("assets/furnishings/default-manifest.json"))
            for name in ["model.glb", "LICENSE-NOTICE.txt"] {
                let relative = directory + "/" + name
                try files.copyItem(at: source.appendingPathComponent(relative), to: root.appendingPathComponent(relative))
            }
        } catch {
            try? files.removeItem(at: root)
            throw error
        }
        return (source, root)
    }

    func testExactCatalog111LoadsExistingPortal() async throws {
        let fixture = try catalog111Fixture()
        defer { try? FileManager.default.removeItem(at: fixture.root) }
        let catalog = try BundledWorkspaceFurnishings.catalog(root: fixture.root)
        XCTAssertEqual(catalog.version, "1.11.0")
        XCTAssertEqual(catalog.items.count, 9)
        let portal = try XCTUnwrap(catalog.items.first { $0.id == "portal-pale-mineral" })
        XCTAssertEqual(portal.kind, .portalFinish)
        let model = try await BundledWorkspaceFurnishings.load(portal.id, from: catalog)
        XCTAssertEqual(model.data.count, 2_468)
        XCTAssertEqual(model.sha256, "62fd3bc49e0e77ffde26f60dee7ddfbbac0939347d68ea53a94700de0695f1ae")
        do {
            _ = try await BundledWorkspaceFurnishings.load("unknown-id", from: catalog)
            XCTFail("Unknown item accepted")
        } catch let error as AuthoringFailure { XCTAssertEqual(error.code, "FURNISHING_CATALOG_INVALID") }
    }

    func testCatalog111RejectsChangedAndMissingFiles() async throws {
        let fixture = try catalog111Fixture(), files = FileManager.default
        defer { try? files.removeItem(at: fixture.root) }
        let catalog = try BundledWorkspaceFurnishings.catalog(root: fixture.root)
        let portal = try XCTUnwrap(catalog.items.first { $0.id == "portal-pale-mineral" })
        for relative in [portal.model.path, portal.rights.notice.path] {
            let target = fixture.root.appendingPathComponent(relative)
            var changed = try Data(contentsOf: target)
            changed[changed.startIndex] ^= 1
            try changed.write(to: target)
            do {
                _ = try await BundledWorkspaceFurnishings.load(portal.id, from: catalog)
                XCTFail("Changed file accepted: \(relative)")
            } catch { }
            try files.removeItem(at: target)
            do {
                _ = try await BundledWorkspaceFurnishings.load(portal.id, from: catalog)
                XCTFail("Missing file accepted: \(relative)")
            } catch { }
            try files.copyItem(at: fixture.source.appendingPathComponent(relative), to: target)
        }
        let manifest = fixture.root.appendingPathComponent("assets/furnishings/default-manifest.json")
        var bytes = try Data(contentsOf: manifest)
        bytes[bytes.startIndex] ^= 1
        try bytes.write(to: manifest)
        XCTAssertThrowsError(try BundledWorkspaceFurnishings.catalog(root: fixture.root))
        bytes.append(0)
        try bytes.write(to: manifest)
        XCTAssertThrowsError(try BundledWorkspaceFurnishings.catalog(root: fixture.root))
    }

    private func catalog17Root() throws -> URL {
        guard let input = ProcessInfo.processInfo.environment["EXHIBITOS_DEFAULT_17_PLATFORM_FIXTURE"] else {
            throw XCTSkip("Set EXHIBITOS_DEFAULT_17_PLATFORM_FIXTURE to the exact default1.7 assembly platform root")
        }
        return URL(fileURLWithPath: input, isDirectory: true)
    }

    func testCompactCaptionStandFromExactCatalog17() async throws {
        let catalog = try BundledWorkspaceFurnishings.catalog(root: catalog17Root())
        XCTAssertEqual(catalog.version, "1.7.0")
        XCTAssertEqual(Set(catalog.items.map(\.id)), Set(["freestanding-info-stand", "gallery-stool", "upholstered-gallery-bench",
            "closed-exit-door", "exit-pictogram-sign", "sculpture-gallery-bench", "portal-pale-mineral", "portal-warm-oak", "compact-caption-stand"]))
        XCTAssertEqual(catalog.items.first { $0.id == "exit-pictogram-sign" }?.catalogVersion, 2)
        let item = try XCTUnwrap(catalog.items.first { $0.id == "compact-caption-stand" })
        XCTAssertEqual(item.kind, .infoStand)
        XCTAssertEqual(item.provenance.recipe, "original-v1")
        XCTAssertNil(item.provenance.source)
        XCTAssertEqual(item.rights.notice.bytes, 580)
        XCTAssertEqual(item.rights.notice.sha256, "5d90fcaaec647de18cf2be4b7bf46927b8136080c3cd7f37c3ee1efe3336b87f")
        let model = try await BundledWorkspaceFurnishings.load(item.id, from: catalog)
        XCTAssertEqual(model.data.count, 13_864)
        XCTAssertEqual(model.sha256, "ec9e424fe07eceeb4b422c0ed3dfa6981ac9f5dc335bd6ed32954d2c1a190752")
    }

    func testCatalog17ModelAndNoticeStayBoundToSnapshot() async throws {
        let source = try catalog17Root(), files = FileManager.default
        let root = files.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        let directory = "assets/furnishings/exhibitos-authored-20261111/compact-caption-stand"
        try files.createDirectory(at: root.appendingPathComponent(directory), withIntermediateDirectories: true)
        defer { try? files.removeItem(at: root) }
        let manifest = "assets/furnishings/default-manifest.json"
        try files.copyItem(at: source.appendingPathComponent(manifest), to: root.appendingPathComponent(manifest))
        for name in ["model.glb", "LICENSE-NOTICE.txt"] {
            let relative = directory + "/" + name
            try files.copyItem(at: source.appendingPathComponent(relative), to: root.appendingPathComponent(relative))
        }
        let catalog = try BundledWorkspaceFurnishings.catalog(root: root)
        _ = try await BundledWorkspaceFurnishings.load("compact-caption-stand", from: catalog)
        let model = root.appendingPathComponent(directory + "/model.glb")
        var bytes = try Data(contentsOf: model); bytes[bytes.count - 1] ^= 1
        try bytes.write(to: model)
        do { _ = try await BundledWorkspaceFurnishings.load("compact-caption-stand", from: catalog); XCTFail("Changed model accepted") } catch { }
        try files.removeItem(at: model)
        try files.copyItem(at: source.appendingPathComponent(directory + "/model.glb"), to: model)
        try Data(count: 580).write(to: root.appendingPathComponent(directory + "/LICENSE-NOTICE.txt"))
        do { _ = try await BundledWorkspaceFurnishings.load("compact-caption-stand", from: catalog); XCTFail("Changed notice accepted") } catch { }
        var changed = try Data(contentsOf: root.appendingPathComponent(manifest)); changed[changed.startIndex] = 32
        try changed.write(to: root.appendingPathComponent(manifest))
        XCTAssertThrowsError(try BundledWorkspaceFurnishings.catalog(root: root))
    }

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

    func testRectangularPortalsFromExactCatalog16PreserveEarlierLibrary() async throws {
        guard let input = ProcessInfo.processInfo.environment["EXHIBITOS_RECTANGULAR_PORTAL_CATALOG_FIXTURE"] else {
            throw XCTSkip("Set EXHIBITOS_RECTANGULAR_PORTAL_CATALOG_FIXTURE to the pinned 1.6.0 source checkout")
        }
        let source = URL(fileURLWithPath: input, isDirectory: true), files = FileManager.default
        let root = files.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try files.createDirectory(at: root.appendingPathComponent("assets/furnishings"), withIntermediateDirectories: true)
        defer { try? files.removeItem(at: root) }
        try files.copyItem(at: source.appendingPathComponent("manifest.json"),
                           to: root.appendingPathComponent("assets/furnishings/default-manifest.json"))
        let portals = ["portal-pale-mineral", "portal-warm-oak"]
        for id in portals {
            let directory = "assets/furnishings/exhibitos-authored-20261109/" + id
            try files.createDirectory(at: root.appendingPathComponent(directory), withIntermediateDirectories: true)
            for name in ["model.glb", "LICENSE-NOTICE.txt"] {
                let relative = directory + "/" + name
                try files.copyItem(at: source.appendingPathComponent(relative), to: root.appendingPathComponent(relative))
            }
        }
        let catalog = try BundledWorkspaceFurnishings.catalog(root: root)
        XCTAssertEqual(catalog.version, "1.6.0")
        XCTAssertEqual(Set(catalog.items.map(\.id)), Set(portals + ["freestanding-info-stand", "gallery-stool",
            "upholstered-gallery-bench", "closed-exit-door", "exit-pictogram-sign", "sculpture-gallery-bench"]))
        XCTAssertEqual(catalog.items.first { $0.id == "exit-pictogram-sign" }?.catalogVersion, 2)
        for id in portals {
            let item = try XCTUnwrap(catalog.items.first { $0.id == id })
            XCTAssertEqual(item.hostKind, "opening"); XCTAssertEqual(item.kind, .portalFinish)
            let original = try await BundledWorkspaceFurnishings.load(id, from: catalog)
            XCTAssertEqual(original.sha256, item.provenance.sourceSha256)
            XCTAssertEqual(original.data.count, item.model.bytes)
        }
        let item = try XCTUnwrap(catalog.items.first { $0.id == portals[0] })
        try Data(count: item.rights.notice.bytes).write(to: root.appendingPathComponent(item.rights.notice.path))
        do { _ = try await BundledWorkspaceFurnishings.load(item.id, from: catalog); XCTFail("Changed portal notice accepted") }
        catch let failure as AuthoringFailure { XCTAssertEqual(failure.code, "FURNISHING_CATALOG_INVALID") }
        var altered = try Data(contentsOf: root.appendingPathComponent("assets/furnishings/default-manifest.json"))
        altered[altered.startIndex] = 32
        try altered.write(to: root.appendingPathComponent("assets/furnishings/default-manifest.json"))
        XCTAssertThrowsError(try BundledWorkspaceFurnishings.catalog(root: root))
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
