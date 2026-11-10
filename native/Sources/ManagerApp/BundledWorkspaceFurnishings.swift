// SPDX-License-Identifier: Apache-2.0
import CryptoKit
import Darwin
import Foundation
import ManagerCore

/// Reads only the exact supported distribution inventories. Selection stages a local file.
enum BundledWorkspaceFurnishings {
    struct Item: Decodable, Identifiable, Sendable {
        struct File: Decodable, Sendable { let path: String, bytes: Int, sha256: String }
        struct Model: Decodable, Sendable { let path: String, mime: String, bytes: Int, sha256: String }
        struct Rights: Decodable, Sendable { let licenseId: String, notice: File }
        struct Provenance: Decodable, Sendable {
            let sourceSha256: String, recipe: String
            let source: File?
            let runtimeManifestSHA256: String?
            let registeredReceiptSHA256: String?
        }
        let catalogId: String, catalogVersion: Int, name: String, hostKind: String
        let model: Model, rights: Rights, provenance: Provenance
        var id: String { catalogId }
        var kind: WorkspaceFurnishingKind? { WorkspaceFurnishingKind(rawValue: hostKind) }
    }
    struct Catalog: Sendable {
        let root: URL
        let version: String
        let items: [Item]

        func provenance(for item: Item) -> String {
            let catalog = "ExhibitOS default furnishings \(version), \(item.catalogId), \(item.provenance.recipe)."
            guard let producer = item.provenance.runtimeManifestSHA256,
                  let receipt = item.provenance.registeredReceiptSHA256 else { return catalog }
            return "\(catalog) Original SHA-256 \(item.provenance.sourceSha256); prepared runtime \(producer); registration receipt \(receipt)."
        }
    }
    private struct Manifest: Decodable { let schema: String, bundleVersion: String, assets: [Item] }
    private static let allowed: [String: WorkspaceFurnishingKind] = [
        "freestanding-info-stand": .infoStand, "gallery-stool": .stool, "upholstered-gallery-bench": .bench
    ]
    private static let generated: [String: WorkspaceFurnishingKind] = [
        "closed-exit-door": .barrier, "exit-pictogram-sign": .infoStand
    ]
    private static let manifests = [
        7079: (version: "1.2.0", digest: "db9427c19ecaaceb3029c49bd4700955fa9a0b2cb568b422d5238f589e44f4e1"),
        9246: (version: "1.4.0", digest: "7073f8f6dfd7a1c44e8e2b7206b503a35fa1a59eb992d08191dde5f6c6e86e6e"),
    ]
    private static func digest(_ data: Data) -> String { SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined() }
    private static func bounded(_ url: URL, sizes: Set<Int>) throws -> Data {
        let fd = Darwin.open(url.path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)
        guard fd >= 0 else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
        let handle = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        defer { try? handle.close() }
        var before = stat(), after = stat()
        guard fstat(fd, &before) == 0, before.st_mode & S_IFMT == S_IFREG,
              let size = Int(exactly: before.st_size), sizes.contains(size), (1...9246).contains(size),
              let data = try handle.read(upToCount: size + 1), data.count == size,
              fstat(fd, &after) == 0, after.st_size == before.st_size,
              after.st_mtimespec.tv_sec == before.st_mtimespec.tv_sec,
              after.st_mtimespec.tv_nsec == before.st_mtimespec.tv_nsec,
              after.st_ctimespec.tv_sec == before.st_ctimespec.tv_sec,
              after.st_ctimespec.tv_nsec == before.st_ctimespec.tv_nsec else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
        return data
    }
    private static func path(_ path: String, root: URL) throws -> URL {
        guard !path.contains(".."), !path.contains("\\") else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
        let url = root.appendingPathComponent(path)
        guard url.resolvingSymlinksInPath() == url, url.path.hasPrefix(root.path + "/") else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
        return url
    }
    private static func directory(for id: String) -> String {
        let source = generated[id] == nil ? "user-authored-20261027" : "generated-20261109"
        return "assets/furnishings/\(source)/\(id)/"
    }

    static func catalog(root override: URL? = nil) throws -> Catalog {
        guard let location = override ?? Bundle.main.resourceURL?.appendingPathComponent("native-runtime/platform", isDirectory: true), location.isFileURL else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
        // Bundle.resourceURL may have a base URL; compare canonical absolute file URLs.
        let root = URL(fileURLWithPath: location.path, isDirectory: true)
        let manifestURL = root.appendingPathComponent("assets/furnishings/default-manifest.json")
        guard root.resolvingSymlinksInPath() == root, manifestURL.resolvingSymlinksInPath() == manifestURL,
              manifestURL.path.hasPrefix(root.path + "/") else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
        let data = try bounded(manifestURL, sizes: Set(manifests.keys))
        guard let pin = manifests[data.count], digest(data) == pin.digest else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
        let manifest = try JSONDecoder().decode(Manifest.self, from: data)
        let supported = pin.version == "1.4.0" ? allowed.merging(generated) { first, _ in first } : allowed
        let items = manifest.assets.filter { supported[$0.id] != nil }
        guard manifest.schema == "org.exhibitos.default-furnishings/v1", manifest.bundleVersion == pin.version,
              items.count == supported.count, Set(items.map(\.id)) == Set(supported.keys),
              items.allSatisfy({ supported[$0.id] == $0.kind && valid($0) }) else {
            throw AuthoringFailure("FURNISHING_CATALOG_INVALID")
        }
        return Catalog(root: root, version: pin.version, items: items)
    }

    private static func valid(_ item: Item) -> Bool {
        let prefix = directory(for: item.id)
        guard item.model.path == prefix + "model.glb", item.rights.notice.path == prefix + "LICENSE-NOTICE.txt",
              item.model.mime == "model/gltf-binary", item.rights.licenseId == "CC0-1.0" else { return false }
        if item.id == "exit-pictogram-sign" {
            let lineage = item.provenance
            return item.catalogVersion == 2 && lineage.recipe == "normalize-legacy-sign-texture-r1"
                && lineage.source?.path == prefix + "source.glb" && lineage.source?.sha256 == lineage.sourceSha256
                && lineage.sourceSha256 != item.model.sha256
                && lineage.runtimeManifestSHA256 != nil && lineage.registeredReceiptSHA256 != nil
        }
        return item.catalogVersion == 1 && item.provenance.recipe == "original-v1" && item.provenance.source == nil
            && item.provenance.sourceSha256 == item.model.sha256
    }

    static func load(_ id: String, from catalog: Catalog) async throws -> ImportFileSnapshot {
        let kind = allowed[id] ?? (catalog.version == "1.4.0" ? generated[id] : nil)
        guard let kind, let item = catalog.items.first(where: { $0.id == id }), item.kind == kind, valid(item) else {
            throw AuthoringFailure("FURNISHING_CATALOG_INVALID")
        }
        let notice = try path(item.rights.notice.path, root: catalog.root)
        let noticeData = try await Task.detached { try bounded(notice, sizes: [item.rights.notice.bytes]) }.value
        guard digest(noticeData) == item.rights.notice.sha256 else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
        if let source = item.provenance.source {
            let original = try await ImportFileSnapshot.load(path(source.path, root: catalog.root), byteLimit: WorkspaceFurnishingModel.byteLimit)
            guard original.data.count == source.bytes, original.sha256 == source.sha256 else {
                throw AuthoringFailure("FURNISHING_CATALOG_INVALID")
            }
        }
        let url = try path(item.model.path, root: catalog.root)
        let snapshot = try await ImportFileSnapshot.load(url, byteLimit: WorkspaceFurnishingModel.byteLimit)
        guard snapshot.sha256 == item.model.sha256, snapshot.data.count == item.model.bytes else { throw AuthoringFailure("FURNISHING_CATALOG_INVALID") }
        return snapshot
    }
}
