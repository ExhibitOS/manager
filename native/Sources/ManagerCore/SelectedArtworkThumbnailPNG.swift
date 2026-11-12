// SPDX-License-Identifier: Apache-2.0
import Foundation
import ImageIO

/// Validate the issued image without normalizing or changing its selected bytes.
enum SelectedArtworkThumbnailPNG {
    private static var invalid: AuthoringFailure { AuthoringFailure("CATALOG_PREVIEW_INVALID") }

    static func validateHeaders(_ response: HTTPURLResponse, revision: Int,
                                selection: NativeArtworkThumbnailSelection) throws {
        guard response.statusCode == 200, response.mimeType?.lowercased() == "image/png",
              response.value(forHTTPHeaderField: "cache-control")?.lowercased() == "no-store",
              response.value(forHTTPHeaderField: "content-encoding").map({ $0.lowercased() == "identity" }) ?? true,
              let length = response.value(forHTTPHeaderField: "content-length"),
              let count = Int(length), String(count) == length, (1...512 * 1024).contains(count),
              response.value(forHTTPHeaderField: "x-exhibitos-artwork-revision") == String(revision),
              response.value(forHTTPHeaderField: "x-exhibitos-thumbnail-selection-version") == String(selection.selectionVersion),
              response.value(forHTTPHeaderField: "x-exhibitos-thumbnail-binding-id") == selection.bindingId?.uuidString.lowercased(),
              let sha = response.value(forHTTPHeaderField: "x-exhibitos-sha256"),
              sha.range(of: "^[a-f0-9]{64}$", options: .regularExpression) != nil else { throw invalid }
    }

    static func validate(_ data: Data, response: HTTPURLResponse) throws {
        guard data.count <= 512 * 1024,
              response.value(forHTTPHeaderField: "content-length") == String(data.count),
              response.value(forHTTPHeaderField: "x-exhibitos-sha256") == nativeImportSHA256(data) else { throw invalid }
        let bytes = [UInt8](data)
        guard bytes.count >= 45, Array(bytes.prefix(8)) == [137,80,78,71,13,10,26,10] else { throw invalid }
        func u32(_ offset: Int) -> UInt32 {
            bytes[offset..<offset + 4].reduce(0) { ($0 << 8) | UInt32($1) }
        }
        var offset = 8, chunks = 0, ended = false, seenData = false
        while offset < bytes.count {
            chunks += 1
            guard chunks <= 256, bytes.count - offset >= 12 else { throw invalid }
            let count = Int(u32(offset))
            guard count <= bytes.count - offset - 12 else { throw invalid }
            let end = offset + count + 12
            var crc: UInt32 = 0xffffffff
            for byte in bytes[offset + 4..<end - 4] {
                crc ^= UInt32(byte)
                for _ in 0..<8 { crc = crc & 1 == 1 ? (crc >> 1) ^ 0xedb88320 : crc >> 1 }
            }
            guard crc ^ 0xffffffff == u32(end - 4) else { throw invalid }
            let kind = String(bytes: bytes[offset + 4..<offset + 8], encoding: .ascii)
            if chunks == 1 {
                guard kind == "IHDR", count == 13,
                      (1...256).contains(u32(offset + 8)), (1...256).contains(u32(offset + 12)) else { throw invalid }
            } else if kind == "IHDR" { throw invalid }
            if kind == "IDAT" { seenData = true }
            if kind == "IEND" {
                guard count == 0, seenData, end == bytes.count else { throw invalid }
                ended = true
            }
            offset = end
        }
        guard ended,
              let source = CGImageSourceCreateWithData(data as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
              CGImageSourceGetCount(source) == 1,
              let image = CGImageSourceCreateImageAtIndex(source, 0, nil),
              (1...256).contains(image.width), (1...256).contains(image.height) else { throw invalid }
    }
}
