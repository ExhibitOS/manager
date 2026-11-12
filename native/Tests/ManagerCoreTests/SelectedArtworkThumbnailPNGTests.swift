// SPDX-License-Identifier: Apache-2.0
import Foundation
import XCTest
import CoreGraphics
import ImageIO
@testable import ManagerCore

final class SelectedArtworkThumbnailPNGTests: XCTestCase {
    private let selection = NativeArtworkThumbnailSelection(selectionVersion: 1,
        bindingId: UUID(uuidString: "00000000-0000-4000-8000-000000000001"))

    private func png(_ size: Int) throws -> Data {
        let pixels = Data(repeating: 128, count: size * size * 4)
        let provider = try XCTUnwrap(CGDataProvider(data: pixels as CFData))
        let image = try XCTUnwrap(CGImage(width: size, height: size, bitsPerComponent: 8, bitsPerPixel: 32,
            bytesPerRow: size * 4, space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent))
        let output = NSMutableData()
        let destination = try XCTUnwrap(CGImageDestinationCreateWithData(output, "public.png" as CFString, 1, nil))
        CGImageDestinationAddImage(destination, image, nil)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
        return output as Data
    }

    private func response(_ data: Data, changes: [String: String] = [:]) throws -> HTTPURLResponse {
        var headers = ["Content-Type": "image/png", "Content-Length": String(data.count),
            "Cache-Control": "no-store", "x-exhibitos-sha256": nativeImportSHA256(data),
            "x-exhibitos-artwork-revision": "2", "x-exhibitos-thumbnail-selection-version": "1",
            "x-exhibitos-thumbnail-binding-id": selection.bindingId!.uuidString.lowercased()]
        headers.merge(changes) { _, changed in changed }
        return try XCTUnwrap(HTTPURLResponse(url: URL(string: "http://127.0.0.1:18091/image")!,
            statusCode: 200, httpVersion: "HTTP/1.1", headerFields: headers))
    }

    func testAcceptsExactIssuedPNGAtBothDimensionBoundsWithoutChangingBytes() throws {
        for size in [1, 256] {
            let data = try png(size), response = try response(data)
            try SelectedArtworkThumbnailPNG.validateHeaders(response, revision: 2, selection: selection)
            try SelectedArtworkThumbnailPNG.validate(data, response: response)
        }
    }

    func testRejectsWrongBindingRevisionVersionMimeCacheEncodingAndLength() throws {
        let data = try png(8)
        for change in [["x-exhibitos-thumbnail-binding-id": UUID().uuidString.lowercased()],
                       ["x-exhibitos-artwork-revision": "3"], ["x-exhibitos-thumbnail-selection-version": "2"],
                       ["Content-Type": "model/gltf-binary"], ["Cache-Control": "public"],
                       ["Content-Encoding": "gzip"], ["Content-Length": "524289"],
                       ["Content-Length": "01"], ["x-exhibitos-sha256": "invalid"]] {
            XCTAssertThrowsError(try SelectedArtworkThumbnailPNG.validateHeaders(response(data, changes: change),
                revision: 2, selection: selection))
        }
        XCTAssertThrowsError(try SelectedArtworkThumbnailPNG.validate(data,
            response: response(data, changes: ["Content-Length": String(data.count + 1)])))
        XCTAssertThrowsError(try SelectedArtworkThumbnailPNG.validate(data,
            response: response(data, changes: ["x-exhibitos-sha256": String(repeating: "0", count: 64)])))
    }

    func testRejectsCorruptCRCTruncationTrailingBytesAndOversizedDimensionsEvenWithMatchingDigest() throws {
        let original = try png(8)
        var corrupt = original; corrupt[29] ^= 1
        var trailing = original; trailing.append(0)
        for data in [corrupt, Data(original.dropLast()), trailing, try png(257)] {
            XCTAssertThrowsError(try SelectedArtworkThumbnailPNG.validate(data, response: response(data)))
        }
    }
}
