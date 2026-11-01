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
                    let source = try decodeJPEG(input, profile: profile, isCurrent: isCurrent)
                    let scale = min(1, 2048 / Double(max(source.width, source.height)))
                    let width = max(1, Int(floor(Double(source.width) * scale)))
                    let height = max(1, Int(floor(Double(source.height) * scale)))
                    let resized = width != source.width || height != source.height
                    guard !resized || sampling.boolValue else { throw Failure.settings }
                    let rgb = try resample(source.rgb, sourceWidth: source.width, sourceHeight: source.height,
                        width: width, height: height, isCurrent: isCurrent)
                    let png = try encode(rgb, width: width, height: height, role: input.role, isCurrent: isCurrent)
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
                    return Image(png: png, report: report)
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
            throw error
        }
    }
    // Internal for the focused numeric recipe test; no format/registration API.
    static func resample(_ source: Data, sourceWidth: Int, sourceHeight: Int, width: Int, height: Int,
                         isCurrent: @Sendable () -> Bool) throws -> Data {
        guard (1...4096).contains(sourceWidth), (1...4096).contains(sourceHeight),
              (1...2048).contains(width), (1...2048).contains(height), width <= sourceWidth, height <= sourceHeight,
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
    private static func decodeJPEG(_ input: Source, profile: Data?, isCurrent: @Sendable () -> Bool) throws -> Pixels {
        if profile == nil { try rejectICC(input.jpeg) }
        let options = [kCGImageSourceShouldCache: false, kCGImageSourceShouldAllowFloat: false] as CFDictionary
        guard let source = CGImageSourceCreateWithData(input.jpeg as CFData, options),
              CGImageSourceGetType(source) as String? == UTType.jpeg.identifier,
              CGImageSourceGetCount(source) == 1, CGImageSourceGetStatus(source) == .statusComplete,
              let props = CGImageSourceCopyPropertiesAtIndex(source, 0, options) as? [CFString: Any],
              let w = props[kCGImagePropertyPixelWidth] as? NSNumber, let h = props[kCGImagePropertyPixelHeight] as? NSNumber,
              w.intValue > 0, h.intValue > 0, w.intValue <= 4096, h.intValue <= 4096,
              ((props[kCGImagePropertyOrientation] as? NSNumber)?.intValue ?? 1) == 1 else { throw Failure.unsupportedJPEG }
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
    private static func encode(_ rgb: Data, width: Int, height: Int, role: Role, isCurrent: @Sendable () -> Bool) throws -> Data {
        try check(isCurrent)
        guard width <= 2048, height <= 2048, width * height <= 4_194_304,
              let provider = CGDataProvider(data: rgb as CFData),
              let image = CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 24,
                bytesPerRow: width * 3, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue),
                provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent) else { throw Failure.encoding }
        let encoded = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(encoded, UTType.png.identifier as CFString, 1, nil) else { throw Failure.encoding }
        CGImageDestinationAddImage(destination, image, nil)
        guard CGImageDestinationFinalize(destination) else { throw Failure.encoding }
        try check(isCurrent)
        guard encoded.length <= 8_388_608 else { throw Failure.limits }
        return try cleanPNG(encoded as Data, width: width, height: height, role: role, isCurrent: isCurrent)
    }
    private static func u32(_ data: Data, _ at: Int) -> Int { (0..<4).reduce(0) { ($0 << 8) | Int(data[at + $1]) } }
    /// Reuse the public worker's closed PNG chunk policy. System encoding is
    /// followed by metadata removal; unknown critical chunks fail closed.
    private static func cleanPNG(_ data: Data, width: Int, height: Int, role: Role, isCurrent: @Sendable () -> Bool) throws -> Data {
        let signature = Data([137,80,78,71,13,10,26,10])
        guard data.count >= 33, data.prefix(8) == signature else { throw Failure.encoding }
        var result = signature, offset = 8, count = 0, idat = false, ended = false
        while offset < data.count {
            try check(isCurrent)
            count += 1; guard count <= 256, data.count - offset >= 12 else { throw Failure.encoding }
            let size = u32(data, offset)
            guard size <= data.count - offset - 12 else { throw Failure.encoding }
            let end = offset + size + 12, name = data[(offset + 4)..<(offset + 8)]
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
            } else if name == Data("IDAT".utf8) { idat = true; result.append(data[offset..<end]) }
            else if name == Data("IEND".utf8) {
                guard size == 0, idat, end == data.count else { throw Failure.encoding }
                ended = true; result.append(data[offset..<end])
            } else { guard name.first.map({ $0 & 32 != 0 }) == true else { throw Failure.encoding } }
            offset = end
        }
        guard ended, result.count <= 8_388_608 else { throw Failure.encoding }
        return result
    }
    private static func decodePNG(_ png: Data, width: Int, height: Int, isCurrent: @Sendable () -> Bool) throws -> Data {
        try check(isCurrent)
        guard let source = CGImageSourceCreateWithData(png as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
              let image = CGImageSourceCreateImageAtIndex(source, 0, nil), image.width == width, image.height == height else { throw Failure.outputMismatch }
        return try pixels(image, allowGray: false, isCurrent: isCurrent).rgb
    }
}
