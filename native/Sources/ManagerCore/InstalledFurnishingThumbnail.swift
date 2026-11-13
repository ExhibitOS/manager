// SPDX-License-Identifier: Apache-2.0
import Foundation

/// Immutable distribution metadata. This is neither a tenant candidate nor a selection.
public struct InstalledFurnishingThumbnail: Equatable, Sendable {
    public static let profile = "installed-furnishing-thumbnail-v1"
    public let value: DocumentValue
    public let source: WorkspaceFurnishingThumbnailSource
    public let png: WorkspaceFurnishingThumbnailPNG
    public let renderRequestId: String

    static func canonicalHash(_ value: DocumentValue) throws -> String {
        let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        return try nativeImportSHA256(encoder.encode(value))
    }
    private static func closed(_ value: DocumentValue, _ keys: Set<String>) -> Bool {
        guard case .object(let fields) = value else { return false }
        return Set(fields.keys) == keys
    }
    public init(value: DocumentValue, descriptor: DocumentValue, furnishing: WorkspaceFurnishing) throws {
        let invalid = AuthoringFailure("CATALOG_PREVIEW_INVALID")
        guard Self.closed(value, ["profile", "source", "png", "provenance"]),
              value["profile"] == .string(Self.profile),
              Self.closed(value["provenance"], ["recipeId", "toolId", "renderRequestId"]),
              value["provenance"]["recipeId"] == .string(ViewportFurnishingThumbnailRequest.recipe),
              value["provenance"]["toolId"] == .string(ViewportFurnishingThumbnailRequest.tool),
              let request = value["provenance"]["renderRequestId"].string,
              request.utf8.count == 36, UUID(uuidString: request)?.uuidString.lowercased() == request,
              descriptor["id"] == .string(furnishing.id), descriptor["version"] == .integer(Int64(furnishing.version)),
              try JSONDecoder().decode(WorkspaceFurnishing.self, from: JSONEncoder().encode(descriptor)) == furnishing else { throw invalid }
        let source = try WorkspaceFurnishingThumbnailSource(value: value["source"], furnishing: furnishing)
        guard try Self.canonicalHash(descriptor) == source.descriptorSha256,
              try Self.canonicalHash(descriptor["rights"]) == source.rightsSha256 else { throw invalid }
        self.value = value; self.source = source; png = try WorkspaceFurnishingThumbnailPNG(value: value["png"])
        renderRequestId = request
    }
}

public struct InstalledFurnishingThumbnailImage: Sendable {
    public let furnishing: WorkspaceFurnishing
    public let binding: InstalledFurnishingThumbnail
    public let data: Data
}
