// SPDX-License-Identifier: Apache-2.0
import Foundation

/// A bounded furnishing capture submission, not proof that pixels derive from
/// a model. The caller/server must recheck the live owner, source and rights.
public struct WorkspaceFurnishingThumbnailSubmission: Sendable {
    public static let kind = "workspace-furnishing-thumbnail-v1"
    public let workspaceId: String
    public let request: ViewportFurnishingThumbnailRequest
    public let pngData: Data
    public let pngSha256: String
    public var pngBytes: Int { pngData.count }
    /// Exact POST payload. It contains the PNG; avoid logging this value/body.
    public let value: DocumentValue
    public let body: Data

    private static var invalid: AuthoringFailure { AuthoringFailure("CATALOG_PREVIEW_INVALID") }
    private static let requestKeys: Set<String> = ["version", "epoch", "sequence", "tenantId", "principalId", "requestId",
        "furnishingId", "furnishingVersion", "modelSha256", "modelBytes", "recipeId", "toolId"]

    /// Accept either the original SDK capture (png.base64) or its exact retained
    /// metadata plus explicitly supplied bytes. No path in capture is trusted.
    /// Native revision/edit-version/ETag are capture provenance, not a scene CAS.
    public init(capture: DocumentValue, pngData suppliedPNG: Data? = nil,
                document: NativeDocumentState, furnishing: WorkspaceFurnishing) throws {
        guard case .object(let root) = capture,
              Set(root.keys) == ["workspaceId", "nativeRevision", "editVersion", "baseETag", "request", "png"],
              capture["workspaceId"] == .string(document.workspaceId),
              [document.workspaceId, document.tenantId, document.principalId].allSatisfy({ UUID(uuidString: $0) != nil }),
              let revision = capture["nativeRevision"].integer, revision >= 0,
              let editVersion = capture["editVersion"].integer, editVersion >= 0,
              let etag = capture["baseETag"].string, !etag.isEmpty, etag.utf8.count <= 1024,
              case .object(let render) = capture["request"], Set(render.keys) == Self.requestKeys,
              case .object(let png) = capture["png"],
              Set(png.keys).subtracting(["base64"]) == ["mime", "width", "height", "bytes", "sha256"],
              capture["png"]["mime"] == .string("image/png"),
              capture["png"]["width"] == .integer(256), capture["png"]["height"] == .integer(256),
              let count = capture["png"]["bytes"].integer, (33...512 * 1024).contains(count),
              let hash = capture["png"]["sha256"].string, nativeImportDigest(hash) else { throw Self.invalid }

        let request = try Self.checkedRenderRequest(capture["request"], document: document, furnishing: furnishing)
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]

        let bytes: Data
        if let encoded = png["base64"] {
            guard let string = encoded.string, string.utf8.count <= 699_052,
                  let decoded = Data(base64Encoded: string), decoded.base64EncodedString() == string,
                  suppliedPNG == nil || suppliedPNG == decoded else { throw Self.invalid }
            bytes = decoded
        } else {
            guard let suppliedPNG else { throw Self.invalid }; bytes = suppliedPNG
        }
        guard Int64(bytes.count) == count, bytes.count <= 512 * 1024, nativeImportSHA256(bytes) == hash else { throw Self.invalid }
        // Reuse the viewport's typed reply and bounded PNG parser. SDK captures
        // are already canonical: submission never silently rewrites their bytes.
        var reply = render
        reply["pngBase64"] = .string(bytes.base64EncodedString()); reply["pngSha256"] = .string(hash)
        reply["width"] = .integer(256); reply["height"] = .integer(256)
        let checked = try JSONDecoder().decode(ViewportFurnishingThumbnailReply.self, from: encoder.encode(DocumentValue.object(reply)))
        guard try checked.pngData(matching: request) == bytes else { throw Self.invalid }

        let value: DocumentValue = .object(["kind": .string(Self.kind), "requestId": .string(request.requestId),
            "renderRequest": capture["request"], "pngSha256": .string(hash), "pngBytes": .integer(Int64(bytes.count)),
            "pngBase64": .string(bytes.base64EncodedString())])
        self.workspaceId = document.workspaceId; self.request = request
        self.pngData = bytes; self.pngSha256 = hash; self.value = value
        self.body = try encoder.encode(value)
    }

    static func checkedRenderRequest(_ raw: DocumentValue, document: NativeDocumentState,
                                     furnishing: WorkspaceFurnishing) throws -> ViewportFurnishingThumbnailRequest {
        let model = furnishing.details.model
        guard case .object(let fields) = raw, Set(fields.keys) == requestKeys else { throw invalid }
        guard raw["version"] == .integer(1), let sequence = raw["sequence"].integer, sequence > 0,
              ["epoch", "requestId", "tenantId", "principalId", "furnishingId"].allSatisfy({ key in
                  guard let id = raw[key].string, id.utf8.count == 36 else { return false }
                  return UUID(uuidString: id) != nil
              }),
              raw["tenantId"] == .string(document.tenantId), raw["principalId"] == .string(document.principalId),
              furnishing.version == 1, raw["furnishingId"] == .string(furnishing.id), raw["furnishingVersion"] == .integer(1),
              model.mime == "model/gltf-binary", (28...32 * 1024 * 1024).contains(model.bytes),
              nativeImportDigest(model.sha256), raw["modelSha256"] == .string(model.sha256), raw["modelBytes"] == .integer(Int64(model.bytes)),
              raw["recipeId"] == .string(ViewportFurnishingThumbnailRequest.recipe),
              raw["toolId"] == .string(ViewportFurnishingThumbnailRequest.tool) else { throw Self.invalid }
        try NativeFurnishingThumbnailRules.requireDisplay(furnishing.details.rights)
        return try JSONDecoder().decode(ViewportFurnishingThumbnailRequest.self, from: JSONEncoder().encode(raw))
    }
}
