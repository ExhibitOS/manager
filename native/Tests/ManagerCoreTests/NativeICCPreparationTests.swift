// SPDX-License-Identifier: Apache-2.0
import XCTest
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers
import CryptoKit
@testable import ManagerCore

@MainActor
final class NativeICCPreparationTests: XCTestCase {
    private func digest(_ data: Data) -> String { SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined() }

    func testSelectedICCRequiresAdmissionAndBindsRepeatPreparation() async throws {
        let width = 16, height = 16, space = try XCTUnwrap(CGColorSpace(name: CGColorSpace.displayP3))
        var pixels = Data(count: width * height * 3)
        for y in 0..<height { for x in 0..<width {
            let values: [UInt8] = y < height / 2 ? [180, 50, 35] : [30, 65, 190]
            let offset = (y * width + x) * 3
            for channel in 0..<3 { pixels[offset + channel] = values[channel] }
        } }
        let image = try XCTUnwrap(CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 24,
            bytesPerRow: width * 3, space: space, bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue),
            provider: XCTUnwrap(CGDataProvider(data: pixels as CFData)), decode: nil, shouldInterpolate: false, intent: .defaultIntent))
        let encoded = NSMutableData()
        let destination = try XCTUnwrap(CGImageDestinationCreateWithData(encoded, UTType.jpeg.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, image, [kCGImageDestinationLossyCompressionQuality: 1.0] as CFDictionary)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
        let bytes = encoded as Data, input = ImportFileSnapshot(name: "synthetic.jpg", mime: "image/jpeg", sha256: digest(bytes), data: bytes)
        do {
            _ = try await NativePreparedJPEG.prepare(original: input, operationID: "same-operation", generation: "generation-1", isCurrent: { true })
            XCTFail("ICC must remain unavailable without selected admission")
        } catch NativeImagePreparation.Failure.unsupportedJPEG {}
        let first = try await NativePreparedJPEG.prepare(original: input, operationID: "same-operation", generation: "generation-1", isCurrent: { true }, allowICC: true)
        let second = try await NativePreparedJPEG.prepare(original: input, operationID: "same-operation", generation: "generation-1", isCurrent: { true }, allowICC: true)
        XCTAssertEqual(first.derived, second.derived); XCTAssertEqual(first.png, second.png); XCTAssertEqual(input.data, bytes)
        XCTAssertEqual(first.derived.recipe.colourMeaning, "srgb")
        let decodedSource = try XCTUnwrap(CGImageSourceCreateWithData(bytes as CFData, nil))
        let sourceImage = try XCTUnwrap(CGImageSourceCreateImageAtIndex(decodedSource, 0, nil))
        let decodedOutput = try XCTUnwrap(CGImageSourceCreateWithData(first.png as CFData, nil))
        let outputImage = try XCTUnwrap(CGImageSourceCreateImageAtIndex(decodedOutput, 0, nil))
        let srgb = try XCTUnwrap(CGColorSpace(name: CGColorSpace.sRGB))
        func rgba(_ image: CGImage, _ colourSpace: CGColorSpace) throws -> Data {
            let context = try XCTUnwrap(CGContext(data: nil, width: width, height: height, bitsPerComponent: 8,
                bytesPerRow: width * 4, space: colourSpace,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue))
            context.setBlendMode(.copy); context.interpolationQuality = .none
            context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
            return Data(bytes: try XCTUnwrap(context.data), count: width * height * 4)
        }
        let sourcePixels = try rgba(sourceImage, space), outputPixels = try rgba(outputImage, srgb)
        for y in [4, 12] {
            let offset = (y * width + 4) * 4
            let components = (0..<3).map { CGFloat(sourcePixels[offset + $0]) / 255 } + [1]
            let colour = try XCTUnwrap(CGColor(colorSpace: space, components: components))
            let expected = try XCTUnwrap(colour.converted(to: srgb, intent: .defaultIntent, options: nil)?.components)
            for channel in 0..<3 {
                XCTAssertEqual(Double(outputPixels[offset + channel]), min(255, max(0, Double(expected[channel]) * 255)), accuracy: 3.1)
            }
        }
        let conversion = try XCTUnwrap(first.derived.recipe.colourConversion)
        XCTAssertEqual(conversion.algorithm, NativeImagePreparation.iccAlgorithm)
        XCTAssertEqual(conversion.sourceIccSha256, digest(try XCTUnwrap(NativeImagePreparation.embeddedICC(bytes, isCurrent: { true }))))
        XCTAssertEqual(first.derived.recipe.settingsSha256, digest(NativePreparedJPEG.iccSettingsBytes))
        let receipt = try XCTUnwrap(JSONSerialization.jsonObject(with: first.preparationReceipt) as? [String: Any])
        XCTAssertEqual(receipt["colourTransferConverted"] as? Bool, true)
        XCTAssertEqual(receipt["originalStored"] as? Bool, false)
    }
}
