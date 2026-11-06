// SPDX-License-Identifier: Apache-2.0
import XCTest
import CryptoKit
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers
import zlib
@testable import ManagerCore

private final class ImageCurrentnessCounter: @unchecked Sendable {
    private let lock = NSLock()
    private var count = 0
    func check() -> Bool { lock.lock(); defer { lock.unlock() }; count += 1; return count < 3 }
}

private final class ImagePreparationGate: @unchecked Sendable {
    private let condition = NSCondition()
    private var entered = false, released = false
    private var continuation: CheckedContinuation<Void, Never>?
    func check() -> Bool {
        condition.lock()
        if !entered { entered = true; continuation?.resume(); continuation = nil }
        while !released { condition.wait() }
        condition.unlock()
        return true
    }
    func wait() async {
        await withCheckedContinuation { value in
            condition.lock()
            if entered { condition.unlock(); value.resume() }
            else { continuation = value; condition.unlock() }
        }
    }
    func release() { condition.lock(); released = true; condition.broadcast(); condition.unlock() }
}

@MainActor
final class NativeImagePreparationTests: XCTestCase {
    private typealias Prep = NativeImagePreparation
    private let settings = Data(#"{"algorithm":"nearest-source-colourspace-jpeg-rgb8-v1","maximumEdge":2048,"allowSpatialSampling":true}"#.utf8)
    private func hash(_ bytes: Data) -> String { SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined() }
    private func binding(_ bytes: Data? = nil) -> Prep.Binding {
        Prep.Binding(operationID: "test-operation", generation: "generation-1", originalSHA256: String(repeating: "a", count: 64),
            settingsSHA256: hash(bytes ?? settings), recipeID: "native-jpeg-role-preparation", recipeVersion: "1")
    }
    /// Synthetic test encoder only. Remove its generated profile so the fixture
    /// meets this recipe's explicitly unprofiled JPEG input contract.
    private func jpeg(width: Int = 19, height: Int = 11, gray: Bool = false, orientation: Int = 1) throws -> Data {
        let channels = gray ? 1 : 3
        var pixels = Data(count: width * height * channels)
        for y in 0..<height { for x in 0..<width { for c in 0..<channels {
            pixels[(y * width + x) * channels + c] = UInt8((x * 13 + y * 29 + c * 47) % 251)
        } } }
        let image = try XCTUnwrap(CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: channels * 8,
            bytesPerRow: width * channels, space: gray ? CGColorSpaceCreateDeviceGray() : CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue), provider: XCTUnwrap(CGDataProvider(data: pixels as CFData)),
            decode: nil, shouldInterpolate: false, intent: .defaultIntent))
        let data = NSMutableData()
        let destination = try XCTUnwrap(CGImageDestinationCreateWithData(data, UTType.jpeg.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, image, [kCGImagePropertyOrientation: orientation, kCGImageDestinationLossyCompressionQuality: 0.95] as CFDictionary)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
        let original = data as Data
        var result = Data(original.prefix(2)), at = 2
        while at + 4 <= original.count {
            let marker = original[at + 1]
            if marker == 218 { result.append(original[at...]); return result }
            let size = Int(original[at + 2]) * 256 + Int(original[at + 3]), end = at + size + 2
            guard original[at] == 255, size >= 2, end <= original.count else { throw Prep.Failure.unsupportedJPEG }
            if marker != 226 { result.append(original[at..<end]) }
            at = end
        }
        throw Prep.Failure.unsupportedJPEG
    }
    private func source(_ data: Data, role: Prep.Role = .baseColor, index: Int = 0) -> Prep.Source {
        Prep.Source(role: role, imageIndex: index, jpeg: data, sha256: hash(data))
    }
    private func chunkNames(_ png: Data) -> [String] {
        var at = 8, names: [String] = []
        while at + 12 <= png.count {
            let size = (0..<4).reduce(0) { ($0 << 8) | Int(png[at + $1]) }
            names.append(String(decoding: png[(at + 4)..<(at + 8)], as: UTF8.self)); at += size + 12
        }
        return names
    }
    private func chunk(_ name: String, _ body: Data = Data()) -> Data {
        let payload = Data(name.utf8) + body
        let crc = payload.withUnsafeBytes { crc32(0, $0.bindMemory(to: Bytef.self).baseAddress, uInt(payload.count)) }
        return Data((0..<4).reversed().map { UInt8(truncatingIfNeeded: body.count >> ($0 * 8)) })
            + payload + Data((0..<4).reversed().map { UInt8(truncatingIfNeeded: crc >> ($0 * 8)) })
    }
    private func chunks(_ png: Data) -> [(String, Data)] {
        var at = 8, result: [(String, Data)] = []
        while at + 12 <= png.count {
            let size = (0..<4).reduce(0) { ($0 << 8) | Int(png[at + $1]) }
            result.append((String(decoding: png[(at + 4)..<(at + 8)], as: UTF8.self), png.subdata(in: (at + 8)..<(at + 8 + size))))
            at += size + 12
        }
        return result
    }
    #if DEBUG
    func testPrivatePaintingProfilesPinFloorSizesAndCanonicalSettings() throws {
        let cases: [(Prep.PrivatePainting, Int, Int, String)] = [
            (.matterhorn, 1925, 2634, "ffa52ac59f13137456e5c9e82b2c1172bb962de250f9d87dfbc22e471214803a"),
            (.aristotle, 2419, 2552, "ff901dcbbd35c64520edcfd5ec8e2777fae9d84ea709d37733d8fe6c89d4afdd"),
            (.fishing, 2135, 1361, "efe4a921875cd7b2375bbdc269357ca5955560473c1f50009a56c3b4fdd59a2a"),
        ]
        XCTAssertEqual(Prep.PrivatePainting.allCases.count, cases.count)
        for (painting, width, height, settingsSHA256) in cases {
            XCTAssertEqual(painting.outputDimensions.width, width)
            XCTAssertEqual(painting.outputDimensions.height, height)
            let canonicalSettings = try painting.canonicalSettings()
            XCTAssertEqual(hash(canonicalSettings), settingsSHA256)
            // A private profile cannot authorize arbitrary ordinary preparation.
            XCTAssertThrowsError(try Prep.prepareSynchronously(sources: [source(try jpeg())],
                binding: binding(canonicalSettings), settingsBytes: canonicalSettings, isCurrent: { true })) {
                XCTAssertEqual($0 as? Prep.Failure, .settings)
            }
        }
        let forged = Data(count: Prep.PrivatePainting.matterhorn.sourceBytes)
        XCTAssertThrowsError(try Prep.preparePrivatePainting(.matterhorn, jpeg: forged, isCurrent: { true })) {
            XCTAssertEqual($0 as? Prep.Failure, .binding)
        }
    }
    #endif
    func testManyConsecutiveIDATChunksKeepTheExactCompressedStream() throws {
        let candidate = try Prep.prepareSynchronously(sources: [source(try jpeg())], binding: binding(), settingsBytes: settings, isCurrent: { true })
        let png = try XCTUnwrap(candidate.images.first?.png), parts = chunks(png)
        let header = try XCTUnwrap(parts.first(where: { $0.0 == "IHDR" })?.1)
        let compressed = parts.filter { $0.0 == "IDAT" }.reduce(into: Data()) { $0.append($1.1) }
        var fragmented = Data(png.prefix(8)) + chunk("IHDR", header)
        for index in 0..<300 {
            fragmented.append(chunk("IDAT", compressed.subdata(in: (index * compressed.count / 300)..<((index + 1) * compressed.count / 300))))
        }
        fragmented.append(chunk("IEND"))
        XCTAssertEqual(chunkNames(fragmented).count, 302)
        let cleaned = try Prep.cleanPNG(fragmented, width: 19, height: 11, role: .baseColor, isCurrent: { true })
        let output = chunks(cleaned)
        XCTAssertEqual(output.map(\.0), ["IHDR", "sRGB", "IDAT", "IEND"])
        XCTAssertEqual(output.first(where: { $0.0 == "IDAT" })?.1, compressed)
        XCTAssertEqual(cleaned, output.reduce(into: Data(png.prefix(8))) { $0.append(chunk($1.0, $1.1)) })
        XCTAssertEqual(cleaned, png)
    }
    func testIDATCoalescingRejectsDamagedOrMisorderedInput() throws {
        let candidate = try Prep.prepareSynchronously(sources: [source(try jpeg())], binding: binding(), settingsBytes: settings, isCurrent: { true })
        let png = try XCTUnwrap(candidate.images.first?.png), parts = chunks(png)
        let header = try XCTUnwrap(parts.first(where: { $0.0 == "IHDR" })?.1)
        let compressed = try XCTUnwrap(parts.first(where: { $0.0 == "IDAT" })?.1)
        let prefix = Data(png.prefix(8)) + chunk("IHDR", header)
        var badCRC = chunk("IDAT", compressed); badCRC[badCRC.count - 1] ^= 1
        var badHeader = header; badHeader[9] = 6
        let cases: [[Data]] = [
            [prefix, badCRC, chunk("IEND")],
            [Data(png.prefix(8)), chunk("IHDR", badHeader), chunk("IDAT", compressed), chunk("IEND")],
            [prefix, chunk("ABCD"), chunk("IDAT", compressed), chunk("IEND")],
            [prefix, chunk("IDAT"), chunk("tEXt"), chunk("IDAT", compressed), chunk("IEND")],
            [png, chunk("tEXt")],
            [Data(png.dropLast())],
            [Data(count: 8_388_609)]
        ]
        for parts in cases {
            let invalid = parts.reduce(into: Data()) { $0.append($1) }
            XCTAssertThrowsError(try Prep.cleanPNG(invalid, width: 19, height: 11, role: .baseColor, isCurrent: { true })) {
                XCTAssertEqual($0 as? Prep.Failure, .encoding)
            }
        }
    }
    func testAuthorizedStandaloneJPEGFixtureUsesProductionPreparation() async throws {
        let environment = ProcessInfo.processInfo.environment
        guard let path = environment["EXHIBITOS_NATIVE_JPEG_FIXTURE_PATH"],
              let expected = environment["EXHIBITOS_NATIVE_JPEG_FIXTURE_SHA256"] else {
            throw XCTSkip("An explicitly authorized local JPEG fixture is required.")
        }
        let url = URL(fileURLWithPath: path)
        let size = try url.resourceValues(forKeys: [.fileSizeKey]).fileSize ?? 0
        guard size > 0, size <= ImportFileSnapshot.limit else { throw Prep.Failure.limits }
        let bytes = try Data(contentsOf: url)
        guard hash(bytes) == expected else { throw Prep.Failure.binding }
        let original = ImportFileSnapshot(name: "authorized-fixture.jpg", mime: "image/jpeg", sha256: expected, data: bytes)
        do {
            let result = try await NativePreparedJPEG.prepare(original: original,
                operationID: "authorized-fixture", generation: "diagnosis-1", isCurrent: { true })
            XCTAssertEqual(result.originalSHA256, expected)
            XCTAssertTrue(result.derived.valid)
            XCTAssertEqual(result.derived.sha256, hash(result.png))
            XCTAssertEqual(chunkNames(result.png), ["IHDR", "sRGB", "IDAT", "IEND"])
            XCTAssertEqual(original.data, bytes)
        } catch let failure as Prep.Failure {
            XCTFail("Production JPEG preparation failed: \(failure)")
        }
    }
    func testPreparedStandaloneJPEGKeepsOriginalAndDerivedBindingsDistinct() async throws {
        let bytes = try jpeg()
        let original = ImportFileSnapshot(name: "synthetic.jpg", mime: "image/jpeg", sha256: hash(bytes), data: bytes)
        let result = try await NativePreparedJPEG.prepare(original: original, operationID: "typed-import", generation: "generation-2", isCurrent: { true })
        XCTAssertEqual(result.originalSHA256, original.sha256); XCTAssertTrue(result.derived.valid)
        XCTAssertNotEqual(result.derived.sha256, original.sha256)
        XCTAssertEqual(result.derived.mime, "image/png"); XCTAssertEqual(result.derived.sha256, hash(result.png))
        XCTAssertEqual(result.derived.recipe.settingsSha256, hash(NativePreparedJPEG.settingsBytes))
        XCTAssertEqual(result.derived.recipe.sourcePixelsSha256, result.derived.recipe.outputPixelsSha256)
        let receipt = try XCTUnwrap(JSONSerialization.jsonObject(with: result.preparationReceipt) as? [String: Any])
        XCTAssertEqual(receipt["originalStored"] as? Bool, false); XCTAssertEqual(receipt["consumerAdmitted"] as? Bool, false)
        XCTAssertEqual(original.data, bytes)
        if let path = ProcessInfo.processInfo.environment["EXHIBITOS_NATIVE_PREPARATION_EVIDENCE"] {
            let root = URL(fileURLWithPath: path, isDirectory: true)
            guard !FileManager.default.fileExists(atPath: root.path) else { throw AuthoringFailure("TEST_OUTPUT_EXISTS") }
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
            try bytes.write(to: root.appendingPathComponent("synthetic-native-original.jpg"), options: .withoutOverwriting)
            try result.png.write(to: root.appendingPathComponent("synthetic-native-derived.png"), options: .withoutOverwriting)
            let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys]
            try encoder.encode(result.derived).write(to: root.appendingPathComponent("native-derived-descriptor.json"), options: .withoutOverwriting)
            try result.preparationReceipt.write(to: root.appendingPathComponent("native-preparation-receipt.json"), options: .withoutOverwriting)
        }
    }
    func testBaseAndNormalPreserveDecodedCodesAndOnlyBaseTagsSRGB() throws {
        let bytes = try jpeg()
        let candidate = try Prep.prepareSynchronously(sources: [source(bytes), source(bytes, role: .normal, index: 1)],
            binding: binding(), settingsBytes: settings, isCurrent: { true })
        XCTAssertEqual(candidate.images.count, 2)
        for image in candidate.images {
            XCTAssertEqual(image.report.sourceRGB8SHA256, image.report.outputRGB8SHA256)
            XCTAssertEqual(image.report.sourceDecodedSamplesSHA256, image.report.outputRGB8SHA256)
            XCTAssertEqual(image.report.sourceJPEGSHA256, hash(bytes)); XCTAssertFalse(image.report.spatialSampling)
            XCTAssertEqual(image.report.outputPNGSHA256, hash(image.png))
            XCTAssertTrue(Set(chunkNames(image.png)).isSubset(of: ["IHDR", "sRGB", "IDAT", "IEND"]))
            XCTAssertEqual(chunkNames(image.png).contains("sRGB"), image.report.role == .baseColor)
        }
        let receipt = try XCTUnwrap(JSONSerialization.jsonObject(with: candidate.receipt) as? [String: Any])
        for key in ["originalStored", "sourceContainerBytesVerified", "sourceContainerMembershipVerified", "consumerAdmitted", "qualityApproved", "normalRenormalized", "colourTransferConverted"] {
            XCTAssertEqual(receipt[key] as? Bool, false)
        }
    }
    func testNearestCentrePreservesRawChannelsWithoutFlipOrNormalization() throws {
        var original = Data()
        for n in 0..<8 {
            let base = UInt8(n * 10)
            original.append(contentsOf: [base + 1, base + 2, base + 3])
        }
        // 4×2 -> 2×1 selects source centres (1,1) and (3,1), indices5 and7.
        let sampled = try Prep.resample(original, sourceWidth: 4, sourceHeight: 2, width: 2, height: 1, isCurrent: { true })
        XCTAssertEqual(sampled, Data([51,52,53,71,72,73]))
        XCTAssertEqual(try Prep.resample(original, sourceWidth: 4, sourceHeight: 2, width: 4, height: 2, isCurrent: { true }), original)
    }
    func testSamplingUsesWholeRectangleFloorDimensionsAndRequiresConsent() throws {
        let bytes = try jpeg(width: 2049, height: 3)
        let output = try Prep.prepareSynchronously(sources: [source(bytes)], binding: binding(), settingsBytes: settings, isCurrent: { true })
        XCTAssertEqual(output.images[0].report.outputWidth, 2048); XCTAssertEqual(output.images[0].report.outputHeight, 2)
        XCTAssertTrue(output.images[0].report.spatialSampling)
        let noSampling = Data(#"{"algorithm":"nearest-source-colourspace-jpeg-rgb8-v1","maximumEdge":2048,"allowSpatialSampling":false}"#.utf8)
        XCTAssertThrowsError(try Prep.prepareSynchronously(sources: [source(bytes)], binding: binding(noSampling), settingsBytes: noSampling, isCurrent: { true })) {
            XCTAssertEqual($0 as? Prep.Failure, .settings)
        }
    }
    func testGrayOnlyAOExpandsEquallyAndOtherRolesRejectGray() throws {
        let bytes = try jpeg(gray: true)
        let result = try Prep.prepareSynchronously(sources: [source(bytes, role: .occlusion)], binding: binding(), settingsBytes: settings, isCurrent: { true })
        XCTAssertTrue(result.images[0].report.grayExpandedToRGB)
        XCTAssertEqual(result.images[0].report.sourceChannels, 1)
        XCTAssertEqual(result.images[0].report.sourceRGB8SHA256, result.images[0].report.outputRGB8SHA256)
        for role in [Prep.Role.baseColor, .normal] {
            XCTAssertThrowsError(try Prep.prepareSynchronously(sources: [source(bytes, role: role)], binding: binding(), settingsBytes: settings, isCurrent: { true })) {
                XCTAssertEqual($0 as? Prep.Failure, .unsupportedPixels)
            }
        }
    }
    func testRejectsICCNonUprightExifWrongHashDuplicateRolesAndOversize() throws {
        let bytes = try jpeg()
        let icc = Data([255,216,255,226,0,16]) + Data("ICC_PROFILE\0".utf8) + Data([1,1]) + bytes.dropFirst(2)
        for input in [icc, try jpeg(orientation: 6), try jpeg(width: 4097, height: 1), Data("not JPEG".utf8)] {
            XCTAssertThrowsError(try Prep.prepareSynchronously(sources: [source(input)], binding: binding(), settingsBytes: settings, isCurrent: { true })) {
                XCTAssertEqual($0 as? Prep.Failure, .unsupportedJPEG)
            }
        }
        let bad = Prep.Source(role: .baseColor, imageIndex: 0, jpeg: bytes, sha256: String(repeating: "0", count: 64))
        XCTAssertThrowsError(try Prep.prepareSynchronously(sources: [bad], binding: binding(), settingsBytes: settings, isCurrent: { true })) { XCTAssertEqual($0 as? Prep.Failure, .binding) }
        XCTAssertThrowsError(try Prep.prepareSynchronously(sources: [source(bytes), source(bytes, index: 1)], binding: binding(), settingsBytes: settings, isCurrent: { true })) { XCTAssertEqual($0 as? Prep.Failure, .limits) }
        let changed = settings + Data(" ".utf8)
        XCTAssertThrowsError(try Prep.prepareSynchronously(sources: [source(bytes)], binding: binding(), settingsBytes: changed, isCurrent: { true })) { XCTAssertEqual($0 as? Prep.Failure, .binding) }
    }
    func testRGBAOKeepsAuthoredChannelsWithoutGrayConversion() throws {
        let bytes = try jpeg()
        let result = try Prep.prepareSynchronously(sources: [source(bytes, role: .occlusion)],
            binding: binding(), settingsBytes: settings, isCurrent: { true })
        XCTAssertFalse(result.images[0].report.grayExpandedToRGB)
        XCTAssertEqual(result.images[0].report.sourceChannels, 3)
        XCTAssertEqual(result.images[0].report.sourceRGB8SHA256, result.images[0].report.outputRGB8SHA256)
    }
    func testAsyncErrorBoundaryRechecksCurrentnessAfterDetachedFailure() async {
        let counter = ImageCurrentnessCounter()
        do {
            // Two worker checks admit the binding then preserve the limits error.
            // Supersession at the third, parent check must override that failure.
            _ = try await Prep.prepare(sources: [], binding: binding(), settingsBytes: settings, isCurrent: { counter.check() })
            XCTFail("Stale failure must not be accepted")
        } catch let error as Prep.Failure { XCTAssertEqual(error, .cancelledOrStale) }
        catch { XCTFail("Unexpected error") }
    }
    func testStalenessPrecedesUnsupportedError() {
        XCTAssertThrowsError(try Prep.prepareSynchronously(sources: [], binding: binding(), settingsBytes: Data(), isCurrent: { false })) {
            XCTAssertEqual($0 as? Prep.Failure, .cancelledOrStale)
        }
    }
    func testAsyncCancellationRetainsAndDrainsActualDetachedTask() async throws {
        let bytes = try jpeg(), input = source(bytes), bound = binding(), settings = settings, gate = ImagePreparationGate()
        let running = Task { try await Prep.prepare(sources: [input], binding: bound, settingsBytes: settings, isCurrent: { gate.check() }) }
        await gate.wait()
        running.cancel()
        var waiterStarted = false, returned = false
        let waiter = Task { waiterStarted = true; _ = await running.result; returned = true }
        while !waiterStarted { await Task.yield() }
        XCTAssertFalse(returned)
        gate.release()
        do { _ = try await running.value; XCTFail("Cancelled candidate must not return") }
        catch let error as Prep.Failure { XCTAssertEqual(error, .cancelledOrStale) }
        await waiter.value; XCTAssertTrue(returned)
    }
}
