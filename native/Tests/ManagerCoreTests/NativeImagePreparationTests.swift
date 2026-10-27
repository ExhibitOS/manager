// SPDX-License-Identifier: Apache-2.0
import XCTest
import CryptoKit
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers
@testable import ManagerCore

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
