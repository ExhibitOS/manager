// SPDX-License-Identifier: Apache-2.0
import Foundation

enum NativeFurnishingThumbnailRules {
    static func requireDisplay(_ rights: ArtworkMetadata.Rights, now: Date = .now) throws {
        func date(_ value: String) -> Date? {
            let formatter = ISO8601DateFormatter()
            if let parsed = formatter.date(from: value) { return parsed }
            formatter.formatOptions.insert(.withFractionalSeconds)
            return formatter.date(from: value)
        }
        guard rights.permissions.display else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        if let value = rights.validFrom {
            guard let start = date(value), start <= now else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        }
        if let value = rights.expiresAt {
            guard let end = date(value), end > now else { throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE") }
        }
    }
}
