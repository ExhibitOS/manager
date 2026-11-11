// SPDX-License-Identifier: Apache-2.0
import Foundation
import Darwin

/// A reviewable product result, not permission to select it. Selection still
/// requires the current authenticated owner, artwork revision and server CAS.
struct NativeThumbnailCandidate: Codable, Sendable {
    let workspaceId: String
    let tenantId: String
    let principalId: String
    let selectionVersion: Int
    let request: ViewportThumbnailRequest
    let reply: ViewportThumbnailReply

    private static func directory(_ root: URL) throws -> Int32 {
        let fd = open(root.path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        guard fd >= 0 else { throw BridgeFailure("AUTOMATION_OUTPUT_INVALID") }
        var info = stat(), named = stat()
        guard fstat(fd, &info) == 0, lstat(root.path, &named) == 0,
              info.st_uid == getuid(), info.st_mode & 0o777 == 0o700,
              info.st_dev == named.st_dev, info.st_ino == named.st_ino else {
            close(fd); throw BridgeFailure("AUTOMATION_OUTPUT_INVALID")
        }
        return fd
    }

    func persist(root: URL, name: String) throws {
        guard !name.contains("/"), name.hasPrefix("thumbnail-"), name.hasSuffix(".json") else { throw BridgeFailure("AUTOMATION_OUTPUT_INVALID") }
        let parent = try Self.directory(root); defer { close(parent) }
        let temporary = "." + name + "." + UUID().uuidString + ".tmp"
        let fd = openat(parent, temporary, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600)
        guard fd >= 0 else { throw BridgeFailure("AUTOMATION_OUTPUT_INVALID") }
        let file = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        defer { try? file.close(); unlinkat(parent, temporary, 0) }
        let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys]
        try file.write(contentsOf: encoder.encode(self)); try file.synchronize()
        // Publish a complete candidate atomically, without replacing any file.
        guard renameatx_np(parent, temporary, parent, name, UInt32(RENAME_EXCL)) == 0,
              fsync(parent) == 0 else { throw BridgeFailure("AUTOMATION_OUTPUT_INVALID") }
    }

    static func materializePreview(_ bytes: Data, root: URL, name: String) throws {
        guard !name.contains("/"), name.hasPrefix("thumbnail-"), name.hasSuffix(".png") else { throw BridgeFailure("AUTOMATION_OUTPUT_INVALID") }
        let parent = try directory(root); defer { close(parent) }
        let fd = openat(parent, name, O_RDWR | O_CREAT | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC, 0o600)
        guard fd >= 0 else { throw BridgeFailure("AUTOMATION_OUTPUT_INVALID") }
        let file = FileHandle(fileDescriptor: fd, closeOnDealloc: true); defer { try? file.close() }
        var info = stat()
        guard fstat(fd, &info) == 0, info.st_mode & S_IFMT == S_IFREG, info.st_nlink == 1,
              info.st_uid == getuid(), info.st_mode & 0o777 == 0o600,
              info.st_size >= 0, info.st_size <= bytes.count else {
            throw BridgeFailure("AUTOMATION_OUTPUT_INVALID")
        }
        let read = try file.read(upToCount: bytes.count + 1)
        let prefix = read ?? Data()
        guard (read != nil || info.st_size == 0), prefix.count == Int(info.st_size),
              prefix == bytes.prefix(prefix.count) else {
            throw BridgeFailure("AUTOMATION_OUTPUT_INVALID")
        }
        // Only a verified prefix of this exact candidate is resumed. Foreign
        // or modified files are never replaced, truncated or removed.
        try file.seek(toOffset: UInt64(prefix.count))
        try file.write(contentsOf: bytes.dropFirst(prefix.count)); try file.synchronize()
        var named = stat()
        guard fstatat(parent, name, &named, AT_SYMLINK_NOFOLLOW) == 0,
              info.st_dev == named.st_dev, info.st_ino == named.st_ino,
              named.st_size == bytes.count, fsync(parent) == 0 else { throw BridgeFailure("AUTOMATION_OUTPUT_INVALID") }
    }

    func rendered(in state: NativeDocumentState, expectedSHA256: String? = nil) throws -> ViewportRenderedThumbnail {
        guard workspaceId == state.workspaceId, tenantId == state.tenantId, principalId == state.principalId,
              request.version == 1,
              request.tenantId == tenantId, request.principalId == principalId,
              (0..<Int(Int32.max)).contains(selectionVersion) else { throw BridgeFailure("AUTOMATION_SCOPE_CHANGED") }
        let rendered = try reply.renderedThumbnail(matching: request)
        guard expectedSHA256 == nil || rendered.pngSha256 == expectedSHA256 else { throw BridgeFailure("AUTOMATION_INPUT_INVALID") }
        return rendered
    }
}
