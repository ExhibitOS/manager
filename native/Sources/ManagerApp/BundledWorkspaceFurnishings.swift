// SPDX-License-Identifier: Apache-2.0
import CryptoKit
import Darwin
import Foundation
import ManagerCore

/// Reads only the sealed 1.2.0 distribution inventory. Selection does not register anything.
enum BundledWorkspaceFurnishings {
    struct Item: Decodable, Identifiable, Sendable {
        struct File: Decodable, Sendable { let path: String, bytes: Int, sha256: String }
        struct Model: Decodable, Sendable { let path: String, mime: String, bytes: Int, sha256: String }
        struct Rights: Decodable, Sendable { let licenseId: String, notice: File }
        struct Provenance: Decodable, Sendable { let sourceSha256: String, recipe: String }
        let catalogId: String, catalogVersion: Int, name: String, hostKind: String
        let model: Model, rights: Rights, provenance: Provenance
        var id: String { catalogId }
        var kind: WorkspaceFurnishingKind? { WorkspaceFurnishingKind(rawValue: hostKind) }
    }
    private struct Manifest: Decodable { let schema: String, bundleVersion: String, assets: [Item] }
    private static let allowed: [String: WorkspaceFurnishingKind] = [
        "freestanding-info-stand": .infoStand, "gallery-stool": .stool, "upholstered-gallery-bench": .bench
    ]
    private static func digest(_ data: Data) -> String { SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined() }
    private static func bounded(_ url: URL, expected: Int) throws -> Data {
        let fd = Darwin.open(url.path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)
        guard fd >= 0 else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
        let handle = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        defer { try? handle.close() }
        var before = stat(), after = stat()
        guard fstat(fd, &before) == 0, before.st_mode & S_IFMT == S_IFREG,
              before.st_size == expected, expected > 0, expected <= 7079,
              let data = try handle.read(upToCount: expected + 1), data.count == expected,
              fstat(fd, &after) == 0, after.st_size == before.st_size,
              after.st_mtimespec.tv_sec == before.st_mtimespec.tv_sec,
              after.st_mtimespec.tv_nsec == before.st_mtimespec.tv_nsec,
              after.st_ctimespec.tv_sec == before.st_ctimespec.tv_sec,
              after.st_ctimespec.tv_nsec == before.st_ctimespec.tv_nsec else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
        return data
    }
    private static func path(_ path: String, root: URL) throws -> URL {
        guard !path.contains(".."), !path.contains("\\"),
              path.hasPrefix("assets/furnishings/user-authored-20261027/") else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
        let url = root.appendingPathComponent(path)
        guard url.resolvingSymlinksInPath() == url, url.path.hasPrefix(root.path + "/") else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
        return url
    }
    static func catalog(root override: URL? = nil) throws -> (URL, [Item]) {
        guard let location = override ?? Bundle.main.resourceURL?.appendingPathComponent("native-runtime/platform", isDirectory: true), location.isFileURL else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
        // Bundle.resourceURL may have a base URL; compare canonical absolute file URLs.
        let root = URL(fileURLWithPath: location.path, isDirectory: true)
        let manifestURL = root.appendingPathComponent("assets/furnishings/default-manifest.json")
        guard root.resolvingSymlinksInPath() == root, manifestURL.resolvingSymlinksInPath() == manifestURL,
              manifestURL.path.hasPrefix(root.path + "/") else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
        let data = try bounded(manifestURL, expected: 7079)
        guard data.count == 7079, digest(data) == "db9427c19ecaaceb3029c49bd4700955fa9a0b2cb568b422d5238f589e44f4e1" else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
        let manifest = try JSONDecoder().decode(Manifest.self, from: data)
        let items = manifest.assets.filter { allowed[$0.catalogId] != nil }
        guard manifest.schema == "org.exhibitos.default-furnishings/v1", manifest.bundleVersion == "1.2.0",
              items.count == 3, Set(items.map(\.id)) == Set(allowed.keys), items.allSatisfy({
                  allowed[$0.catalogId] == $0.kind && $0.catalogVersion == 1 && $0.model.mime == "model/gltf-binary" &&
                  $0.rights.licenseId == "CC0-1.0" && $0.provenance.recipe == "original-v1" && $0.provenance.sourceSha256 == $0.model.sha256
              }) else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
        return (root, items)
    }
    static func load(_ item: Item, root: URL) async throws -> ImportFileSnapshot {
        let prefix = "assets/furnishings/user-authored-20261027/\(item.id)/"
        guard item.model.path == prefix + "model.glb", item.rights.notice.path == prefix + "LICENSE-NOTICE.txt" else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
        let notice = try path(item.rights.notice.path, root: root)
        let noticeData = try await Task.detached { try bounded(notice, expected: item.rights.notice.bytes) }.value
        guard digest(noticeData) == item.rights.notice.sha256 else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
        let url = try path(item.model.path, root: root)
        let snapshot = try await ImportFileSnapshot.load(url, byteLimit: WorkspaceFurnishingModel.byteLimit)
        guard snapshot.sha256 == item.model.sha256, snapshot.data.count == item.model.bytes else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
        return snapshot
    }
}
