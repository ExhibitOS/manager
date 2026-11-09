// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Normalize only generated viewport thumbnails. Retain pixel and color chunks
/// exactly; remove WebKit's non-rendering eXIf chunk before the final digest.
enum GeneratedThumbnailPNG {
    static func canonical(_ input: Data) throws -> Data {
        let bytes = [UInt8](input)
        func invalid() -> AuthoringFailure { AuthoringFailure("CATALOG_PREVIEW_INVALID") }
        guard bytes.count >= 33, bytes.count <= 512 * 1024,
              Array(bytes[0..<8]) == [137,80,78,71,13,10,26,10] else { throw invalid() }
        func u32(_ offset: Int) -> UInt32 {
            bytes[offset..<offset + 4].reduce(0) { ($0 << 8) | UInt32($1) }
        }
        func crc(_ range: Range<Int>) -> UInt32 {
            var value: UInt32 = 0xffffffff
            for byte in bytes[range] {
                value ^= UInt32(byte)
                for _ in 0..<8 { value = (value & 1) == 1 ? (value >> 1) ^ 0xedb88320 : value >> 1 }
            }
            return value ^ 0xffffffff
        }
        var output = Data(bytes[0..<8]), offset = 8, count = 0
        var seenHeader = false, seenSRGB = false, seenExif = false, seenIDAT = false, ended = false
        while offset < bytes.count {
            count += 1
            guard count <= 256, bytes.count - offset >= 12 else { throw invalid() }
            let length = Int(u32(offset))
            guard length <= bytes.count - offset - 12 else { throw invalid() }
            let end = offset + length + 12, name = Array(bytes[offset + 4..<offset + 8])
            guard crc((offset + 4)..<(end - 4)) == u32(end - 4) else { throw invalid() }
            let kind = String(bytes: name, encoding: .ascii) ?? ""
            switch kind {
            case "IHDR":
                guard !seenHeader, offset == 8, length == 13, u32(offset + 8) == 256, u32(offset + 12) == 256,
                      bytes[offset + 16] == 8, [2,6].contains(bytes[offset + 17]),
                      bytes[offset + 18] == 0, bytes[offset + 19] == 0, bytes[offset + 20] == 0 else { throw invalid() }
                seenHeader = true
            case "sRGB":
                guard seenHeader, !seenSRGB, !seenIDAT, length == 1, bytes[offset + 8] <= 3 else { throw invalid() }
                seenSRGB = true
            case "eXIf":
                guard seenHeader, !seenExif, !seenIDAT, (8...1024).contains(length) else { throw invalid() }
                seenExif = true
            case "IDAT":
                guard seenHeader, !ended else { throw invalid() }
                seenIDAT = true
            case "IEND":
                guard seenHeader, seenIDAT, !ended, length == 0, end == bytes.count else { throw invalid() }
                ended = true
            default: throw invalid()
            }
            if kind != "eXIf" { output.append(contentsOf: bytes[offset..<end]) }
            offset = end
        }
        guard ended else { throw invalid() }
        return output
    }
}
