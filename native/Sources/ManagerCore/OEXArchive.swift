// SPDX-License-Identifier: Apache-2.0
import Foundation
import CryptoKit
import Darwin

/// Owns only a freshly created private directory; its lifetime follows the archive.
final class OEXTemporaryFile: @unchecked Sendable {
    let directory: URL
    let url: URL
    let handle: FileHandle
    init() throws {
        directory = FileManager.default.temporaryDirectory.appendingPathComponent("exhibitos-oex-" + UUID().uuidString, isDirectory: true)
        var created = false
        do {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
            created = true
            url = directory.appendingPathComponent("archive")
            let fd = Darwin.open(url.path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600)
            guard fd >= 0 else { throw AuthoringFailure("FILE_READ_FAILED") }
            handle = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        } catch {
            if created { try? FileManager.default.removeItem(at: directory) }
            throw error
        }
    }
    deinit { try? handle.close(); try? FileManager.default.removeItem(at: directory) }
}

final class OEXArchiveWriter {
    private let file: OEXTemporaryFile
    private var hash = SHA256()
    private var prefix = Data()
    private(set) var count = 0
    init() throws { file = try OEXTemporaryFile() }
    func append(_ block: Data) throws {
        try Task.checkCancellation()
        guard block.count <= NativeOEXArchive.maximumBytes - count else { throw AuthoringFailure("OEX_LIMIT") }
        if prefix.count < 4 { prefix.append(block.prefix(4 - prefix.count)) }
        try file.handle.write(contentsOf: block)
        hash.update(data: block); count += block.count
    }
    func finish() throws -> NativeOEXArchive {
        guard count >= 22, prefix == Data([0x50,0x4b,0x03,0x04]) else { throw AuthoringFailure("OEX_RESPONSE_INVALID") }
        try file.handle.close()
        return NativeOEXArchive(file: file, count: count, sha256: hash.finalize().map { String(format: "%02x", $0) }.joined())
    }
}

/// A stable, bounded private snapshot of a selected local OEX file.
/// The source path is never persisted or sent to the server.
public struct NativeOEXImportFile: Sendable {
    public let name: String
    public let archive: NativeOEXArchive
    public static func load(_ url: URL) async throws -> Self {
        guard url.isFileURL, url.pathExtension.lowercased() == "oex" else { throw AuthoringFailure("OEX_FILE_REQUIRED") }
        let work = Task.detached(priority: .userInitiated) {
            let access = url.startAccessingSecurityScopedResource()
            defer { if access { url.stopAccessingSecurityScopedResource() } }
            let fd = Darwin.open(url.path, O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK)
            guard fd >= 0 else { throw AuthoringFailure("FILE_READ_FAILED") }
            let handle = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
            defer { try? handle.close() }
            var before = stat()
            guard fstat(fd, &before) == 0, before.st_mode & S_IFMT == S_IFREG else { throw AuthoringFailure("FILE_REGULAR_REQUIRED") }
            guard before.st_size > 0, before.st_size <= NativeOEXArchive.maximumBytes else { throw AuthoringFailure("OEX_LIMIT") }
            let writer = try OEXArchiveWriter()
            while let block = try handle.read(upToCount: 256 * 1024), !block.isEmpty { try writer.append(block) }
            var after = stat()
            guard fstat(fd, &after) == 0, after.st_size == before.st_size, writer.count == before.st_size,
                  after.st_mtimespec.tv_sec == before.st_mtimespec.tv_sec, after.st_mtimespec.tv_nsec == before.st_mtimespec.tv_nsec,
                  after.st_ctimespec.tv_sec == before.st_ctimespec.tv_sec, after.st_ctimespec.tv_nsec == before.st_ctimespec.tv_nsec else { throw AuthoringFailure("FILE_CHANGED") }
            return Self(name: url.lastPathComponent, archive: try writer.finish())
        }
        return try await withTaskCancellationHandler {
            let result = try await work.value; try Task.checkCancellation(); return result
        } onCancel: { work.cancel() }
    }
}

/// Transport admission only; Platform validates archive structure, rights and asset bytes.
public struct NativeOEXArchive: Sendable {
    public static let maximumBytes = 64 * 1024 * 1024
    public static let mime = "application/vnd.exhibitos.oex+zip"
    private let file: OEXTemporaryFile
    public let count: Int
    public let sha256: String
    var fileURL: URL { file.url }
    fileprivate init(file: OEXTemporaryFile, count: Int, sha256: String) { self.file = file; self.count = count; self.sha256 = sha256 }
    public init(data: Data) throws {
        let writer = try OEXArchiveWriter(); try writer.append(data); self = try writer.finish()
    }
    public func write(to url: URL) async throws {
        guard url.isFileURL else { throw AuthoringFailure("EXPORT_WRITE_FAILED") }
        let ownedFile = file
        let work = Task.detached(priority: .userInitiated) {
            let temporary = url.deletingLastPathComponent().appendingPathComponent(".exhibitos-oex-" + UUID().uuidString)
            let fd = Darwin.open(temporary.path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600)
            guard fd >= 0 else { throw AuthoringFailure("EXPORT_WRITE_FAILED") }
            let output = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
            defer { try? output.close(); try? FileManager.default.removeItem(at: temporary) }
            do {
                let input = try FileHandle(forReadingFrom: ownedFile.url); defer { try? input.close() }
                while let block = try input.read(upToCount: 256 * 1024), !block.isEmpty { try Task.checkCancellation(); try output.write(contentsOf: block) }
                try output.close(); try Task.checkCancellation()
                guard Darwin.rename(temporary.path, url.path) == 0 else { throw AuthoringFailure("EXPORT_WRITE_FAILED") }
            } catch is CancellationError { throw CancellationError() }
            catch { throw AuthoringFailure("EXPORT_WRITE_FAILED") }
        }
        try await withTaskCancellationHandler { try await work.value } onCancel: { work.cancel() }
    }
}
