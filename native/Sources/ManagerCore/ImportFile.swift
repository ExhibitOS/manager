// SPDX-License-Identifier: Apache-2.0
import Foundation
import Darwin
import CryptoKit

public struct NativeUploadState: Codable, Sendable {
    public let format: Int
    public var revision: UInt64
    public let workspaceId: String
    public let tenantId: String
    public let principalId: String
    public var intent: NativeImportIntent?
    public var jobId: String?
    public var fileName: String?
}
/// A bounded immutable copy. The selected source is opened read-only without
/// following a final symlink; paths/bookmarks and file bytes never enter IPC.
public struct ImportFileSnapshot: Sendable {
    public static let limit = 32 * 1024 * 1024
    public let name: String
    public let mime: String
    public let sha256: String
    public let data: Data

    public static func load(_ url: URL) async throws -> Self {
        let worker = Task.detached(priority: .userInitiated) { try read(url) }
        return try await withTaskCancellationHandler { try await worker.value } onCancel: { worker.cancel() }
    }
    private static func read(_ url: URL) throws -> Self {
        guard url.isFileURL else { throw AuthoringFailure("FILE_LOCAL_REQUIRED") }
        let mime: String
        switch url.pathExtension.lowercased() {
        case "glb": mime = "model/gltf-binary"
        case "png": mime = "image/png"
        default: throw AuthoringFailure("FILE_FORMAT_UNSUPPORTED")
        }
        let access = url.startAccessingSecurityScopedResource()
        defer { if access { url.stopAccessingSecurityScopedResource() } }
        let fd = Darwin.open(url.path, O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK)
        guard fd >= 0 else { throw AuthoringFailure("FILE_READ_FAILED") }
        let handle = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        defer { try? handle.close() }
        var before = stat()
        guard fstat(fd, &before) == 0, before.st_mode & S_IFMT == S_IFREG else { throw AuthoringFailure("FILE_REGULAR_REQUIRED") }
        guard before.st_size > 0, before.st_size <= limit else { throw AuthoringFailure("FILE_SIZE_INVALID") }
        var data = Data(); data.reserveCapacity(Int(before.st_size))
        while true {
            try Task.checkCancellation()
            guard let block = try handle.read(upToCount: min(256 * 1024, limit + 1 - data.count)), !block.isEmpty else { break }
            data.append(block)
            guard data.count <= limit else { throw AuthoringFailure("FILE_SIZE_INVALID") }
        }
        var after = stat()
        guard fstat(fd, &after) == 0, after.st_size == before.st_size, data.count == before.st_size,
              after.st_mtimespec.tv_sec == before.st_mtimespec.tv_sec, after.st_mtimespec.tv_nsec == before.st_mtimespec.tv_nsec,
              after.st_ctimespec.tv_sec == before.st_ctimespec.tv_sec, after.st_ctimespec.tv_nsec == before.st_ctimespec.tv_nsec else { throw AuthoringFailure("FILE_CHANGED") }
        return Self(name: url.lastPathComponent, mime: mime, sha256: SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined(), data: data)
    }
    public func matches(_ intent: NativeImportIntent) -> Bool {
        intent.bytes == data.count && intent.mime == mime && intent.sha256 == sha256
    }
}
