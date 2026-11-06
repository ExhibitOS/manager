// SPDX-License-Identifier: Apache-2.0
import Foundation
import CryptoKit
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers
import zlib

/// System ImageIO glue for the closed native JPEG role recipe. This prepares
/// bytes only: storage, original/container membership, rights and registration
/// remain the import owner's responsibility.
public enum NativeImagePreparation {
    public static let algorithm = "nearest-source-colourspace-jpeg-rgb8-v1"
    public static let iccAlgorithm = "icc-to-srgb-jpeg-rgb8-v1"
    public static let tool = "ExhibitOSManager.ImageIO"
    public static let toolVersion = "1"
    public static let iccToolVersion = "2"
    public enum Role: String, Codable, Sendable { case baseColor, normal, occlusion }
    public enum Failure: Error, Equatable { case cancelledOrStale, binding, settings, limits, unsupportedJPEG, unsupportedPixels, encoding, outputMismatch }
    public struct Source: Sendable {
        public let role: Role
        public let imageIndex: Int
        public let jpeg: Data
        public let sha256: String
        public init(role: Role, imageIndex: Int, jpeg: Data, sha256: String) {
            self.role = role; self.imageIndex = imageIndex; self.jpeg = Data(jpeg); self.sha256 = sha256
        }
    }
    public struct Binding: Codable, Equatable, Sendable {
        public let operationID: String
        public let generation: String
        public let originalSHA256: String
        public let settingsSHA256: String
        public let recipeID: String
        public let recipeVersion: String
        public init(operationID: String, generation: String, originalSHA256: String, settingsSHA256: String,
                    recipeID: String, recipeVersion: String) {
            self.operationID = operationID; self.generation = generation; self.originalSHA256 = originalSHA256
            self.settingsSHA256 = settingsSHA256; self.recipeID = recipeID; self.recipeVersion = recipeVersion
        }
    }
    public struct ImageReport: Codable, Sendable {
        public let role: Role
        public let sourceImageIndex: Int
        public let sourceJPEGSHA256: String
        public let sourceBytes: Int
        public let sourceWidth: Int
        public let sourceHeight: Int
        public let sourceChannels: Int
        public let sourceDecodedSamplesSHA256: String
        public let sourceRGB8SHA256: String
        public let outputWidth: Int
        public let outputHeight: Int
        public let outputRGB8SHA256: String
        public let outputPNGSHA256: String
        public let outputBytes: Int
        public let spatialSampling: Bool
        public let grayExpandedToRGB: Bool
        public let colourMeaning: String
        public let sourceICCSHA256: String?
        public let colourTransferConverted: Bool?
        public let explicitBufferPayloadBytes: Int
    }
    public struct Image: Sendable { public let png: Data; public let report: ImageReport }
    public struct Candidate: Sendable {
        public let binding: Binding
        public let images: [Image]
        public let receipt: Data
    }
    private struct Receipt: Encodable {
        let version = 1
        let algorithm: String
        let tool = NativeImagePreparation.tool
        let toolVersion: String
        let decoderOS = ProcessInfo.processInfo.operatingSystemVersionString
        let binding: Binding
        let images: [ImageReport]
        let originalStored = false
        let sourceContainerBytesVerified = false
        let sourceContainerMembershipVerified = false
        let consumerAdmitted = false
        let qualityApproved = false
        let colourTransferConverted: Bool
        let normalRenormalized = false
        let normalDirectionRemapped = false
        let geometryChanged = false
        let samplersChanged = false
        let tangentsChanged = false
        let coordinateUnits = "unknown"
        let decoderAllocatorPeak = "unmeasured; explicit payload counts are not RSS"
    }
    private struct Pixels {
        let width: Int, height: Int, channels: Int
        let samples: Data, sourceRGB: Data, rgb: Data
    }
    private static func hash(_ bytes: Data) -> String { SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined() }
    private static func digest(_ value: String) -> Bool { value.utf8.count == 64 && value.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) } }
    private static func token(_ value: String) -> Bool {
        !value.isEmpty && value.utf8.count <= 128 && value.utf8.allSatisfy { (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0) || [45, 46, 95].contains($0) }
    }
    private static func check(_ isCurrent: @Sendable () -> Bool) throws {
        guard !Task<Never, Never>.isCancelled, isCurrent() else { throw Failure.cancelledOrStale }
    }
    /// The actual detached handle is cancelled and awaited; ImageIO's in-flight
    /// native decode/encode may finish cooperatively before this call returns.
    public static func prepare(sources: [Source], binding: Binding, settingsBytes: Data,
                               isCurrent: @escaping @Sendable () -> Bool) async throws -> Candidate {
        let worker = Task.detached(priority: .userInitiated) {
            try prepareSynchronously(sources: sources, binding: binding, settingsBytes: settingsBytes, isCurrent: isCurrent)
        }
        return try await withTaskCancellationHandler {
            do {
                let candidate = try await worker.value
                try check(isCurrent)
                return candidate
            } catch {
                try check(isCurrent) // parent cancellation/currentness also takes precedence after worker failure
                throw error
            }
        } onCancel: { worker.cancel() }
    }
    public static func prepareSynchronously(sources: [Source], binding: Binding, settingsBytes: Data,
                                            isCurrent: @Sendable () -> Bool) throws -> Candidate {
        do {
            try check(isCurrent)
            guard [binding.operationID, binding.generation, binding.recipeID, binding.recipeVersion].allSatisfy(token),
                  digest(binding.originalSHA256), digest(binding.settingsSHA256), hash(settingsBytes) == binding.settingsSHA256 else { throw Failure.binding }
            guard settingsBytes.count <= 4096,
                  let settings = try? JSONSerialization.jsonObject(with: settingsBytes) as? [String: Any],
                  Set(settings.keys) == ["algorithm", "maximumEdge", "allowSpatialSampling"],
                  let selectedAlgorithm = settings["algorithm"] as? String,
                  [algorithm, iccAlgorithm].contains(selectedAlgorithm),
                  let edge = settings["maximumEdge"] as? NSNumber, CFGetTypeID(edge) != CFBooleanGetTypeID(), edge.doubleValue == 2048,
                  let sampling = settings["allowSpatialSampling"] as? NSNumber, CFGetTypeID(sampling) == CFBooleanGetTypeID() else { throw Failure.settings }
            guard !sources.isEmpty, sources.count <= 3, Set(sources.map(\.role)).count == sources.count,
                  Set(sources.map(\.imageIndex)).count == sources.count,
                  sources.allSatisfy({ (0..<8).contains($0.imageIndex) && !$0.jpeg.isEmpty && $0.jpeg.count <= 32 * 1024 * 1024 }),
                  sources.reduce(0, { $0 + $1.jpeg.count }) <= 32 * 1024 * 1024 else { throw Failure.limits }
            guard selectedAlgorithm != iccAlgorithm || (sources.count == 1 && sources[0].role == .baseColor) else { throw Failure.settings }
            var images: [Image] = []
            for input in sources {
                try check(isCurrent)
                guard digest(input.sha256), hash(input.jpeg) == input.sha256 else { throw Failure.binding }
                let image = try autoreleasepool {
                    let profile = selectedAlgorithm == iccAlgorithm ? try embeddedICC(input.jpeg, isCurrent: isCurrent) : nil
                    if selectedAlgorithm == iccAlgorithm && profile == nil { throw Failure.unsupportedJPEG }
                    return try prepareImage(input, profile: profile, allowSpatialSampling: sampling.boolValue,
                        isCurrent: isCurrent).image
                }
                images.append(image)
            }
            guard images.reduce(0, { $0 + $1.report.outputWidth * $1.report.outputHeight }) <= 12_582_912 else { throw Failure.limits }
            let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys]
            let candidate = Candidate(binding: binding, images: images, receipt: try encoder.encode(Receipt(algorithm: selectedAlgorithm, toolVersion: selectedAlgorithm == iccAlgorithm ? iccToolVersion : toolVersion, binding: binding, images: images.map(\.report), colourTransferConverted: selectedAlgorithm == iccAlgorithm)))
            try check(isCurrent)
            return candidate
        } catch {
            try check(isCurrent) // cancellation/staleness takes precedence over format errors
            if error is EncodedPNGLimit { throw Failure.limits }
            throw error
        }
    }
    fileprivate struct Size: Equatable {
        let width: Int
        let height: Int
    }
    private enum PNGEncoding {
        case imageIO
        #if DEBUG
        case losslessPrivatePainting
        #endif
    }

    private static func prepareImage(_ input: Source, profile: Data?, allowSpatialSampling: Bool,
                                     sourceSize: Size? = nil, outputSize: Size? = nil,
                                     maximumEdge: Int = 2048, maximumPixels: Int = 4_194_304,
                                     pngEncoding: PNGEncoding = .imageIO,
                                     isCurrent: @Sendable () -> Bool) throws -> (image: Image, decodedPNGHash: String?) {
        let source = try decodeJPEG(input, profile: profile, expectedSize: sourceSize, isCurrent: isCurrent)
        let scale = min(1, 2048 / Double(max(source.width, source.height)))
        let width = outputSize?.width ?? max(1, Int(floor(Double(source.width) * scale)))
        let height = outputSize?.height ?? max(1, Int(floor(Double(source.height) * scale)))
        let resized = width != source.width || height != source.height
        guard !resized || allowSpatialSampling else { throw Failure.settings }
        let rgb = try resample(source.rgb, sourceWidth: source.width, sourceHeight: source.height,
            width: width, height: height, maximumEdge: maximumEdge, isCurrent: isCurrent)
        let png: Data
        switch pngEncoding {
        case .imageIO:
            png = try encode(rgb, width: width, height: height, role: input.role, maximumEdge: maximumEdge,
                maximumPixels: maximumPixels, isCurrent: isCurrent)
        #if DEBUG
        case .losslessPrivatePainting:
            png = try losslessPaintingPNG(rgb, width, height, isCurrent)
        #endif
        }
        let verified = try decodePNG(png, width: width, height: height, isCurrent: isCurrent)
        guard verified == rgb else { throw Failure.outputMismatch }
        let report = ImageReport(role: input.role, sourceImageIndex: input.imageIndex,
            sourceJPEGSHA256: input.sha256, sourceBytes: input.jpeg.count,
            sourceWidth: source.width, sourceHeight: source.height, sourceChannels: source.channels,
            sourceDecodedSamplesSHA256: hash(source.samples), sourceRGB8SHA256: hash(source.sourceRGB),
            outputWidth: width, outputHeight: height, outputRGB8SHA256: hash(rgb),
            outputPNGSHA256: hash(png), outputBytes: png.count, spatialSampling: resized,
            grayExpandedToRGB: source.channels == 1,
            colourMeaning: profile != nil ? "ICC-converted sRGB; not physical colour calibration" : (input.role == .baseColor ? "decoded source RGB codes; display sRGB assumed, not calibrated" : "linear numeric RGB codes; no transfer conversion"),
            sourceICCSHA256: profile.map(hash), colourTransferConverted: profile == nil ? nil : true,
            explicitBufferPayloadBytes: input.jpeg.count + source.samples.count + source.rgb.count + (profile == nil ? 0 : source.sourceRGB.count + source.width * source.height * 4) + rgb.count + png.count + verified.count)
        return (Image(png: png, report: report), pngEncoding == .imageIO ? nil : hash(verified))
    }
    // Internal for the focused numeric recipe test; no format/registration API.
    static func resample(_ source: Data, sourceWidth: Int, sourceHeight: Int, width: Int, height: Int,
                         isCurrent: @Sendable () -> Bool) throws -> Data {
        try resample(source, sourceWidth: sourceWidth, sourceHeight: sourceHeight, width: width, height: height,
                     maximumEdge: 2048, isCurrent: isCurrent)
    }
    private static func resample(_ source: Data, sourceWidth: Int, sourceHeight: Int, width: Int, height: Int,
                                 maximumEdge: Int, isCurrent: @Sendable () -> Bool) throws -> Data {
        guard (1...4096).contains(sourceWidth), (1...4096).contains(sourceHeight),
              (1...maximumEdge).contains(width), (1...maximumEdge).contains(height), width <= sourceWidth, height <= sourceHeight,
              source.count == sourceWidth * sourceHeight * 3 else { throw Failure.limits }
        var output = Data(count: width * height * 3)
        try output.withUnsafeMutableBytes { targetRaw in
            try source.withUnsafeBytes { sourceRaw in
                let target = targetRaw.bindMemory(to: UInt8.self), pixels = sourceRaw.bindMemory(to: UInt8.self)
                for y in 0..<height {
                    try check(isCurrent)
                    let sy = min(sourceHeight - 1, ((2 * y + 1) * sourceHeight) / (2 * height))
                    for x in 0..<width {
                        let sx = min(sourceWidth - 1, ((2 * x + 1) * sourceWidth) / (2 * width))
                        let src = (sy * sourceWidth + sx) * 3, dst = (y * width + x) * 3
                        target[dst] = pixels[src]; target[dst + 1] = pixels[src + 1]; target[dst + 2] = pixels[src + 2]
                    }
                }
            }
        }
        return output
    }
    private static func decodeJPEG(_ input: Source, profile: Data?, expectedSize: Size?, isCurrent: @Sendable () -> Bool) throws -> Pixels {
        if profile == nil { try rejectICC(input.jpeg) }
        let options = [kCGImageSourceShouldCache: false, kCGImageSourceShouldAllowFloat: false] as CFDictionary
        guard let source = CGImageSourceCreateWithData(input.jpeg as CFData, options),
              CGImageSourceGetType(source) as String? == UTType.jpeg.identifier,
              CGImageSourceGetCount(source) == 1, CGImageSourceGetStatus(source) == .statusComplete,
              let props = CGImageSourceCopyPropertiesAtIndex(source, 0, options) as? [CFString: Any],
              let w = props[kCGImagePropertyPixelWidth] as? NSNumber, let h = props[kCGImagePropertyPixelHeight] as? NSNumber,
              w.intValue > 0, h.intValue > 0, w.intValue <= 4096, h.intValue <= 4096,
              ((props[kCGImagePropertyOrientation] as? NSNumber)?.intValue ?? 1) == 1 else { throw Failure.unsupportedJPEG }
        if let expectedSize {
            guard Size(width: w.intValue, height: h.intValue) == expectedSize else { throw Failure.binding }
        }
        if let tiff = props[kCGImagePropertyTIFFDictionary] as? [CFString: Any],
           let orientation = tiff[kCGImagePropertyTIFFOrientation] as? NSNumber, orientation.intValue != 1 { throw Failure.unsupportedJPEG }
        try check(isCurrent)
        guard let image = CGImageSourceCreateImageAtIndex(source, 0, options),
              image.width == w.intValue, image.height == h.intValue else { throw Failure.unsupportedJPEG }
        try check(isCurrent)
        let original = try pixels(image, allowGray: input.role == .occlusion, isCurrent: isCurrent)
        guard let profile else { return original }
        guard input.role == .baseColor, original.channels == 3,
              let embeddedSpace = CGColorSpace(iccData: profile as CFData), embeddedSpace.model == .rgb,
              let decodedSpace = image.colorSpace, decodedSpace.model == .rgb,
              let decodedProfile = decodedSpace.copyICCData() as Data?, hash(decodedProfile) == hash(profile),
              let sourceImage = image.copy(colorSpace: embeddedSpace),
              let targetSpace = CGColorSpace(name: CGColorSpace.sRGB),
              let context = CGContext(data: nil, width: image.width, height: image.height, bitsPerComponent: 8,
                  bytesPerRow: image.width * 4, space: targetSpace,
                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue),
              let raw = context.data else { throw Failure.unsupportedPixels }
        context.setBlendMode(.copy); context.interpolationQuality = .none
        context.draw(sourceImage, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        let rgba = raw.bindMemory(to: UInt8.self, capacity: image.width * image.height * 4)
        var converted = Data(capacity: image.width * image.height * 3)
        for y in 0..<image.height {
            try check(isCurrent)
            for x in 0..<image.width {
                let offset = (y * image.width + x) * 4
                guard rgba[offset + 3] == 255 else { throw Failure.unsupportedPixels }
                converted.append(contentsOf: [rgba[offset], rgba[offset + 1], rgba[offset + 2]])
            }
        }
        return Pixels(width: image.width, height: image.height, channels: original.channels, samples: original.samples, sourceRGB: original.rgb, rgb: converted)
    }
    private static func pixels(_ image: CGImage, allowGray: Bool, isCurrent: @Sendable () -> Bool) throws -> Pixels {
        let gray = image.colorSpace?.model == .monochrome
        guard image.bitsPerComponent == 8, !image.bitmapInfo.contains(.floatComponents), image.decode == nil,
              (image.colorSpace?.model == .rgb && [24, 32].contains(image.bitsPerPixel)) || (allowGray && gray && image.bitsPerPixel == 8),
              image.width > 0, image.height > 0, image.width <= 4096, image.height <= 4096,
              let provider = image.dataProvider, let data = provider.data else { throw Failure.unsupportedPixels }
        let channels = gray ? 1 : 3, stride = image.width * channels
        let storageChannels = image.bitsPerPixel / 8
        let offsets: [Int]
        if storageChannels == 1 || storageChannels == 3 {
            guard image.alphaInfo == .none else { throw Failure.unsupportedPixels }
            offsets = Array(0..<storageChannels)
        } else {
            let little = image.byteOrderInfo == .order32Little
            guard [.orderDefault, .order32Big, .order32Little].contains(image.byteOrderInfo) else { throw Failure.unsupportedPixels }
            if image.alphaInfo == .noneSkipLast { offsets = little ? [3, 2, 1] : [0, 1, 2] }
            else if image.alphaInfo == .noneSkipFirst { offsets = little ? [2, 1, 0] : [1, 2, 3] }
            else { throw Failure.unsupportedPixels }
        }
        guard image.bytesPerRow >= image.width * storageChannels, image.bytesPerRow <= image.width * storageChannels + 256,
              CFDataGetLength(data) == image.bytesPerRow * image.height else { throw Failure.unsupportedPixels }
        var samples = Data(capacity: stride * image.height), rgb = Data(capacity: image.width * image.height * 3)
        guard let raw = CFDataGetBytePtr(data) else { throw Failure.unsupportedPixels }
        for y in 0..<image.height {
            try check(isCurrent)
            let row = raw.advanced(by: y * image.bytesPerRow)
            for x in 0..<image.width {
                for channel in offsets { samples.append(row[x * storageChannels + channel]) }
                if gray { rgb.append(contentsOf: [row[x], row[x], row[x]]) }
                else {
                    let values = offsets.map { row[x * storageChannels + $0] }
                    rgb.append(contentsOf: values)
                }
            }
        }
        return Pixels(width: image.width, height: image.height, channels: channels, samples: samples, sourceRGB: rgb, rgb: rgb)
    }
    /// Extract a complete, bounded ICC profile from JPEG APP2 fragments. No
    /// profile is assumed from a name, and malformed/duplicate parts fail.
    static func embeddedICC(_ data: Data, isCurrent: @Sendable () -> Bool) throws -> Data? {
        try check(isCurrent)
        guard data.count >= 4, data[0] == 255, data[1] == 216 else { throw Failure.unsupportedJPEG }
        let signature = Data("ICC_PROFILE\0".utf8)
        var offset = 2, markers = 0, expected: Int?, fragments: [Int: Data] = [:], reachedScan = false
        while offset < data.count {
            try check(isCurrent)
            markers += 1; guard markers <= 4096, data[offset] == 255 else { throw Failure.unsupportedJPEG }
            while offset < data.count && data[offset] == 255 { offset += 1 }
            guard offset < data.count else { throw Failure.unsupportedJPEG }
            let marker = data[offset]; offset += 1
            if marker == 218 { reachedScan = true; break }
            guard marker != 0, marker != 216, marker != 217, !(208...215).contains(marker), offset + 2 <= data.count else { throw Failure.unsupportedJPEG }
            let size = Int(data[offset]) * 256 + Int(data[offset + 1])
            guard size >= 2, size <= data.count - offset else { throw Failure.unsupportedJPEG }
            let payload = data[(offset + 2)..<(offset + size)]
            if marker == 226 && payload.starts(with: signature) {
                guard payload.count >= 14 else { throw Failure.unsupportedJPEG }
                let start = payload.startIndex, sequence = Int(payload[start + 12]), count = Int(payload[start + 13])
                guard count > 0, sequence > 0, sequence <= count, expected == nil || expected == count, fragments[sequence] == nil else { throw Failure.unsupportedJPEG }
                expected = count; fragments[sequence] = Data(payload[(start + 14)..<payload.endIndex])
                guard fragments.values.reduce(0, { $0 + $1.count }) <= 1_048_576 else { throw Failure.limits }
            }
            offset += size
        }
        guard reachedScan else { throw Failure.unsupportedJPEG }
        guard let count = expected else {
            guard data.range(of: signature) == nil else { throw Failure.unsupportedJPEG }
            return nil
        }
        guard fragments.count == count else { throw Failure.unsupportedJPEG }
        var profile = Data()
        for i in 1...count { guard let fragment = fragments[i] else { throw Failure.unsupportedJPEG }; profile.append(fragment) }
        guard profile.count >= 128,
              Int(profile[0..<4].reduce(UInt32(0), { ($0 << 8) | UInt32($1) })) == profile.count,
              profile[16..<20] == Data("RGB ".utf8), profile[36..<40] == Data("acsp".utf8) else { throw Failure.unsupportedJPEG }
        try check(isCurrent)
        return profile
    }
    /// Inspect JPEG marker metadata only; ImageIO remains the decoder. ICC is
    /// outside this source-code-preserving recipe, never silently transformed.
    private static func rejectICC(_ data: Data) throws {
        guard data.count >= 4, data[0] == 255, data[1] == 216,
              data.range(of: Data("ICC_PROFILE\0".utf8)) == nil else { throw Failure.unsupportedJPEG }
        var offset = 2, markers = 0
        while offset < data.count {
            markers += 1; guard markers <= 4096, data[offset] == 255 else { throw Failure.unsupportedJPEG }
            while offset < data.count && data[offset] == 255 { offset += 1 }
            guard offset < data.count else { throw Failure.unsupportedJPEG }
            let marker = data[offset]; offset += 1
            if marker == 218 { return }
            guard marker != 0, marker != 216, marker != 217, !(208...215).contains(marker), offset + 2 <= data.count else { throw Failure.unsupportedJPEG }
            let size = Int(data[offset]) * 256 + Int(data[offset + 1])
            guard size >= 2, size <= data.count - offset else { throw Failure.unsupportedJPEG }
            if marker == 226 && data[(offset + 2)..<(offset + size)].starts(with: Data("ICC_PROFILE\0".utf8)) { throw Failure.unsupportedJPEG }
            offset += size
        }
        throw Failure.unsupportedJPEG
    }
    struct EncodedPNGLimit: Error {
        static let maximumBytes = 8_388_608
        enum Stage: String {
            case imageIO = "imageio-encoded-png"
            case lossless = "lossless-encoded-png"
        }
        let observedBytes: Int
        let stage: Stage
    }
    static func checkEncodedPNGSize(_ byteCount: Int, stage: EncodedPNGLimit.Stage = .imageIO) throws {
        guard byteCount <= EncodedPNGLimit.maximumBytes else { throw EncodedPNGLimit(observedBytes: byteCount, stage: stage) }
    }

    private static func encode(_ rgb: Data, width: Int, height: Int, role: Role,
                               maximumEdge: Int, maximumPixels: Int, isCurrent: @Sendable () -> Bool) throws -> Data {
        try check(isCurrent)
        guard width <= maximumEdge, height <= maximumEdge, width * height <= maximumPixels,
              let provider = CGDataProvider(data: rgb as CFData),
              let image = CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 24,
                bytesPerRow: width * 3, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue),
                provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent) else { throw Failure.encoding }
        let encoded = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(encoded, UTType.png.identifier as CFString, 1, nil) else { throw Failure.encoding }
        CGImageDestinationAddImage(destination, image, nil)
        guard CGImageDestinationFinalize(destination) else { throw Failure.encoding }
        try check(isCurrent)
        try checkEncodedPNGSize(encoded.length)
        return try cleanPNG(encoded as Data, width: width, height: height, role: role, isCurrent: isCurrent)
    }
    private static func u32(_ data: Data, _ at: Int) -> Int { (0..<4).reduce(0) { ($0 << 8) | Int(data[at + $1]) } }
    /// Validate the bounded system output before removing metadata. ImageIO may
    /// split one compressed stream into many IDAT chunks; join those payloads
    /// without changing them so the consumer receives only three or four chunks.
    static func cleanPNG(_ data: Data, width: Int, height: Int, role: Role, isCurrent: @Sendable () -> Bool) throws -> Data {
        let signature = Data([137,80,78,71,13,10,26,10])
        guard data.count >= 33, data.count <= 8_388_608, data.prefix(8) == signature else { throw Failure.encoding }
        var result = signature, compressed = Data(), offset = 8, idat = false, closedIDAT = false, ended = false
        while offset < data.count {
            try check(isCurrent)
            // Every iteration consumes at least 12 bytes of the bounded input.
            guard data.count - offset >= 12 else { throw Failure.encoding }
            let size = u32(data, offset)
            guard size <= data.count - offset - 12 else { throw Failure.encoding }
            let end = offset + size + 12, name = data[(offset + 4)..<(offset + 8)]
            guard name.allSatisfy({ (65...90).contains($0) || (97...122).contains($0) }),
                  (65...90).contains(data[offset + 6]) else { throw Failure.encoding }
            let crcInput = data.subdata(in: (offset + 4)..<(end - 4))
            let actual = crcInput.withUnsafeBytes { crc32(0, $0.bindMemory(to: Bytef.self).baseAddress, uInt(crcInput.count)) }
            guard actual == uLong(u32(data, end - 4)), !ended else { throw Failure.encoding }
            if offset == 8 {
                guard name == Data("IHDR".utf8), size == 13, u32(data, offset + 8) == width, u32(data, offset + 12) == height,
                      data[offset + 16] == 8, data[offset + 17] == 2,
                      data[offset + 18] == 0, data[offset + 19] == 0, data[offset + 20] == 0 else { throw Failure.encoding }
                result.append(data[offset..<end])
                if role == .baseColor {
                    let payload = Data([115, 82, 71, 66, 0]) // sRGB: explicitly assumed perceptual display intent
                    let crc = payload.withUnsafeBytes { crc32(0, $0.bindMemory(to: Bytef.self).baseAddress, uInt(payload.count)) }
                    result.append(contentsOf: [0, 0, 0, 1]); result.append(payload)
                    result.append(contentsOf: (0..<4).reversed().map { UInt8(truncatingIfNeeded: crc >> ($0 * 8)) })
                }
            } else if name == Data("IDAT".utf8) {
                guard !closedIDAT else { throw Failure.encoding }
                idat = true; compressed.append(data[(offset + 8)..<(end - 4)])
            }
            else if name == Data("IEND".utf8) {
                guard size == 0, idat, end == data.count else { throw Failure.encoding }
                let type = Data("IDAT".utf8)
                let initial = type.withUnsafeBytes { crc32(0, $0.bindMemory(to: Bytef.self).baseAddress, uInt(type.count)) }
                let crc = compressed.withUnsafeBytes { crc32(initial, $0.bindMemory(to: Bytef.self).baseAddress, uInt(compressed.count)) }
                result.append(contentsOf: (0..<4).reversed().map { UInt8(truncatingIfNeeded: compressed.count >> ($0 * 8)) })
                result.append(type); result.append(compressed)
                result.append(contentsOf: (0..<4).reversed().map { UInt8(truncatingIfNeeded: crc >> ($0 * 8)) })
                ended = true; result.append(data[offset..<end])
            } else {
                guard name.first.map({ $0 & 32 != 0 }) == true else { throw Failure.encoding }
                if idat { closedIDAT = true }
            }
            offset = end
        }
        guard ended, result.count <= 8_388_608 else { throw Failure.encoding }
        return result
    }
    static func decodePNG(_ png: Data, width: Int, height: Int, isCurrent: @Sendable () -> Bool) throws -> Data {
        try check(isCurrent)
        guard let source = CGImageSourceCreateWithData(png as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
              let image = CGImageSourceCreateImageAtIndex(source, 0, nil), image.width == width, image.height == height else { throw Failure.outputMismatch }
        return try pixels(image, allowGray: false, isCurrent: isCurrent).rgb
    }
}

#if DEBUG
extension NativeImagePreparation {
    /// Debug-only source preparation for the three reviewed demo originals.
    /// The caller selects an identity, never an edge, pixel or colour setting.
    enum PrivatePainting: String, CaseIterable {
        case matterhorn = "met-10158"
        case aristotle = "met-437394"
        case fishing = "met-436951"

        var sourceBytes: Int {
            switch self {
            case .matterhorn: 2_000_787
            case .aristotle: 3_486_903
            case .fishing: 3_419_265
            }
        }

        var sourceSHA256: String {
            switch self {
            case .matterhorn: "3a44b61de7e0d92d7b02c2df0c487c56e87e6167ac38e3ab6d5bec7f82a82df3"
            case .aristotle: "55dc9232dd6b412cea4964f19e98a56f6bd1fa7213e4e947834413cf65d8776a"
            case .fishing: "4bda0d825e8a247035de34a5cc4d9afad0ab22448cea66e4270ff48861b286a3"
            }
        }

        fileprivate var sourceSize: Size {
            switch self {
            case .matterhorn: Size(width: 2721, height: 3722)
            case .aristotle: Size(width: 3792, height: 4000)
            case .fishing: Size(width: 4000, height: 2551)
            }
        }

        var maximumEdge: Int {
            switch self {
            case .matterhorn: 2634
            case .aristotle: 2552
            case .fishing: 2135
            }
        }

        var sourceIccSha256: String? {
            self == .matterhorn ? nil : "f6bd2776ef85c5a0a9313989eb60f3db7996bc4af48453353f53840643d8e32b"
        }

        var outputDimensions: (width: Int, height: Int) {
            let source = sourceSize
            let longest = max(source.width, source.height)
            let edge = min(maximumEdge, longest)
            return (source.width * edge / longest, source.height * edge / longest)
        }

        fileprivate var settings: PrivatePaintingSettings {
            PrivatePaintingSettings(algorithm: sourceIccSha256 == nil ? NativeImagePreparation.algorithm : NativeImagePreparation.iccAlgorithm,
                maximumEdge: maximumEdge)
        }

        func canonicalSettings() throws -> Data {
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.sortedKeys]
            return try encoder.encode(settings)
        }
    }

    fileprivate struct PrivatePaintingSettings: Encodable {
        let algorithm: String
        let maximumEdge: Int
        let rounding = "floor"
        let sampling = "nearest-centre"
        let maximumEncodedBytes = EncodedPNGLimit.maximumBytes
    }

    private struct PrivatePaintingColour: Encodable {
        let kind: String
        let version: Int?
        let algorithm: String?
        let sourceIccSha256: String?
    }

    private struct PrivatePaintingPNGEncoding: Encodable {
        let algorithm = "png-rgb8-adaptive-zlib-v1"
        let version = 1
        let tool = "zlib"
        let toolVersion = String(cString: zlibVersion())
        let beforeRGB8SHA256: String
        let afterRGB8SHA256: String
    }

    private struct PrivatePaintingReceipt: Encodable {
        let recipeID = "native-private-demo-painting-rgb8"
        let recipeVersion = 1
        let sourceID: String
        let effectiveSourceOrientation = 1
        let settings: PrivatePaintingSettings
        let settingsSHA256: String
        let colour: PrivatePaintingColour
        let tool = NativeImagePreparation.tool
        let toolVersion: String
        let decoderOS = ProcessInfo.processInfo.operatingSystemVersionString
        let image: ImageReport
        let pngEncoding: PrivatePaintingPNGEncoding?
    }

    /// The larger private Aristotle raster gets one deterministic adaptive
    /// filter/DEFLATE pass. It never passes an oversize PNG to the consumer.
    static func losslessPaintingPNG(_ rgb: Data, _ width: Int, _ height: Int,
                                    _ isCurrent: @Sendable () -> Bool) throws -> Data {
        guard (1...2419).contains(width), (1...2552).contains(height),
              rgb.count == width * height * 3 else { throw Failure.limits }
        let stride = width * 3
        var filtered = Data(count: (stride + 1) * height)
        try filtered.withUnsafeMutableBytes { destination in
            try rgb.withUnsafeBytes { source in
                let input = source.bindMemory(to: UInt8.self)
                let output = destination.bindMemory(to: UInt8.self)
                for y in 0..<height {
                    try check(isCurrent)
                    let row = y * stride
                    let preceding = row - stride
                    let filteredRow = y * (stride + 1)
                    var scores = [0, 0, 0, 0, 0]
                    for x in 0..<stride {
                        let value = Int(input[row + x])
                        let left = x >= 3 ? Int(input[row + x - 3]) : 0
                        let above = y > 0 ? Int(input[preceding + x]) : 0
                        let corner = y > 0 && x >= 3 ? Int(input[preceding + x - 3]) : 0
                        for kind in scores.indices {
                            let difference = value - pngPredictor(kind, left, above, corner)
                            scores[kind] += abs(Int(Int8(truncatingIfNeeded: difference)))
                        }
                    }
                    let filter = scores.indices.min { scores[$0] < scores[$1] }!
                    output[filteredRow] = UInt8(filter)
                    for x in 0..<stride {
                        let left = x >= 3 ? Int(input[row + x - 3]) : 0
                        let above = y > 0 ? Int(input[preceding + x]) : 0
                        let corner = y > 0 && x >= 3 ? Int(input[preceding + x - 3]) : 0
                        output[filteredRow + x + 1] = UInt8(truncatingIfNeeded: Int(input[row + x]) - pngPredictor(filter, left, above, corner))
                    }
                }
            }
        }
        try check(isCurrent)
        var length = compressBound(uLong(filtered.count))
        var compressed = Data(count: Int(length))
        let code = compressed.withUnsafeMutableBytes { output in
            filtered.withUnsafeBytes { source in
                compress2(output.bindMemory(to: Bytef.self).baseAddress, &length,
                    source.bindMemory(to: Bytef.self).baseAddress, uLong(source.count), Z_BEST_COMPRESSION)
            }
        }
        guard code == Z_OK else { throw Failure.encoding }
        try check(isCurrent)
        // Signature + IHDR + sRGB + IDAT overhead + IEND = 70 bytes.
        try checkEncodedPNGSize(Int(length) + 70, stage: .lossless)
        compressed.count = Int(length)
        var png = Data([137, 80, 78, 71, 13, 10, 26, 10])
        let header = pngWord(UInt32(width)) + pngWord(UInt32(height)) + Data([8, 2, 0, 0, 0])
        appendPNGChunk("IHDR", header, to: &png)
        appendPNGChunk("sRGB", Data([0]), to: &png)
        appendPNGChunk("IDAT", compressed, to: &png)
        appendPNGChunk("IEND", Data(), to: &png)
        return png
    }

    private static func pngPredictor(_ filter: Int, _ left: Int, _ above: Int, _ corner: Int) -> Int {
        switch filter {
        case 0: return 0
        case 1: return left
        case 2: return above
        case 3: return (left + above) / 2
        default:
            let estimate = left + above - corner
            let leftDistance = abs(estimate - left), aboveDistance = abs(estimate - above), cornerDistance = abs(estimate - corner)
            if leftDistance <= aboveDistance && leftDistance <= cornerDistance { return left }
            return aboveDistance <= cornerDistance ? above : corner
        }
    }

    private static func pngWord(_ value: UInt32) -> Data {
        Data((0..<4).reversed().map { UInt8(truncatingIfNeeded: value >> ($0 * 8)) })
    }

    private static func appendPNGChunk(_ name: String, _ bytes: Data, to output: inout Data) {
        let type = Data(name.utf8)
        let seed = type.withUnsafeBytes { crc32(0, $0.bindMemory(to: Bytef.self).baseAddress, uInt(type.count)) }
        let crc = bytes.isEmpty ? seed : bytes.withUnsafeBytes {
            crc32(seed, $0.bindMemory(to: Bytef.self).baseAddress, uInt(bytes.count))
        }
        output.append(pngWord(UInt32(bytes.count)))
        output.append(type)
        output.append(bytes)
        output.append(pngWord(UInt32(crc)))
    }

    static func preparePrivatePainting(_ painting: PrivatePainting, jpeg: Data,
                                       isCurrent: @Sendable () -> Bool) throws -> (png: Data, receipt: Data) {
        try check(isCurrent)
        guard jpeg.count == painting.sourceBytes, hash(jpeg) == painting.sourceSHA256 else { throw Failure.binding }
        let profile = try embeddedICC(jpeg, isCurrent: isCurrent)
        guard profile.map(hash) == painting.sourceIccSha256 else { throw Failure.binding }

        let dimensions = painting.outputDimensions
        let output = Size(width: dimensions.width, height: dimensions.height)
        let input = Source(role: .baseColor, imageIndex: 0, jpeg: jpeg, sha256: painting.sourceSHA256)
        let pngEncoding: PNGEncoding = painting == .aristotle ? .losslessPrivatePainting : .imageIO
        let (image, decodedPNGHash) = try autoreleasepool {
            try prepareImage(input, profile: profile, allowSpatialSampling: true,
                sourceSize: painting.sourceSize, outputSize: output, maximumEdge: painting.maximumEdge,
                maximumPixels: output.width * output.height, pngEncoding: pngEncoding, isCurrent: isCurrent)
        }
        guard image.report.sourceChannels == 3 else { throw Failure.unsupportedPixels }

        let colour: PrivatePaintingColour
        if let icc = image.report.sourceICCSHA256 {
            colour = PrivatePaintingColour(kind: "icc-to-srgb", version: 1, algorithm: iccAlgorithm, sourceIccSha256: icc)
        } else {
            colour = PrivatePaintingColour(kind: "no-profile-srgb-assumed", version: nil, algorithm: nil, sourceIccSha256: nil)
        }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        let receipt = PrivatePaintingReceipt(sourceID: painting.rawValue, settings: painting.settings,
            settingsSHA256: hash(try painting.canonicalSettings()), colour: colour,
            toolVersion: profile == nil ? toolVersion : iccToolVersion, image: image.report,
            pngEncoding: decodedPNGHash.map {
                PrivatePaintingPNGEncoding(beforeRGB8SHA256: image.report.outputRGB8SHA256, afterRGB8SHA256: $0)
            })
        try check(isCurrent)
        return (image.png, try encoder.encode(receipt))
    }
}
#endif
