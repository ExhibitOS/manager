// SPDX-License-Identifier: Apache-2.0
import Foundation
import Darwin
import CryptoKit
import AudioToolbox

public struct NativeUploadState: Codable, Sendable {
    public let format: Int
    public var revision: UInt64
    public let workspaceId: String
    public let tenantId: String
    public let principalId: String
    public var intent: NativeImportIntent?
    public var jobId: String?
    public var fileName: String?
    public var compactVariant: NativeCompactModelVariantTransfer? = nil
    public var gpuVariant: NativeGPUModelPreparationTransfer? = nil

    public func permitsOrdinaryJobAction(_ jobId: String, selectedArtworkId: String?) -> Bool {
        guard gpuVariant == nil, let intent, let selectedArtworkId else { return false }
        return self.jobId == jobId && intent.artworkId == selectedArtworkId && intent.hayesOriginal == nil
            && (intent.productImagePreparation == nil || intent.productImagePreparation?.isFresh == true)
    }

    public func holdsRequiredModelMetadata(for artworkId: String?) -> Bool {
        guard let artworkId else { return false }
        if let gpuVariant { return artworkId.caseInsensitiveCompare(gpuVariant.artworkId) == .orderedSame }
        if let compactVariant { return artworkId.caseInsensitiveCompare(compactVariant.artworkId) == .orderedSame }
        guard let intent, intent.modelPreparation != nil else { return false }
        return artworkId.caseInsensitiveCompare(intent.artworkId) == .orderedSame
    }
}
/// A bounded immutable copy. The selected source is opened read-only without
/// following a final symlink; paths/bookmarks and file bytes never enter IPC.
public struct ImportFileSnapshot: Sendable {
    public static let limit = 32 * 1024 * 1024
    /// A selected source can be admitted even when a later static output may
    /// exceed its per-file limit; publication may choose a different derivative.
    public static func mayExceedStaticOutputLimit(mime: String, bytes: Int) -> Bool {
        mime == "model/gltf-binary" && bytes > 25 * 1024 * 1024
    }
    public let name: String
    public let mime: String
    public let sha256: String
    public let data: Data

    public static func load(_ url: URL, allowJPEG: Bool = false, byteLimit: Int = limit,
                            allowLargeModel: Bool = false, allowWAV: Bool = false, allowMP3: Bool = false) async throws -> Self {
        guard (1...limit).contains(byteLimit) else { throw AuthoringFailure("FILE_SIZE_INVALID") }
        let maximumBytes = allowLargeModel && url.pathExtension.lowercased() == "glb"
            ? NativeModelPreparationIntent.largeSourceLimit : byteLimit
        let worker = Task.detached(priority: .userInitiated) { try read(url, allowJPEG: allowJPEG, allowWAV: allowWAV, allowMP3: allowMP3, limit: maximumBytes) }
        return try await withTaskCancellationHandler {
            let snapshot = try await worker.value
            try Task.checkCancellation()
            return snapshot
        } onCancel: { worker.cancel() }
    }
    private static func read(_ url: URL, allowJPEG: Bool, allowWAV: Bool, allowMP3: Bool, limit: Int) throws -> Self {
        guard url.isFileURL else { throw AuthoringFailure("FILE_LOCAL_REQUIRED") }
        let mime: String
        switch url.pathExtension.lowercased() {
        case "wav":
            guard allowWAV else { throw AuthoringFailure("FILE_FORMAT_UNSUPPORTED") }
            mime = "audio/wav"
        case "mp3":
            guard allowMP3 else { throw AuthoringFailure("FILE_FORMAT_UNSUPPORTED") }
            mime = "audio/mpeg"
        case "glb": mime = "model/gltf-binary"
        case "png": mime = "image/png"
        case "jpg", "jpeg":
            guard allowJPEG else { throw AuthoringFailure("FILE_FORMAT_UNSUPPORTED") }
            mime = "image/jpeg"
        default: throw AuthoringFailure("FILE_FORMAT_UNSUPPORTED")
        }
        let data = try readBytes(url, limit: limit)
        return Self(name: url.lastPathComponent, mime: mime, sha256: digest(data), data: data)
    }

    fileprivate static func readBytes(_ url: URL, limit: Int) throws -> Data {
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
        return data
    }
    fileprivate static func digest(_ data: Data) -> String {
        SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }

    fileprivate static func preparedWAV(_ data: Data) -> Self {
        Self(name: "prepared.wav", mime: "audio/wav", sha256: digest(data), data: data)
    }

    public func matches(_ intent: NativeImportIntent) -> Bool {
        intent.bytes == data.count && intent.mime == mime && intent.sha256 == sha256
    }
}

/// Local, request-bound custody for the original and the exact bytes uploaded on retry.
struct NativeRoomAudioPreparation {
    static let byteLimit = 12 * 1024 * 1024
    private static let recipe = "core-audio-mp3-pcm16le-source-format-v1"
    private let directory: URL
    let wav: ImportFileSnapshot

    static func prepare(_ original: ImportFileSnapshot, profile: URL, requestID: String,
                        scope: DocumentValue) throws -> Self {
        guard original.mime == "audio/mpeg", let id = UUID(uuidString: requestID) else { throw invalid() }
        let directory = profile.appendingPathComponent("native-room-audio-" + id.uuidString.lowercased())
        let binding: DocumentValue = .object([
            "recipe": .string(recipe), "scope": scope, "originalSha256": .string(original.sha256),
            "originalBytes": .integer(Int64(original.data.count))
        ])
        var info = stat()
        if lstat(directory.path, &info) != 0 {
            guard errno == ENOENT else { throw invalid() }
            let staging = profile.appendingPathComponent(".native-room-audio-" + UUID().uuidString)
            guard mkdir(staging.path, 0o700) == 0 else { throw invalid() }
            defer { try? FileManager.default.removeItem(at: staging) }
            let source = staging.appendingPathComponent("original.mp3")
            try write(original.data, to: source)
            let wav = ImportFileSnapshot.preparedWAV(try decode(source))
            try write(wav.data, to: staging.appendingPathComponent("prepared.wav"))
            let prepared: DocumentValue = .object(["binding": binding, "wavSha256": .string(wav.sha256)])
            try write(JSONEncoder().encode(prepared), to: staging.appendingPathComponent("preparation.json"))
            try synchronizeDirectory(staging)
            if renamex_np(staging.path, directory.path, UInt32(RENAME_EXCL)) != 0 && errno != EEXIST {
                throw invalid()
            }
            try synchronizeDirectory(profile)
        }
        guard lstat(directory.path, &info) == 0, info.st_mode & S_IFMT == S_IFDIR,
              info.st_mode & 0o777 == 0o700, info.st_uid == getuid() else { throw invalid() }
        let stored = try readValue(directory.appendingPathComponent("preparation.json"))
        guard stored["binding"] == binding,
              try ImportFileSnapshot.readBytes(directory.appendingPathComponent("original.mp3"),
                                               limit: byteLimit) == original.data else { throw invalid() }
        let wav = ImportFileSnapshot.preparedWAV(try ImportFileSnapshot.readBytes(
            directory.appendingPathComponent("prepared.wav"), limit: byteLimit))
        guard stored["wavSha256"] == .string(wav.sha256), isCanonicalWAV(wav.data) else { throw invalid() }
        return Self(directory: directory, wav: wav)
    }

    func record(_ receipt: NativeRoomAudioReceipt) throws {
        let url = directory.appendingPathComponent("intake.json")
        var info = stat()
        if lstat(url.path, &info) == 0 {
            let previous = try NativeRoomAudioReceipt(Self.readValue(url))
            guard previous.id == receipt.id, previous.mediaAsset == receipt.mediaAsset else { throw Self.invalid() }
            return
        }
        guard errno == ENOENT else { throw Self.invalid() }
        let temp = directory.appendingPathComponent(".intake-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: temp) }
        try Self.write(JSONEncoder().encode(receipt.value), to: temp)
        if renamex_np(temp.path, url.path, UInt32(RENAME_EXCL)) != 0 {
            guard errno == EEXIST else { throw Self.invalid() }
            let previous = try NativeRoomAudioReceipt(Self.readValue(url))
            guard previous.id == receipt.id, previous.mediaAsset == receipt.mediaAsset else { throw Self.invalid() }
        }
        try Self.synchronizeDirectory(directory)
    }

    private static func invalid() -> AuthoringFailure { AuthoringFailure("AUDIO_PREPARATION_INVALID") }

    private static func readValue(_ url: URL) throws -> DocumentValue {
        try JSONDecoder().decode(DocumentValue.self, from: ImportFileSnapshot.readBytes(url, limit: 65_536))
    }

    private static func write(_ data: Data, to url: URL) throws {
        let fd = Darwin.open(url.path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600)
        guard fd >= 0 else { throw invalid() }
        let file = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        try file.write(contentsOf: data)
        try file.synchronize()
        try file.close()
    }

    private static func synchronizeDirectory(_ url: URL) throws {
        let fd = Darwin.open(url.path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        guard fd >= 0 else { throw invalid() }
        defer { Darwin.close(fd) }
        guard fsync(fd) == 0 else { throw invalid() }
    }

    private static func isCanonicalWAV(_ bytes: Data) -> Bool {
        guard bytes.count > 44, bytes.count <= byteLimit else { return false }
        func number(_ offset: Int, size: Int) -> Int {
            (0..<size).reduce(0) { $0 | (Int(bytes[offset + $1]) << ($1 * 8)) }
        }
        let channels = number(22, size: 2)
        let rate = number(24, size: 4)
        let stride = channels * 2
        let length = bytes.count - 44
        return bytes.prefix(4).elementsEqual("RIFF".utf8)
            && number(4, size: 4) == bytes.count - 8
            && bytes[8..<16].elementsEqual("WAVEfmt ".utf8)
            && number(16, size: 4) == 16 && number(20, size: 2) == 1
            && (1...2).contains(channels) && (8000...48000).contains(rate)
            && number(28, size: 4) == rate * stride && number(32, size: 2) == stride
            && number(34, size: 2) == 16 && bytes[36..<40].elementsEqual("data".utf8)
            && number(40, size: 4) == length && length % stride == 0 && length <= 60 * rate * stride
    }

    private static func decode(_ url: URL) throws -> Data {
        var source: AudioFileID?
        guard AudioFileOpenURL(url as CFURL, .readPermission, kAudioFileMP3Type, &source) == noErr,
              let source else { throw invalid() }
        defer { AudioFileClose(source) }
        var fileType: AudioFileTypeID = 0
        var size = UInt32(MemoryLayout<AudioFileTypeID>.size)
        guard AudioFileGetProperty(source, kAudioFilePropertyFileFormat, &size, &fileType) == noErr,
              fileType == kAudioFileMP3Type else { throw invalid() }
        var decoder: ExtAudioFileRef?
        guard ExtAudioFileWrapAudioFileID(source, false, &decoder) == noErr, let decoder else { throw invalid() }
        defer { ExtAudioFileDispose(decoder) }

        var format = AudioStreamBasicDescription()
        size = UInt32(MemoryLayout<AudioStreamBasicDescription>.size)
        guard ExtAudioFileGetProperty(decoder, kExtAudioFileProperty_FileDataFormat, &size, &format) == noErr,
              format.mFormatID == kAudioFormatMPEGLayer3, (1...2).contains(format.mChannelsPerFrame),
              (8000...48000).contains(format.mSampleRate),
              format.mSampleRate.rounded() == format.mSampleRate else { throw invalid() }
        let channels = format.mChannelsPerFrame
        let rate = UInt32(format.mSampleRate)
        let stride = channels * 2
        var pcm = AudioStreamBasicDescription(mSampleRate: format.mSampleRate, mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked, mBytesPerPacket: stride,
            mFramesPerPacket: 1, mBytesPerFrame: stride, mChannelsPerFrame: channels, mBitsPerChannel: 16, mReserved: 0)
        guard ExtAudioFileSetProperty(decoder, kExtAudioFileProperty_ClientDataFormat, size, &pcm) == noErr else { throw invalid() }

        let chunkFrames: UInt32 = 4096
        let limit = min(byteLimit - 44, Int(60 * rate * stride))
        var samples = Data()
        var chunk = Data(count: Int(chunkFrames * stride))
        while true {
            try Task.checkCancellation()
            var frames = chunkFrames
            let result = chunk.withUnsafeMutableBytes { raw in
                var buffers = AudioBufferList(mNumberBuffers: 1,
                    mBuffers: AudioBuffer(mNumberChannels: channels, mDataByteSize: UInt32(raw.count), mData: raw.baseAddress))
                return ExtAudioFileRead(decoder, &frames, &buffers)
            }
            guard result == noErr, frames <= chunkFrames else { throw invalid() }
            if frames == 0 { break }
            let newBytes = Int(frames * stride)
            guard newBytes <= limit - samples.count else { throw invalid() }
            samples.append(contentsOf: chunk.prefix(newBytes))
        }
        guard !samples.isEmpty else { throw invalid() }

        var wave = Data()
        func number<T: FixedWidthInteger>(_ value: T) {
            withUnsafeBytes(of: value.littleEndian) { wave.append(contentsOf: $0) }
        }
        wave.append(contentsOf: "RIFF".utf8)
        number(UInt32(samples.count + 36))
        wave.append(contentsOf: "WAVEfmt ".utf8)
        number(UInt32(16))
        number(UInt16(1))
        number(UInt16(channels))
        number(rate)
        number(rate * stride)
        number(UInt16(stride))
        number(UInt16(16))
        wave.append(contentsOf: "data".utf8)
        number(UInt32(samples.count))
        wave.append(samples)
        return wave
    }
}
