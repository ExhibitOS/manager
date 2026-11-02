// SPDX-License-Identifier: Apache-2.0
import Foundation
import Darwin

extension SceneSnapshotResult {
    /// Same private-stage/exclusive-rename pattern as the bounded OEX writer.
    /// A failed write leaves no final PNG and never replaces another file.
    public func writeNewPNG(to url: URL) async throws {
        guard url.isFileURL, !["", ".", ".."].contains(url.lastPathComponent), png.count <= SceneSnapshotViewportRequest.maximumPNGBytes else { throw AuthoringFailure("SCENE_SNAPSHOT_WRITE_FAILED") }
        let bytes = png
        let work = Task.detached(priority: .userInitiated) {
            try Task.checkCancellation()
            let parent = Darwin.open(url.deletingLastPathComponent().path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            guard parent >= 0 else { throw AuthoringFailure("SCENE_SNAPSHOT_WRITE_FAILED") }
            defer { Darwin.close(parent) }
            let name = ".exhibitos-scene-" + UUID().uuidString
            guard mkdirat(parent, name, 0o700) == 0 else { throw AuthoringFailure("SCENE_SNAPSHOT_WRITE_FAILED") }
            let directory = openat(parent, name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            guard directory >= 0 else { throw AuthoringFailure("SCENE_SNAPSHOT_WRITE_FAILED") }
            defer { Darwin.close(directory) }
            var identity = stat()
            guard fstat(directory, &identity) == 0, identity.st_mode & S_IFMT == S_IFDIR,
                  identity.st_mode & 0o777 == 0o700, identity.st_uid == geteuid() else { throw AuthoringFailure("SCENE_SNAPSHOT_WRITE_FAILED") }
            defer {
                var named = stat()
                if fstatat(parent, name, &named, AT_SYMLINK_NOFOLLOW) == 0, named.st_dev == identity.st_dev, named.st_ino == identity.st_ino {
                    _ = unlinkat(parent, name, AT_REMOVEDIR)
                }
            }
            let fd = openat(directory, "image", O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600)
            guard fd >= 0 else { throw AuthoringFailure("SCENE_SNAPSHOT_WRITE_FAILED") }
            let output = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
            var outputIdentity = stat()
            guard fstat(fd, &outputIdentity) == 0, outputIdentity.st_mode & S_IFMT == S_IFREG,
                  outputIdentity.st_uid == geteuid(), outputIdentity.st_nlink == 1 else { throw AuthoringFailure("SCENE_SNAPSHOT_WRITE_FAILED") }
            defer {
                var named = stat()
                if fstatat(directory, "image", &named, AT_SYMLINK_NOFOLLOW) == 0,
                   named.st_dev == outputIdentity.st_dev, named.st_ino == outputIdentity.st_ino { _ = unlinkat(directory, "image", 0) }
                try? output.close()
            }
            try Task.checkCancellation()
            try output.write(contentsOf: bytes)
            var complete = stat(), namedFile = stat(), namedDirectory = stat()
            guard fstat(fd, &complete) == 0, complete.st_size == Int64(bytes.count), fsync(fd) == 0,
                  fstatat(directory, "image", &namedFile, AT_SYMLINK_NOFOLLOW) == 0,
                  namedFile.st_dev == outputIdentity.st_dev, namedFile.st_ino == outputIdentity.st_ino,
                  fstatat(parent, name, &namedDirectory, AT_SYMLINK_NOFOLLOW) == 0,
                  namedDirectory.st_dev == identity.st_dev, namedDirectory.st_ino == identity.st_ino else { throw AuthoringFailure("SCENE_SNAPSHOT_WRITE_FAILED") }
            try Task.checkCancellation()
            // Once committed, report success even if cancellation arrives after
            // the kernel operation. Never replace an existing/racing destination.
            guard renameatx_np(directory, "image", parent, url.lastPathComponent, UInt32(RENAME_EXCL)) == 0 else { throw AuthoringFailure("SCENE_SNAPSHOT_WRITE_FAILED") }
        }
        try await withTaskCancellationHandler { try await work.value } onCancel: { work.cancel() }
    }
}
