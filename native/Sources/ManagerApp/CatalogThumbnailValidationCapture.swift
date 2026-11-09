// SPDX-License-Identifier: Apache-2.0
#if DEBUG
import Darwin
import Foundation
import ManagerCore

/// A one-shot private evidence destination beside the signed validation profile.
struct CatalogThumbnailValidationCapture {
    struct Intent: Decodable {
        let artworkId: String
        let revision: Int
        let sourceAssetId: String
        let sourceSha256: String
        let displaySha256: String
        let manifestSHA256: String
    }

    private struct Receipt: Encodable {
        let artworkId: String
        let revision: Int
        let sourceAssetId: String
        let sourceSha256: String
        let displaySha256: String
        let manifestSHA256: String
        let pngSha256: String
        let pngBytes: Int
        let width = 256
        let height = 256
        let selectionVersion = 0
        let view = "default"
    }

    let intent: Intent
    private let destination: URL

    struct Reservation {
        fileprivate let parent: FileHandle
        fileprivate let directory: FileHandle
        fileprivate let name: String

        fileprivate var isCurrent: Bool {
            var opened = stat(), named = stat()
            return fstat(directory.fileDescriptor, &opened) == 0
                && fstatat(parent.fileDescriptor, name, &named, AT_SYMLINK_NOFOLLOW) == 0
                && (named.st_mode & mode_t(S_IFMT)) == mode_t(S_IFDIR)
                && opened.st_dev == named.st_dev && opened.st_ino == named.st_ino
        }
    }

    static var requested: Bool {
        guard let root = evidenceRoot else { return false }
        var info = stat()
        return lstat(root.appendingPathComponent("catalog-thumbnail-capture-intent.json").path, &info) == 0
    }

    static func load() -> Self? {
        guard let root = evidenceRoot,
              let pins = Bundle.main.object(forInfoDictionaryKey: "ExhibitOSValidationInputs") as? [String: String] else { return nil }
        let file = root.appendingPathComponent("catalog-thumbnail-capture-intent.json")
        let descriptor = open(file.path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK)
        guard descriptor >= 0 else { return nil }
        let input = FileHandle(fileDescriptor: descriptor, closeOnDealloc: true)
        defer { try? input.close() }
        var info = stat()
        guard fstat(descriptor, &info) == 0, (info.st_mode & mode_t(S_IFMT)) == mode_t(S_IFREG), info.st_nlink == 1,
              info.st_uid == geteuid(), (info.st_mode & 0o777) == 0o600, (1...4096).contains(info.st_size),
              let data = try? input.read(upToCount: 4097), data.count == Int(info.st_size),
              let intent = try? JSONDecoder().decode(Intent.self, from: data),
              UUID(uuidString: intent.artworkId)?.uuidString.lowercased() == intent.artworkId,
              UUID(uuidString: intent.sourceAssetId)?.uuidString.lowercased() == intent.sourceAssetId,
              (1...Int(Int32.max)).contains(intent.revision),
              [intent.sourceSha256, intent.displaySha256, intent.manifestSHA256].allSatisfy({
                  $0.range(of: #"^[a-f0-9]{64}$"#, options: .regularExpression) != nil
              }), pins["runtimeManifest"] == intent.manifestSHA256 else { return nil }
        return Self(intent: intent, destination: root.appendingPathComponent("catalog-thumbnail-capture", isDirectory: true))
    }

    private static var evidenceRoot: URL? {
        guard Bundle.main.bundleIdentifier == "org.exhibitos.manager.validation",
              let profile = Bundle.main.object(forInfoDictionaryKey: "ExhibitOSValidationProfile") as? String,
              profile.hasPrefix("/") else { return nil }
        let root = URL(fileURLWithPath: profile, isDirectory: true).deletingLastPathComponent()
        var info = stat()
        guard root.resolvingSymlinksInPath().path == root.path,
              lstat(root.path, &info) == 0, (info.st_mode & mode_t(S_IFMT)) == mode_t(S_IFDIR), info.st_uid == geteuid(),
              (info.st_mode & 0o777) == 0o700 else { return nil }
        return root
    }

    var canWrite: Bool {
        var info = stat()
        return lstat(destination.path, &info) != 0 && errno == ENOENT
    }

    func reserve() throws -> Reservation {
        let parentPath = destination.deletingLastPathComponent().path
        let parentFD = open(parentPath, O_RDONLY | O_DIRECTORY | O_NOFOLLOW)
        guard parentFD >= 0 else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        let parent = FileHandle(fileDescriptor: parentFD, closeOnDealloc: true)
        let name = destination.lastPathComponent
        var info = stat()
        guard fstat(parentFD, &info) == 0, info.st_uid == geteuid(), (info.st_mode & 0o777) == 0o700,
              mkdirat(parentFD, name, 0o700) == 0 else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        let directoryFD = openat(parentFD, name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW)
        guard directoryFD >= 0 else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        let directory = FileHandle(fileDescriptor: directoryFD, closeOnDealloc: true)
        let reservation = Reservation(parent: parent, directory: directory, name: name)
        guard fstat(directoryFD, &info) == 0, info.st_uid == geteuid(), (info.st_mode & 0o777) == 0o700,
              reservation.isCurrent else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        return reservation
    }

    func write(_ rendered: ViewportRenderedThumbnail, to reservation: Reservation) throws {
        guard rendered.artworkId == intent.artworkId, rendered.revision == intent.revision,
              rendered.sourceAssetId == intent.sourceAssetId, rendered.sourceSha256 == intent.sourceSha256,
              rendered.displaySha256 == intent.displaySha256, (24...512 * 1024).contains(rendered.pngData.count) else {
            throw AuthoringFailure("CATALOG_PREVIEW_INVALID")
        }
        let receipt = Receipt(artworkId: rendered.artworkId, revision: rendered.revision,
            sourceAssetId: rendered.sourceAssetId, sourceSha256: rendered.sourceSha256, displaySha256: rendered.displaySha256,
            manifestSHA256: intent.manifestSHA256, pngSha256: rendered.pngSha256, pngBytes: rendered.pngData.count)
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        let metadata = try encoder.encode(receipt)
        guard reservation.isCurrent else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        try writePrivate(rendered.pngData, name: "preview.png", in: reservation)
        try writePrivate(metadata, name: "receipt.json", in: reservation)
        guard reservation.isCurrent else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
    }

    private func writePrivate(_ data: Data, name: String, in reservation: Reservation) throws {
        let descriptor = openat(reservation.directory.fileDescriptor, name, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, 0o600)
        guard descriptor >= 0 else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        let output = FileHandle(fileDescriptor: descriptor, closeOnDealloc: true)
        try output.write(contentsOf: data)
        try output.synchronize()
        try output.close()
    }
}
#endif
