// SPDX-License-Identifier: Apache-2.0
import Foundation
import CryptoKit
import Darwin

/// A stable, bounded read-only snapshot of a selected local OEX file.
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
            var data = Data(); data.reserveCapacity(Int(before.st_size))
            while true {
                try Task.checkCancellation()
                guard let block = try handle.read(upToCount: min(256 * 1024, NativeOEXArchive.maximumBytes + 1 - data.count)), !block.isEmpty else { break }
                data.append(block)
                guard data.count <= NativeOEXArchive.maximumBytes else { throw AuthoringFailure("OEX_LIMIT") }
            }
            var after = stat()
            guard fstat(fd, &after) == 0, after.st_size == before.st_size, data.count == before.st_size,
                  after.st_mtimespec.tv_sec == before.st_mtimespec.tv_sec, after.st_mtimespec.tv_nsec == before.st_mtimespec.tv_nsec,
                  after.st_ctimespec.tv_sec == before.st_ctimespec.tv_sec, after.st_ctimespec.tv_nsec == before.st_ctimespec.tv_nsec else { throw AuthoringFailure("FILE_CHANGED") }
            return Self(name: url.lastPathComponent, archive: try NativeOEXArchive(data: data))
        }
        return try await withTaskCancellationHandler {
            let result = try await work.value; try Task.checkCancellation(); return result
        } onCancel: { work.cancel() }
    }
}

/// Transport admission only. The existing Platform OEX service validates the
/// complete archive profile, current rights and every approved asset byte.
public struct NativeOEXArchive: Sendable {
    public static let maximumBytes = 64 * 1024 * 1024
    public static let mime = "application/vnd.exhibitos.oex+zip"
    public let data: Data
    public let sha256: String
    public init(data: Data) throws {
        guard data.count <= Self.maximumBytes else { throw AuthoringFailure("OEX_LIMIT") }
        guard data.count >= 22, data.prefix(4) == Data([0x50,0x4b,0x03,0x04]) else { throw AuthoringFailure("OEX_RESPONSE_INVALID") }
        self.data = data
        sha256 = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }
    public func write(to url: URL) async throws {
        guard url.isFileURL else { throw AuthoringFailure("EXPORT_WRITE_FAILED") }
        let bytes = data
        try await Task.detached(priority: .userInitiated) {
            do { try bytes.write(to: url, options: .atomic) }
            catch { throw AuthoringFailure("EXPORT_WRITE_FAILED") }
        }.value
    }
}
