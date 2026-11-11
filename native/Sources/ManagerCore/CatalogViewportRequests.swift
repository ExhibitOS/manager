// SPDX-License-Identifier: Apache-2.0
import Foundation

public struct ViewportThumbnailRequest: Encodable, Sendable {
    public let version = 1
    public let epoch: String, tenantId: String, principalId: String, requestId: String, artworkId: String
    public let sequence: Int, expectedRevision: Int
    public init(frame: ViewportFrame, artworkId: String, expectedRevision: Int) throws {
        guard !frame.disabled, frame.sequence > 0, expectedRevision > 0, expectedRevision <= 2_147_483_647,
              let artwork = UUID(uuidString: artworkId), UUID(uuidString: frame.epoch) != nil,
              UUID(uuidString: frame.tenantId) != nil, UUID(uuidString: frame.principalId) != nil else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
        epoch = frame.epoch; sequence = frame.sequence; tenantId = frame.tenantId; principalId = frame.principalId
        requestId = UUID().uuidString.lowercased(); self.artworkId = artwork.uuidString.lowercased(); self.expectedRevision = expectedRevision
    }
}
public struct ViewportRenderedThumbnail: Sendable {
    public static let profile = "rendered-display-preview-v1"
    public static let recipe = "artwork-glb-card-256-v1"
    public static let tool = "exhibitos-three-0.186.1-thumbnail-v1"
    public let artworkId: String, revision: Int
    public let sourceAssetId: String, sourceSha256: String
    /// The model bytes used by the renderer may come from a derivative of the approved primary asset.
    public let displaySha256: String
    public let recipeId: String, toolId: String, modelBytes: Int
    public let pngData: Data, pngSha256: String
}

public struct ViewportThumbnailReply: Decodable, Sendable {
    public let version: Int, epoch: String, sequence: Int, tenantId: String, principalId: String, requestId: String
    public let artworkId: String, revision: Int, pngBase64: String, width: Int, height: Int
    public let sourceAssetId: String?, sourceSha256: String?, displaySha256: String?, pngSha256: String?
    public let recipeId: String?, toolId: String?, modelBytes: Int?

    public func pngData(matching request: ViewportThumbnailRequest) throws -> Data {
        guard version == 1, epoch == request.epoch, sequence == request.sequence, tenantId == request.tenantId,
              principalId == request.principalId, requestId == request.requestId, artworkId == request.artworkId, revision == request.expectedRevision,
              (1...256).contains(width), (1...256).contains(height), pngBase64.utf8.count <= 699_052,
              let bytes = Data(base64Encoded: pngBase64), bytes.base64EncodedString() == pngBase64, bytes.count <= 512 * 1024, bytes.count >= 33,
              bytes.prefix(16) == Data([137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82]) else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
        func dimension(_ offset: Int) -> Int { bytes[offset..<offset+4].reduce(0) { ($0 << 8) | Int($1) } }
        guard dimension(16) == width, dimension(20) == height else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
        return bytes
    }

    public func renderedThumbnail(matching request: ViewportThumbnailRequest) throws -> ViewportRenderedThumbnail {
        guard width == 256, height == 256,
              let sourceAssetId, UUID(uuidString: sourceAssetId) != nil,
              let sourceSha256, nativeImportDigest(sourceSha256),
              let displaySha256, nativeImportDigest(displaySha256),
              recipeId == ViewportRenderedThumbnail.recipe, toolId == ViewportRenderedThumbnail.tool,
              let modelBytes, (28...16 * 1024 * 1024).contains(modelBytes) else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
        let original = try pngData(matching: request)
        guard pngSha256 == nativeImportSHA256(original) else { throw AuthoringFailure("CATALOG_PREVIEW_INVALID") }
        let bytes = try GeneratedThumbnailPNG.canonical(original)
        let digest = nativeImportSHA256(bytes)
        return ViewportRenderedThumbnail(artworkId: artworkId, revision: revision, sourceAssetId: sourceAssetId,
            sourceSha256: sourceSha256, displaySha256: displaySha256,
            recipeId: ViewportRenderedThumbnail.recipe, toolId: ViewportRenderedThumbnail.tool, modelBytes: modelBytes,
            pngData: bytes, pngSha256: digest)
    }
}
public struct NativeRenderedArtworkThumbnailSelectionReceipt: Codable, Sendable {
    public let id: UUID, thumbnailAssetId: UUID
    public let thumbnailSha256: String
    public let selectionVersion: Int, selected: Bool
}
public struct NativeArtworkThumbnailSelection: Decodable, Sendable {
    public let selectionVersion: Int, bindingId: UUID?
}
public struct ViewportFurnishingThumbnailRequest: Codable, Equatable, Sendable {
    public static let capability = "furnishing-registration-thumbnail-v1"
    public static let recipe = "furnishing-glb-card-256-v1"
    public static let tool = "exhibitos-three-0.186.1-thumbnail-v1"

    public let version: Int, sequence: Int, furnishingVersion: Int, modelBytes: Int
    public let epoch: String, tenantId: String, principalId: String, requestId: String
    public let furnishingId: String, modelSha256: String, recipeId: String, toolId: String

    public init(frame: ViewportFrame, furnishing: WorkspaceFurnishing) throws {
        let model = furnishing.details.model
        guard !frame.disabled, frame.sequence > 0, furnishing.version == 1,
              model.mime == "model/gltf-binary", (28...32 * 1024 * 1024).contains(model.bytes),
              nativeImportDigest(model.sha256), furnishing.details.rights.permissions.display,
              [frame.epoch, frame.tenantId, frame.principalId, furnishing.id].allSatisfy({ UUID(uuidString: $0) != nil }) else {
            throw AuthoringFailure("CATALOG_PREVIEW_UNAVAILABLE")
        }
        version = 1; sequence = frame.sequence; furnishingVersion = furnishing.version; modelBytes = model.bytes
        epoch = frame.epoch; tenantId = frame.tenantId; principalId = frame.principalId
        requestId = UUID().uuidString.lowercased(); furnishingId = furnishing.id; modelSha256 = model.sha256
        recipeId = Self.recipe; toolId = Self.tool
    }
}

public struct ViewportFurnishingThumbnailReply: Decodable, Sendable {
    private let binding: ViewportFurnishingThumbnailRequest
    private let pngBase64: String, pngSha256: String
    private let width: Int, height: Int

    public init(from decoder: Decoder) throws {
        binding = try ViewportFurnishingThumbnailRequest(from: decoder)
        let fields = try decoder.container(keyedBy: CodingKeys.self)
        pngBase64 = try fields.decode(String.self, forKey: .pngBase64)
        pngSha256 = try fields.decode(String.self, forKey: .pngSha256)
        width = try fields.decode(Int.self, forKey: .width)
        height = try fields.decode(Int.self, forKey: .height)
    }

    public func pngData(matching request: ViewportFurnishingThumbnailRequest) throws -> Data {
        guard binding == request, width == 256, height == 256, pngBase64.utf8.count <= 699_052,
              let bytes = Data(base64Encoded: pngBase64), bytes.base64EncodedString() == pngBase64,
              bytes.count <= 512 * 1024, nativeImportSHA256(bytes) == pngSha256 else {
            throw AuthoringFailure("CATALOG_PREVIEW_INVALID")
        }
        return try GeneratedThumbnailPNG.canonical(bytes)
    }

    private enum CodingKeys: String, CodingKey { case pngBase64, pngSha256, width, height }
}

public struct ViewportFindSelectionRequest: Encodable, Sendable {
    public let version = 1
    public let epoch: String, tenantId: String, principalId: String, requestId: String
    public let sequence: Int
    public let selection: ViewportSelection
    public init(frame: ViewportFrame) throws {
        guard !frame.disabled, frame.sequence > 0, let selection = frame.selection, selection.kind == "placement", selection.belongs(to: frame.draft),
              UUID(uuidString: frame.epoch) != nil, UUID(uuidString: frame.tenantId) != nil, UUID(uuidString: frame.principalId) != nil else { throw AuthoringFailure("VIEWPORT_NOT_READY") }
        epoch = frame.epoch; sequence = frame.sequence; tenantId = frame.tenantId; principalId = frame.principalId
        requestId = UUID().uuidString.lowercased(); self.selection = selection
    }
}
public struct ViewportFindSelectionReply: Decodable, Sendable {
    public let version: Int, epoch: String, sequence: Int, tenantId: String, principalId: String, requestId: String
    public let selection: ViewportSelection
    public let accepted: Bool
    public func matches(_ request: ViewportFindSelectionRequest) -> Bool {
        accepted && version == 1 && epoch == request.epoch && sequence == request.sequence && tenantId == request.tenantId && principalId == request.principalId && requestId == request.requestId && selection == request.selection
    }
}
