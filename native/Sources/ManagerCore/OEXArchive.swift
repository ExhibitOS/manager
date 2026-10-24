// SPDX-License-Identifier: Apache-2.0
import Foundation
import CryptoKit

/// Transport admission only. The existing Platform OEX service validates the
/// complete archive profile, current rights and every approved asset byte.
public struct NativeOEXArchive: Sendable {
    public static let maximumBytes = 64 * 1024 * 1024
    public static let mime = "application/vnd.exhibitos.oex+zip"
    public let data: Data
    public let sha256: String
    public init(data: Data) throws {
        guard data.count <= Self.maximumBytes else { throw AuthoringFailure("OEX_LIMIT") }
        guard data.count >= 22, data.prefix(4) == Data([0x50,0x4b,0x03,0x04]) else { throw AuthoringFailure("OEX_RESPONSE_INVALID") }
        self.data = data
        sha256 = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }
    public func write(to url: URL) async throws {
        guard url.isFileURL else { throw AuthoringFailure("EXPORT_WRITE_FAILED") }
        let bytes = data
        try await Task.detached(priority: .userInitiated) {
            do { try bytes.write(to: url, options: .atomic) }
            catch { throw AuthoringFailure("EXPORT_WRITE_FAILED") }
        }.value
    }
}
