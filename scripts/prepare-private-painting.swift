// SPDX-License-Identifier: Apache-2.0
import Darwin
import Foundation

/// Standalone debug tool. This source is outside all app and package targets.
@main
struct PreparePrivatePainting {
    private enum Failure: Error {
        case arguments, source, destination
    }

    private static func readSource(_ path: String, expectedBytes: Int) throws -> Data {
        let fd = open(path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)
        guard fd >= 0 else { throw Failure.source }
        let input = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        defer { try? input.close() }
        var metadata = stat()
        guard fstat(fd, &metadata) == 0, metadata.st_mode & S_IFMT == S_IFREG,
              metadata.st_size == expectedBytes else { throw Failure.source }
        let bytes = try input.read(upToCount: expectedBytes + 1) ?? Data()
        guard bytes.count == expectedBytes else { throw Failure.source }
        return bytes
    }

    private static func run() throws {
        let args = CommandLine.arguments
        guard args.count == 4, let painting = NativeImagePreparation.PrivatePainting(rawValue: args[1]),
              args[2].hasPrefix("/"), args[3].hasPrefix("/") else { throw Failure.arguments }
        let source = URL(fileURLWithPath: args[2]).standardizedFileURL
        let destination = URL(fileURLWithPath: args[3], isDirectory: true).standardizedFileURL
        let parent = destination.deletingLastPathComponent()
        let sourceParent = source.deletingLastPathComponent()
        guard source.path == args[2], source.resolvingSymlinksInPath().path == source.path,
              destination.path == args[3], parent.resolvingSymlinksInPath().path == parent.path,
              parent.path != sourceParent.path, !parent.path.hasPrefix(sourceParent.path + "/") else { throw Failure.arguments }

        var info = stat()
        guard lstat(parent.path, &info) == 0, info.st_mode & S_IFMT == S_IFDIR,
              info.st_mode & 0o777 == 0o700, info.st_uid == geteuid(),
              lstat(destination.path, &info) != 0, errno == ENOENT else { throw Failure.destination }

        let jpeg = try readSource(source.path, expectedBytes: painting.sourceBytes)
        let prepared = try NativeImagePreparation.preparePrivatePainting(painting, jpeg: jpeg, isCurrent: { true })

        // Publish PNG and receipt together only after both exclusive writes succeed.
        let stage = parent.appendingPathComponent(".painting-" + UUID().uuidString, isDirectory: true)
        guard mkdir(stage.path, 0o700) == 0 else { throw Failure.destination }
        var published = false
        defer { if !published { try? FileManager.default.removeItem(at: stage) } }
        try prepared.png.write(to: stage.appendingPathComponent("full.png"), options: .withoutOverwriting)
        try prepared.receipt.write(to: stage.appendingPathComponent("receipt.json"), options: .withoutOverwriting)
        guard renamex_np(stage.path, destination.path, UInt32(RENAME_EXCL)) == 0 else { throw Failure.destination }
        published = true
    }

    static func main() {
        _ = umask(0o077)
        do {
            try run()
        } catch let limit as NativeImagePreparation.EncodedPNGLimit {
            let maximum = NativeImagePreparation.EncodedPNGLimit.maximumBytes
            let message = "PRIVATE_PAINTING_PREPARATION_FAILED: limits stage=imageio-encoded-png observedBytes=\(limit.observedBytes) limitBytes=\(maximum)\n"
            FileHandle.standardError.write(Data(message.utf8))
            exit(1)
        } catch {
            // File-system errors can contain private paths. Report a fixed diagnostic.
            let detail = (error as? Failure).map { String(describing: $0) }
                ?? (error as? NativeImagePreparation.Failure).map { String(describing: $0) }
                ?? "io"
            FileHandle.standardError.write(Data("PRIVATE_PAINTING_PREPARATION_FAILED: \(detail)\n".utf8))
            exit(1)
        }
    }
}
