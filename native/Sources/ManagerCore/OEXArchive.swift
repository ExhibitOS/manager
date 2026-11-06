// SPDX-License-Identifier: Apache-2.0
import Foundation
import CryptoKit
import Darwin

fileprivate struct OEXSnapshotStamp: Sendable {
    let device: dev_t, inode: ino_t, size: off_t
    let modifiedSeconds: Int64, modifiedNanoseconds: Int64, changedSeconds: Int64, changedNanoseconds: Int64
    init(_ value: stat) {
        device = value.st_dev; inode = value.st_ino; size = value.st_size
        modifiedSeconds = Int64(value.st_mtimespec.tv_sec); modifiedNanoseconds = Int64(value.st_mtimespec.tv_nsec)
        changedSeconds = Int64(value.st_ctimespec.tv_sec); changedNanoseconds = Int64(value.st_ctimespec.tv_nsec)
    }
    func matches(_ value: stat) -> Bool {
        value.st_mode & S_IFMT == S_IFREG && value.st_mode & 0o777 == 0o600
            && value.st_uid == geteuid() && value.st_nlink == 1 && value.st_dev == device && value.st_ino == inode && value.st_size == size
            && Int64(value.st_mtimespec.tv_sec) == modifiedSeconds && Int64(value.st_mtimespec.tv_nsec) == modifiedNanoseconds
            && Int64(value.st_ctimespec.tv_sec) == changedSeconds && Int64(value.st_ctimespec.tv_nsec) == changedNanoseconds
    }
}

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
    private let limit: Int
    private var hash = SHA256()
    private var prefix = Data()
    private(set) var count = 0
    init(limit: Int = NativeOEXArchive.maximumBytes) throws {
        guard (1...NativeOEXImportFile.boundedMaximumBytes).contains(limit) else { throw AuthoringFailure("OEX_LIMIT") }
        self.limit = limit; file = try OEXTemporaryFile()
    }
    func append(_ block: Data) throws {
        try Task.checkCancellation()
        guard block.count <= limit - count else { throw AuthoringFailure("OEX_LIMIT") }
        if prefix.count < 4 { prefix.append(block.prefix(4 - prefix.count)) }
        try file.handle.write(contentsOf: block)
        hash.update(data: block); count += block.count
    }
    func finish() throws -> NativeOEXArchive {
        guard count >= 22, prefix == Data([0x50,0x4b,0x03,0x04]) else { throw AuthoringFailure("OEX_RESPONSE_INVALID") }
        var metadata = stat()
        guard fstat(file.handle.fileDescriptor, &metadata) == 0, metadata.st_size == Int64(count),
              metadata.st_mode & S_IFMT == S_IFREG, metadata.st_mode & 0o777 == 0o600,
              metadata.st_uid == geteuid(), metadata.st_nlink == 1 else { throw AuthoringFailure("FILE_CHANGED") }
        let stamp = OEXSnapshotStamp(metadata)
        try file.handle.close()
        return NativeOEXArchive(file: file, count: count, sha256: hash.finalize().map { String(format: "%02x", $0) }.joined(), stamp: stamp)
    }
}

/// A stable, bounded private snapshot of a selected local OEX file.
/// The source path is never persisted or sent to the server.
public struct NativeOEXImportFile: Sendable {
    public static let boundedMaximumBytes = 512 * 1024 * 1024
    public let name: String
    public let archive: NativeOEXArchive
    public static func load(_ url: URL) async throws -> Self {
        try await snapshot(url, limit: NativeOEXArchive.maximumBytes)
    }
    /// Only for a selected operation with a separately admitted live owner.
    public static func loadForBoundedProfile(_ url: URL) async throws -> Self {
        try await snapshot(url, limit: boundedMaximumBytes)
    }
    private static func snapshot(_ url: URL, limit: Int) async throws -> Self {
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
            guard before.st_size > 0, before.st_size <= Int64(limit) else { throw AuthoringFailure("OEX_LIMIT") }
            let writer = try OEXArchiveWriter(limit: limit)
            while let block = try handle.read(upToCount: 256 * 1024), !block.isEmpty { try writer.append(block) }
            var after = stat()
            guard fstat(fd, &after) == 0, after.st_size == before.st_size, Int64(writer.count) == before.st_size,
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
    private let stamp: OEXSnapshotStamp
    public let count: Int
    public let sha256: String
    var fileURL: URL { file.url }
    fileprivate init(file: OEXTemporaryFile, count: Int, sha256: String, stamp: OEXSnapshotStamp) { self.file = file; self.count = count; self.sha256 = sha256; self.stamp = stamp }
    public init(data: Data) throws {
        let writer = try OEXArchiveWriter(); try writer.append(data); self = try writer.finish()
    }
    /// Provisional metadata from the same sealed snapshot. Platform still hashes
    /// and validates the complete archive under the independently held import lease.
    func selectedFormat(helper: URL, workspaceV5ValidationRuntime: URL? = nil, workspaceV6ValidationRuntime: URL? = nil) async throws -> String {
        let ownedFile = file, identity = stamp, expectedCount = count
        let work = Task.detached(priority: .userInitiated) {
            let fd = Darwin.open(ownedFile.url.path, O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK)
            guard fd >= 0 else { throw AuthoringFailure("FILE_CHANGED") }
            let source = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
            defer { try? source.close() }
            var before = stat()
            guard fstat(fd, &before) == 0, identity.matches(before), before.st_size == Int64(expectedCount) else { throw AuthoringFailure("FILE_CHANGED") }
            let child = Process(), pipe = Pipe()
            child.executableURL = helper
            if let runtime = workspaceV6ValidationRuntime {
                child.arguments = ["--oex-selected-format-v6-validation", runtime.path]
            } else {
                child.arguments = workspaceV5ValidationRuntime.map { ["--oex-selected-format-v5-validation", $0.path] } ?? ["--oex-selected-format"]
            }
            child.environment = ["PATH": "/usr/bin:/bin", "LANG": "C"]
            child.standardInput = source; child.standardOutput = pipe; child.standardError = FileHandle.nullDevice
            try Task.checkCancellation(); try child.run()
            try? pipe.fileHandleForWriting.close()
            defer {
                if child.isRunning { child.terminate() }
                child.waitUntilExit()
                try? pipe.fileHandleForReading.close()
            }
            let outFD = pipe.fileHandleForReading.fileDescriptor
            let flags = fcntl(outFD, F_GETFL)
            guard flags >= 0, fcntl(outFD, F_SETFL, flags | O_NONBLOCK) == 0 else { throw AuthoringFailure("OEX_FORMAT_INVALID") }
            let deadline = ContinuousClock.now.advanced(by: .seconds(45))
            var result = Data(), eof = false
            while child.isRunning || !eof {
                try Task.checkCancellation()
                guard ContinuousClock.now < deadline else { throw AuthoringFailure("OEX_FORMAT_INVALID") }
                var block = [UInt8](repeating: 0, count: 1024)
                let n = Darwin.read(outFD, &block, block.count)
                if n == 0 { eof = true }
                else if n > 0 {
                    guard result.count + n <= 1024 else { throw AuthoringFailure("OEX_FORMAT_INVALID") }
                    result.append(contentsOf: block.prefix(n))
                } else if errno != EAGAIN && errno != EINTR { throw AuthoringFailure("OEX_FORMAT_INVALID") }
                if child.isRunning || !eof { try await Task.sleep(for: .milliseconds(10)) }
            }
            child.waitUntilExit(); try Task.checkCancellation()
            var after = stat()
            guard child.terminationStatus == 0, fstat(fd, &after) == 0, identity.matches(after), result.last == 10 else { throw AuthoringFailure("OEX_FORMAT_INVALID") }
            result.removeLast()
            let hint = try JSONDecoder().decode(OEXSelectedFormatHint.self, from: result)
            try hint.check(bytes: result, count: expectedCount, device: before.st_dev, inode: before.st_ino, workspaceV6: workspaceV6ValidationRuntime != nil)
            return hint.archiveFormat
        }
        return try await withTaskCancellationHandler {
            let value = try await work.value; try Task.checkCancellation(); return value
        } onCancel: { work.cancel() }
    }
    /// Reads one deterministic segment from the private snapshot. The
    /// consumer still verifies the full archive digest.
    func boundedChunk(index: Int) throws -> Data {
        let size = 16 * 1024 * 1024
        guard index >= 0, index < 32, index <= (count - 1) / size else { throw AuthoringFailure("OEX_IMPORT_INVALID") }
        let offset = index * size, length = min(size, count - offset)
        let fd = Darwin.open(file.url.path, O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK)
        guard fd >= 0 else { throw AuthoringFailure("FILE_READ_FAILED") }
        defer { Darwin.close(fd) }
        var before = stat()
        guard fstat(fd, &before) == 0, stamp.matches(before) else { throw AuthoringFailure("FILE_CHANGED") }
        var bytes = Data(count: length)
        try bytes.withUnsafeMutableBytes { raw in
            guard let base = raw.baseAddress else { throw AuthoringFailure("FILE_READ_FAILED") }
            var read = 0
            while read < length {
                try Task.checkCancellation()
                let n = Darwin.pread(fd, base.advanced(by: read), length - read, off_t(offset + read))
                if n < 0 && errno == EINTR { continue }
                guard n > 0 else { throw AuthoringFailure("FILE_READ_FAILED") }
                read += n
            }
        }
        var after = stat()
        guard fstat(fd, &after) == 0, stamp.matches(after) else { throw AuthoringFailure("FILE_CHANGED") }
        try Task.checkCancellation()
        return bytes
    }
    /// Selected bounded output only. The caller must first establish the
    /// separately reviewed owner terminal/drain proof for this exact snapshot.
    func writeBoundedVerified(to url: URL) async throws {
        guard url.isFileURL, !url.lastPathComponent.isEmpty, url.lastPathComponent != ".", url.lastPathComponent != ".." else { throw AuthoringFailure("EXPORT_WRITE_FAILED") }
        let ownedFile = file, identity = stamp, expectedHash = sha256, expectedBytes = count
        let work = Task.detached(priority: .userInitiated) {
            let parentFD = Darwin.open(url.deletingLastPathComponent().path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            guard parentFD >= 0 else { throw AuthoringFailure("EXPORT_WRITE_FAILED") }
            defer { Darwin.close(parentFD) }
            // One owner creates and holds this private directory until commit.
            let directory = ".exhibitos-oex-" + UUID().uuidString
            guard mkdirat(parentFD, directory, 0o700) == 0 else { throw AuthoringFailure("EXPORT_WRITE_FAILED") }
            let dirFD = openat(parentFD, directory, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            guard dirFD >= 0 else { throw AuthoringFailure("EXPORT_WRITE_FAILED") }
            defer { Darwin.close(dirFD) }
            var dirIdentity = stat()
            guard fstat(dirFD, &dirIdentity) == 0, dirIdentity.st_mode & S_IFMT == S_IFDIR,
                  dirIdentity.st_mode & 0o777 == 0o700, dirIdentity.st_uid == geteuid() else { throw AuthoringFailure("EXPORT_WRITE_FAILED") }
            defer {
                var namedDir = stat()
                if fstatat(parentFD, directory, &namedDir, AT_SYMLINK_NOFOLLOW) == 0,
                   namedDir.st_dev == dirIdentity.st_dev, namedDir.st_ino == dirIdentity.st_ino { _ = unlinkat(parentFD, directory, AT_REMOVEDIR) }
            }
            let sourceFD = Darwin.open(ownedFile.url.path, O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK)
            guard sourceFD >= 0 else { throw AuthoringFailure("FILE_CHANGED") }
            let input = FileHandle(fileDescriptor: sourceFD, closeOnDealloc: true)
            defer { try? input.close() }
            var before = stat()
            guard fstat(sourceFD, &before) == 0, identity.matches(before), before.st_size == Int64(expectedBytes) else { throw AuthoringFailure("FILE_CHANGED") }
            let fd = openat(dirFD, "archive", O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600)
            guard fd >= 0 else { throw AuthoringFailure("EXPORT_WRITE_FAILED") }
            let output = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
            var outputIdentity = stat()
            guard fstat(fd, &outputIdentity) == 0, outputIdentity.st_mode & S_IFMT == S_IFREG,
                  outputIdentity.st_uid == geteuid(), outputIdentity.st_nlink == 1 else { throw AuthoringFailure("EXPORT_WRITE_FAILED") }
            defer {
                // Remove only the named file and directory identities we created.
                var named = stat()
                if fstatat(dirFD, "archive", &named, AT_SYMLINK_NOFOLLOW) == 0,
                   named.st_dev == outputIdentity.st_dev, named.st_ino == outputIdentity.st_ino { _ = unlinkat(dirFD, "archive", 0) }
                try? output.close()
            }
            var hasher = SHA256(), copied = 0
            while let block = try input.read(upToCount: 256 * 1024), !block.isEmpty {
                try Task.checkCancellation()
                guard block.count <= expectedBytes - copied else { throw AuthoringFailure("FILE_CHANGED") }
                try output.write(contentsOf: block); hasher.update(data: block); copied += block.count
            }
            var after = stat(), namedOutput = stat(), namedDir = stat()
            guard fstat(sourceFD, &after) == 0, identity.matches(after), copied == expectedBytes,
                  hasher.finalize().map({ String(format: "%02x", $0) }).joined() == expectedHash else { throw AuthoringFailure("FILE_CHANGED") }
            guard Darwin.fsync(fd) == 0,
                  fstatat(dirFD, "archive", &namedOutput, AT_SYMLINK_NOFOLLOW) == 0,
                  namedOutput.st_dev == outputIdentity.st_dev, namedOutput.st_ino == outputIdentity.st_ino,
                  fstatat(parentFD, directory, &namedDir, AT_SYMLINK_NOFOLLOW) == 0,
                  namedDir.st_dev == dirIdentity.st_dev, namedDir.st_ino == dirIdentity.st_ino else { throw AuthoringFailure("EXPORT_WRITE_FAILED") }
            try Task.checkCancellation()
            // No existing or concurrently created destination may be replaced.
            // After this exclusive rename succeeds, report committed even if
            // cancellation arrives while the kernel call executes.
            guard renameatx_np(dirFD, "archive", parentFD, url.lastPathComponent, UInt32(RENAME_EXCL)) == 0 else { throw AuthoringFailure("EXPORT_WRITE_FAILED") }
        }
        try await withTaskCancellationHandler { try await work.value } onCancel: { work.cancel() }
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

struct OEXSelectedFormatHint: Decodable {
    let version: Int, kind: String, archiveFormat: String, archiveBytes: Int
    let manifestSha256: String, inputDevice: String, inputInode: String
    func check(bytes: Data, count: Int, device: dev_t, inode: ino_t, workspaceV6: Bool = false) throws {
        let supportedFormats = workspaceV6 ? ["1.0.0-draft.12"] : ["1.0.0-draft.9", "1.0.0-draft.10"]
        guard let object = try JSONSerialization.jsonObject(with: bytes) as? [String: Any],
              Set(object.keys) == Set(["version", "kind", "archiveFormat", "archiveBytes", "manifestSha256", "inputDevice", "inputInode"]),
              version == 1, kind == "oex-format-hint", supportedFormats.contains(archiveFormat),
              archiveBytes == count, inputDevice == String(UInt32(bitPattern: device)), inputInode == String(inode),
              manifestSha256.utf8.count == 64, manifestSha256.utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) }) else { throw AuthoringFailure("OEX_FORMAT_INVALID") }
    }
}
